use std::path::PathBuf;

use anyhow::bail;
use nero_extensions::types::MediaResource;
use nero_processor::Processor;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::utils::AsyncTryFromWithProcessor;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionOptions {
    pub cache_dir: PathBuf,
    pub max_cache_size: Option<u64>,
}

impl From<ExtensionOptions> for nero_extensions::ExtensionOptions {
    fn from(options: ExtensionOptions) -> Self {
        Self {
            cache_dir: options.cache_dir,
            max_cache_size: options.max_cache_size,
        }
    }
}

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

impl AsyncTryFromWithProcessor<nero_extensions::types::Video> for Video {
    async fn async_try_from_with_processor(
        video: nero_extensions::types::Video,
        processor: &Processor,
    ) -> anyhow::Result<Self> {
        let url = match video.media_resource {
            nero_extensions::types::MediaResource::HttpRequest(request) => {
                match processor.register_video_request(*request.clone()).await {
                    Ok(url) => Ok(url),
                    Err(e) if e.to_string().contains("torrent") => {
                        #[cfg(not(feature = "torrent"))]
                        bail!(
                            "Torrent files are not supported for videos (Torrent feature disabled)"
                        );

                        #[cfg(feature = "torrent")]
                        processor
                            .register_torrent(nero_processor::TorrentSource::Http(request), vec![])
                            .await
                    }
                    Err(e) => Err(e),
                }
            }
            #[cfg(not(feature = "torrent"))]
            nero_extensions::types::MediaResource::MagnetUri(_) => {
                bail!("Magnet URIs are not supported for videos (Torrent feature disabled)");
            }
            #[cfg(feature = "torrent")]
            nero_extensions::types::MediaResource::MagnetUri(uri) => {
                processor
                    .register_torrent(nero_processor::TorrentSource::MagnetUri(uri), vec![])
                    .await
            }
        }?;

        Ok(Self {
            url,
            server: video.server,
            resolution: video.resolution,
        })
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
