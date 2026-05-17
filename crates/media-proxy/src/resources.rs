use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::bail;
use http::uri::Scheme;
use tokio::{sync::RwLock, time};
use url::Url;

use crate::HttpRequest;
#[cfg(feature = "torrent")]
use crate::torrent::TorrentSource;

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
    addr: SocketAddr,
    http_client: reqwest::Client,
    entries: Arc<RwLock<HashMap<String, Entry>>>,
    ttl: Option<Duration>,
    capacity: Option<usize>,
}

impl ResourceStore {
    pub(crate) fn new(
        addr: SocketAddr,
        http_client: reqwest::Client,
        config: ResourceStoreConfig,
    ) -> Self {
        let store = Self {
            addr,
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

    async fn insert_http(&self, id: String, req: Box<HttpRequest>) -> anyhow::Result<Url> {
        if req.headers().is_empty() && req.body().is_none() {
            return Ok(Url::parse(&req.uri().to_string())?);
        }

        let mime_type = crate::mime::mime_type(&self.http_client, &req)
            .await?
            .ok_or(anyhow::anyhow!("Could not detect mime type"))?;

        #[cfg(feature = "torrent")]
        if mime_type.type_() == mime::APPLICATION
            && mime_type.subtype() == "application/x-bittorrent"
        {
            let resource = Resource::Torrent(TorrentSource::Http(req));
            let url = Url::parse(&format!("{}://{}/torrent/{}", Scheme::HTTP, self.addr, id))?;
            self.save(id, resource).await?;
            return Ok(url);
        }

        let path = match mime_type.type_() {
            mime::IMAGE => "image",
            mime::VIDEO => "video",
            _ => bail!("Unsupported media type"),
        };

        let url = Url::parse(&format!("{}://{}/{}/{}", Scheme::HTTP, self.addr, path, id))?;
        self.save(id, Resource::Http(req)).await?;

        Ok(url)
    }

    pub async fn insert(&self, id: String, resource: Resource) -> anyhow::Result<Url> {
        match resource {
            Resource::Http(req) => self.insert_http(id, req).await,
            #[cfg(feature = "torrent")]
            Resource::Torrent(src) => {
                let url = Url::parse(&format!("{}://{}/torrent/{}", Scheme::HTTP, self.addr, id))?;
                self.save(id, Resource::Torrent(src)).await?;
                Ok(url)
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
