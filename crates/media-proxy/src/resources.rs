use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::bail;
use serde::{Deserialize, Serialize};
use tokio::{sync::RwLock, time};
use url::Url;

use crate::HttpRequest;
#[cfg(feature = "torrent")]
use crate::torrent::TorrentSource;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ResourceKind {
    Image,
    Video,
    Torrent,
}

impl ResourceKind {
    fn route(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Video => "video",
            Self::Torrent => "torrent",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MediaReference {
    Direct { url: Url },
    Proxied { kind: ResourceKind, id: String },
}

impl MediaReference {
    pub fn resolve(&self, proxy_base: &Url) -> Result<Url, url::ParseError> {
        match self {
            Self::Direct { url } => Ok(url.clone()),
            Self::Proxied { kind, id } => proxy_base.join(&format!("{}/{id}", kind.route())),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Resource {
    Http(Box<HttpRequest>),
    #[cfg(feature = "torrent")]
    Torrent(TorrentSource),
}

#[derive(Debug, Clone)]
struct Entry {
    resource: Resource,
    expires_at: Option<Instant>,
}

impl Entry {
    fn new(resource: Resource, ttl: Option<Duration>) -> Self {
        Self {
            resource,
            expires_at: ttl.map(|d| Instant::now() + d),
        }
    }

    fn is_expired(&self) -> bool {
        self.expires_at
            .map(|e| Instant::now() >= e)
            .unwrap_or(false)
    }
}

#[derive(Default)]
pub struct ResourceStoreConfig {
    pub ttl: Option<Duration>,
    pub capacity: Option<usize>,
}

pub struct ResourceStore {
    http_client: reqwest::Client,
    entries: Arc<RwLock<HashMap<String, Entry>>>,
    ttl: Option<Duration>,
    capacity: Option<usize>,
}

impl ResourceStore {
    pub(crate) fn new(http_client: reqwest::Client, config: ResourceStoreConfig) -> Self {
        let store = Self {
            http_client,
            entries: Arc::new(RwLock::new(HashMap::new())),
            ttl: config.ttl,
            capacity: config.capacity,
        };

        if store.ttl.is_some() {
            store.spawn_cleanup_task();
        }

        store
    }

    fn spawn_cleanup_task(&self) {
        let entries = Arc::clone(&self.entries);
        tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_secs(60));
            loop {
                interval.tick().await;
                entries.write().await.retain(|_, e| !e.is_expired());
            }
        });
    }

    async fn save(&self, id: String, resource: Resource) -> anyhow::Result<()> {
        let mut entries = self.entries.write().await;
        if let Some(max) = self.capacity
            && entries.len() >= max
            && !entries.contains_key(&id)
        {
            anyhow::bail!("resource store is at capacity");
        }
        entries.insert(id, Entry::new(resource, self.ttl));
        Ok(())
    }

    async fn insert_http(
        &self,
        id: String,
        req: Box<HttpRequest>,
    ) -> anyhow::Result<MediaReference> {
        if req.headers().is_empty() && req.body().is_none() {
            return Ok(MediaReference::Direct {
                url: req.uri().to_string().parse()?,
            });
        }

        let mime_type = crate::mime::mime_type(&self.http_client, &req)
            .await?
            .ok_or(anyhow::anyhow!("Could not detect mime type"))?;

        #[cfg(feature = "torrent")]
        if mime_type.type_() == mime::APPLICATION
            && mime_type.subtype() == "application/x-bittorrent"
        {
            let resource = Resource::Torrent(TorrentSource::Http(req));
            self.save(id.clone(), resource).await?;
            return Ok(MediaReference::Proxied {
                kind: ResourceKind::Torrent,
                id,
            });
        }

        let kind = match mime_type.type_() {
            mime::IMAGE => ResourceKind::Image,
            mime::VIDEO => ResourceKind::Video,
            _ => bail!("Unsupported media type"),
        };

        self.save(id.clone(), Resource::Http(req)).await?;

        Ok(MediaReference::Proxied { kind, id })
    }

    pub async fn insert(&self, id: String, resource: Resource) -> anyhow::Result<MediaReference> {
        match resource {
            Resource::Http(req) => self.insert_http(id, req).await,
            #[cfg(feature = "torrent")]
            Resource::Torrent(src) => {
                self.save(id.clone(), Resource::Torrent(src)).await?;
                Ok(MediaReference::Proxied {
                    kind: ResourceKind::Torrent,
                    id,
                })
            }
        }
    }

    pub async fn get(&self, id: &str) -> Option<Resource> {
        let entries = self.entries.read().await;
        let entry = entries.get(id)?;
        if entry.is_expired() {
            return None;
        }
        Some(entry.resource.clone())
    }

    pub async fn remove(&self, id: &str) -> Option<Resource> {
        let mut entries = self.entries.write().await;
        let entry = entries.remove(id)?;
        if entry.is_expired() {
            return None;
        }
        Some(entry.resource)
    }
}
