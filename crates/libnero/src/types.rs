use anyhow::bail;
use nero_extensions::{WasmExtension, types::MediaResource};
use nero_processor::Processor;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::{file_resolver::TorrentFileResolver, utils::AsyncTryFromWithProcessor};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page<T> {
    items: Vec<T>,
    has_next_page: bool,
}

impl<T, U> AsyncTryFromWithProcessor<nero_extensions::types::Page<T>> for Page<U>
where
    U: AsyncTryFromWithProcessor<T>,
{
    async fn async_try_from_with_processor(
        page: nero_extensions::types::Page<T>,
        processor: &Processor,
    ) -> anyhow::Result<Self> {
        let mut items = Vec::with_capacity(page.items.len());
        for item in page.items {
            items.push(U::async_try_from_with_processor(item, processor).await?);
        }
        Ok(Self {
            items,
            has_next_page: page.has_next_page,
        })
    }
}

pub type SeriesPage = Page<Series>;
pub type EpisodesPage = Page<Episode>;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Series {
    id: String,
    title: String,
    poster_url: Option<Url>,
    synopsis: Option<String>,
    r#type: Option<String>,
}

impl AsyncTryFromWithProcessor<nero_extensions::types::Series> for Series {
    async fn async_try_from_with_processor(
        series: nero_extensions::types::Series,
        processor: &Processor,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            id: series.id,
            title: series.title,
            poster_url: match series.poster_resource {
                Some(MediaResource::HttpRequest(req)) => {
                    Some(processor.register_image_request(*req).await?)
                }
                Some(MediaResource::MagnetUri(_)) => {
                    bail!("Magnet URIs are not supported for images");
                }
                None => None,
            },
            synopsis: series.synopsis,
            r#type: series.r#type,
        })
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Episode {
    id: String,
    number: u16,
    title: Option<String>,
    thumbnail_url: Option<Url>,
    description: Option<String>,
}

impl AsyncTryFromWithProcessor<nero_extensions::types::Episode> for Episode {
    async fn async_try_from_with_processor(
        episode: nero_extensions::types::Episode,
        processor: &Processor,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            id: episode.id,
            number: episode.number,
            title: episode.title,
            thumbnail_url: match episode.thumbnail_resource {
                Some(MediaResource::HttpRequest(req)) => {
                    Some(processor.register_image_request(*req).await?)
                }
                Some(MediaResource::MagnetUri(_)) => {
                    bail!("Magnet URIs are not supported for images");
                }
                None => None,
            },
            description: episode.description,
        })
    }
}

type Resolution = (u16, u16);

#[derive(Debug, Serialize)]
pub struct Video {
    url: Url,
    server: String,
    resolution: Resolution,
}

impl Video {
    pub async fn from_extension_video(
        extension_video: nero_extensions::types::Video,
        extension: &WasmExtension,
        processor: &Processor,
        requested_series_id: &str,
        requested_episode_number: u32,
    ) -> anyhow::Result<Self> {
        let url = match extension_video.media_resource {
            nero_extensions::types::MediaResource::HttpRequest(request) => {
                match processor.register_video_request(*request.clone()).await {
                    Ok(url) => Ok(url),
                    Err(e) if e.to_string().contains("torrent") => {
                        Self::handle_torrent_source(
                            processor,
                            extension,
                            nero_processor::TorrentSource::Http(request.clone()),
                            requested_series_id,
                            requested_episode_number,
                        )
                        .await
                    }
                    Err(e) => Err(e),
                }
            }
            nero_extensions::types::MediaResource::MagnetUri(uri) => {
                Self::handle_torrent_source(
                    processor,
                    extension,
                    nero_processor::TorrentSource::MagnetUri(uri.clone()),
                    requested_series_id,
                    requested_episode_number,
                )
                .await
            }
        }?;

        Ok(Self {
            url,
            server: extension_video.server,
            resolution: extension_video.resolution,
        })
    }

    async fn handle_torrent_source(
        processor: &Processor,
        extension: &WasmExtension,
        torrent_source: nero_processor::TorrentSource,
        requested_series_id: &str,
        requested_episode_number: u32,
    ) -> anyhow::Result<Url> {
        let torrent_backend = processor
            .torrent_backend()
            .await
            .ok_or(anyhow::anyhow!("torrent support is not enabled."))?;

        let files = torrent_backend.list_files(&torrent_source).await?;

        let video_files = files
            .into_iter()
            .filter(|f| {
                mime_guess::from_path(&f.path)
                    .first()
                    .map(|mime| mime.type_() == mime_guess::mime::VIDEO)
                    .unwrap_or(false)
            })
            .collect::<Vec<_>>();

        let target_index = video_files
            .find_episode(extension, requested_series_id, requested_episode_number)
            .await?
            .ok_or(anyhow::anyhow!("Episode not found"))?;

        processor
            .register_torrent(torrent_source, vec![target_index])
            .await
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Filter {
    id: String,
    display_name: String,
}

impl From<nero_extensions::types::Filter> for Filter {
    fn from(filter: nero_extensions::types::Filter) -> Self {
        Self {
            id: filter.id,
            display_name: filter.display_name,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilterCategory {
    id: String,
    display_name: String,
    filters: Vec<Filter>,
}

impl From<nero_extensions::types::FilterCategory> for FilterCategory {
    fn from(category: nero_extensions::types::FilterCategory) -> Self {
        Self {
            id: category.id,
            display_name: category.display_name,
            filters: category.filters.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct SearchFilter {
    id: String,
    values: Vec<String>,
}

impl From<SearchFilter> for nero_extensions::types::SearchFilter {
    fn from(filter: SearchFilter) -> Self {
        Self {
            id: filter.id,
            values: filter.values,
        }
    }
}
