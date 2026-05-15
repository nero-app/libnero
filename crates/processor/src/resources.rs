use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};

use http::uri::Scheme;
use tokio::{sync::RwLock, time};
use url::Url;

use crate::HttpRequest;
#[cfg(feature = "torrent")]
use crate::torrent::TorrentSource;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceKind {
    Image,
    Video,
    #[cfg(feature = "torrent")]
    Torrent,
}

#[derive(Debug, Clone)]
pub enum ResourceData {
    Http(Box<HttpRequest>),
    #[cfg(feature = "torrent")]
    Torrent {
        source: TorrentSource,
        file_indices: Vec<usize>,
    },
}

#[derive(Debug, Clone)]
pub struct Resource {
    pub kind: ResourceKind,
    pub data: ResourceData,
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
    addr: SocketAddr,
    #[cfg(feature = "torrent")]
    http_client: reqwest::Client,
    entries: Arc<RwLock<HashMap<String, Entry>>>,
    ttl: Option<Duration>,
    capacity: Option<usize>,
}

impl ResourceStore {
    pub(crate) fn new(
        addr: SocketAddr,
        #[cfg(feature = "torrent")] http_client: reqwest::Client,
        config: ResourceStoreConfig,
    ) -> Self {
        let store = Self {
            addr,
            #[cfg(feature = "torrent")]
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

    pub async fn insert(&self, id: String, resource: Resource) -> anyhow::Result<Url> {
        #[cfg(feature = "torrent")]
        let resource = if let ResourceData::Http(req) = &resource.data
            && let Some(mime) = crate::mime::mime_type(&self.http_client, req).await?
            && mime.type_() == mime::APPLICATION
            && mime.subtype() == "application/x-bittorrent"
        {
            Resource {
                kind: ResourceKind::Torrent,
                data: ResourceData::Torrent {
                    source: TorrentSource::Http(req.clone()),
                    file_indices: vec![],
                },
            }
        } else {
            resource
        };

        match &resource.data {
            ResourceData::Http(req) if req.headers().is_empty() && req.body().is_none() => {
                return Ok(Url::parse(&req.uri().to_string())?);
            }
            _ => {}
        }

        let path = match resource.kind {
            ResourceKind::Image => "image",
            ResourceKind::Video => "video",
            #[cfg(feature = "torrent")]
            ResourceKind::Torrent => "torrent",
        };

        let url = Url::parse(&format!("{}://{}/{}/{}", Scheme::HTTP, self.addr, path, id))?;

        let mut entries = self.entries.write().await;
        if let Some(max) = self.capacity
            && entries.len() >= max
            && !entries.contains_key(&id)
        {
            anyhow::bail!("resource store is at capacity");
        }

        entries.insert(id, Entry::new(resource, self.ttl));
        Ok(url)
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
