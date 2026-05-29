use std::path::PathBuf;

use anyhow::bail;
use nero_extensions::types::MediaResource;
use nero_media_proxy::{MediaProxy, resources::Resource};
use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

use crate::utils::AsyncTryFromWithProxy;

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
    pub items: Vec<T>,
    pub has_next_page: bool,
}

impl<T, U> AsyncTryFromWithProxy<nero_extensions::types::Page<T>> for Page<U>
where
    U: AsyncTryFromWithProxy<T>,
{
    async fn async_try_from_with_proxy(
        page: nero_extensions::types::Page<T>,
        proxy: &MediaProxy,
    ) -> anyhow::Result<Self> {
        let mut items = Vec::with_capacity(page.items.len());
        for item in page.items {
            items.push(U::async_try_from_with_proxy(item, proxy).await?);
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
    pub id: String,
    pub title: String,
    pub poster_url: Option<Url>,
    pub synopsis: Option<String>,
    pub r#type: Option<String>,
}

impl AsyncTryFromWithProxy<nero_extensions::types::Series> for Series {
    async fn async_try_from_with_proxy(
        series: nero_extensions::types::Series,
        proxy: &MediaProxy,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            id: series.id,
            title: series.title,
            poster_url: match series.poster_resource {
                Some(MediaResource::HttpRequest(req)) => {
                    let id = Uuid::new_v4().to_string();
                    let resource = Resource::Http(req);
                    Some(proxy.resource_store().insert(id, resource).await?)
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
    pub id: String,
    pub number: u16,
    pub title: Option<String>,
    pub thumbnail_url: Option<Url>,
    pub description: Option<String>,
}

impl AsyncTryFromWithProxy<nero_extensions::types::Episode> for Episode {
    async fn async_try_from_with_proxy(
        episode: nero_extensions::types::Episode,
        proxy: &MediaProxy,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            id: episode.id,
            number: episode.number,
            title: episode.title,
            thumbnail_url: match episode.thumbnail_resource {
                Some(MediaResource::HttpRequest(req)) => {
                    let id = Uuid::new_v4().to_string();
                    let resource = Resource::Http(req);
                    Some(proxy.resource_store().insert(id, resource).await?)
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
    pub url: Url,
    pub server: String,
    pub resolution: Resolution,
}

impl AsyncTryFromWithProxy<nero_extensions::types::Video> for Video {
    async fn async_try_from_with_proxy(
        video: nero_extensions::types::Video,
        proxy: &MediaProxy,
    ) -> anyhow::Result<Self> {
        let url = match video.media_resource {
            nero_extensions::types::MediaResource::HttpRequest(request) => {
                let id = Uuid::new_v4().to_string();
                let resource = Resource::Http(request);
                proxy.resource_store().insert(id, resource).await
            }
            #[cfg(not(feature = "torrent"))]
            nero_extensions::types::MediaResource::MagnetUri(_) => {
                bail!("Magnet URIs are not supported for videos (Torrent feature disabled)");
            }
            #[cfg(feature = "torrent")]
            nero_extensions::types::MediaResource::MagnetUri(uri) => {
                use nero_media_proxy::torrent::TorrentSource;

                let id = Uuid::new_v4().to_string();
                let resource = Resource::Torrent(TorrentSource::MagnetUri(uri));
                proxy.resource_store().insert(id, resource).await
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
    pub id: String,
    pub display_name: String,
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
    pub id: String,
    pub display_name: String,
    pub filters: Vec<Filter>,
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
    pub id: String,
    pub values: Vec<String>,
}

impl From<SearchFilter> for nero_extensions::types::SearchFilter {
    fn from(filter: SearchFilter) -> Self {
        Self {
            id: filter.id,
            values: filter.values,
        }
    }
}
