mod cache;
mod error;
mod mime_detector;
mod routes;
mod utils;

use std::{
    hash::{DefaultHasher, Hash, Hasher},
    io,
    net::SocketAddr,
    sync::Arc,
    time::Duration,
};

use anyhow::{anyhow, bail};
use axum::{Router, routing::get};
use bytes::Bytes;
use http::uri::Scheme;
use librqbit::{AddTorrent, AddTorrentOptions, Session};
use tokio::{net::TcpListener, sync::RwLock};
use tracing::debug;
use url::Url;

use crate::{
    cache::Cache,
    mime_detector::mime_type,
    routes::{handle_image_request, handle_torrent_request, handle_video_request},
    utils::get_request_hash,
};

type HttpRequest = http::Request<Option<Bytes>>;

#[derive(Debug, Clone)]
pub enum TorrentSource {
    Http(Box<HttpRequest>),
    MagnetUri(String),
}

#[derive(Debug, Clone)]
enum Request {
    Http(Box<HttpRequest>),
    Torrent {
        source: TorrentSource,
        files: Vec<String>,
    },
}

#[derive(Debug, Clone, Default)]
pub struct CacheConfig {
    pub image_ttl: Option<Duration>,
    pub image_capacity: Option<usize>,
    pub video_ttl: Option<Duration>,
    pub video_capacity: Option<usize>,
}

struct ServerState {
    addr: SocketAddr,

    http_client: reqwest::Client,
    torrent_api: RwLock<Option<librqbit::Api>>,

    image_requests: Cache<u64, HttpRequest>,
    video_requests: Cache<u64, Request>,

    current_video: RwLock<Option<Request>>,
}

pub struct Processor {
    state: Arc<ServerState>,
}

impl Processor {
    pub fn new(addr: SocketAddr) -> Self {
        Self::with_cache_config(addr, CacheConfig::default())
    }

    pub fn with_cache_config(addr: SocketAddr, cache_config: CacheConfig) -> Self {
        let state = ServerState {
            addr,
            http_client: reqwest::Client::new(),
            torrent_api: RwLock::new(None),
            image_requests: {
                let mut cache = Cache::default();
                if let Some(ttl) = cache_config.image_ttl {
                    cache = cache.with_ttl(ttl);
                }
                if let Some(capacity) = cache_config.image_capacity {
                    cache = cache.with_capacity(capacity);
                }
                cache
            },
            video_requests: {
                let mut cache = Cache::default();
                if let Some(ttl) = cache_config.video_ttl {
                    cache = cache.with_ttl(ttl);
                }
                if let Some(capacity) = cache_config.video_capacity {
                    cache = cache.with_capacity(capacity);
                }
                cache
            },
            current_video: RwLock::new(None),
        };

        Self {
            state: Arc::new(state),
        }
    }

    pub async fn run(&self) -> io::Result<()> {
        let app = Router::new()
            .route("/image/{request_hash}", get(handle_image_request))
            .route("/video/{request_hash}", get(handle_video_request))
            .route("/torrent/{request_hash}", get(handle_torrent_request))
            .with_state(self.state.clone());

        let listener = TcpListener::bind(self.state.addr).await?;
        debug!("listening on {}", listener.local_addr().unwrap());
        axum::serve(listener, app).await
    }

    pub async fn enable_torrent_support(&self, session: Arc<Session>) {
        let api = librqbit::Api::new(session, None);
        *self.state.torrent_api.write().await = Some(api);
    }

    pub async fn disable_torrent_support(&self) {
        *self.state.torrent_api.write().await = None;
    }

    pub async fn register_image_request(&self, request: HttpRequest) -> anyhow::Result<Url> {
        if request.headers().is_empty() {
            return Ok(Url::parse(&request.uri().to_string())?);
        }

        let mime_type = mime_type(&self.state.http_client, &request)
            .await?
            .ok_or(anyhow::anyhow!("Could not detect mime type"))?;

        if mime_type.subtype() == "application/x-bittorrent" {
            bail!("Torrents are not supported for images");
        }

        let request_hash = get_request_hash(&request);
        let url = Url::parse(&format!(
            "{}://{}/image/{request_hash}",
            Scheme::HTTP,
            self.state.addr,
        ))?;

        self.state
            .image_requests
            .insert(request_hash, request)
            .await;

        Ok(url)
    }

    pub async fn register_video_request(&self, request: HttpRequest) -> anyhow::Result<Url> {
        if request.headers().is_empty() {
            return Ok(Url::parse(&request.uri().to_string())?);
        }

        let mime_type = mime_type(&self.state.http_client, &request)
            .await?
            .ok_or(anyhow::anyhow!("Could not detect mime type"))?;

        let request_hash = get_request_hash(&request);
        let mut base = Url::parse(&format!("{}://{}", Scheme::HTTP, self.state.addr))?;

        match mime_type.type_() {
            mime::VIDEO => base.set_path(&format!("/video/{request_hash}")),
            mime::APPLICATION if mime_type.subtype() == "application/x-bittorrent" => {
                bail!("Torrent file. Use register_torrent() instead.")
            }
            _ => bail!("Unsupported media type"),
        }

        self.state
            .video_requests
            .insert(request_hash, Request::Http(Box::new(request)))
            .await;

        Ok(base)
    }

    pub async fn get_torrent_files(&self, source: TorrentSource) -> anyhow::Result<Vec<String>> {
        let uri = match source {
            TorrentSource::Http(request) => request.uri().to_string(),
            TorrentSource::MagnetUri(uri) => uri,
        };

        let api_guard = self.state.torrent_api.read().await;
        let torrent_api = api_guard
            .as_ref()
            .ok_or(anyhow!("torrent support is not enabled."))?;

        let options = AddTorrentOptions {
            overwrite: true,
            list_only: true,
            ..Default::default()
        };
        let response = torrent_api
            .api_add_torrent(AddTorrent::from_url(uri), Some(options))
            .await?;

        let files = response
            .details
            .files
            .ok_or(anyhow!("no files found"))?
            .into_iter()
            .map(|f| f.name)
            .collect::<Vec<_>>();

        Ok(files)
    }

    pub async fn register_torrent(
        &self,
        source: TorrentSource,
        files: Vec<String>,
    ) -> anyhow::Result<Url> {
        let request_hash = match &source {
            TorrentSource::Http(request) => get_request_hash(request),
            TorrentSource::MagnetUri(uri) => {
                let mut hasher = DefaultHasher::new();
                uri.hash(&mut hasher);
                hasher.finish()
            }
        };

        let url = Url::parse(&format!(
            "{}://{}/torrent/{request_hash}",
            Scheme::HTTP,
            self.state.addr,
        ))?;

        self.state
            .video_requests
            .insert(request_hash, Request::Torrent { source, files })
            .await;

        Ok(url)
    }
}
