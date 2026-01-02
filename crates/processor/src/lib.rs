mod cache;
mod error;
mod mime_detector;
mod routes;
mod utils;

use std::{io, net::SocketAddr, sync::Arc, time::Duration};

use anyhow::bail;
use axum::{Router, routing::get};
use bytes::Bytes;
use http::uri::Scheme;
use tokio::{net::TcpListener, sync::RwLock};
use tracing::debug;
use url::Url;

use crate::{
    cache::Cache,
    mime_detector::mime_type,
    routes::{handle_image_request, handle_video_request},
    utils::get_request_hash,
};

type HttpRequest = http::Request<Option<Bytes>>;

#[derive(Debug, Clone)]
enum Request {
    Http(Box<HttpRequest>),
    MagnetUri(String),
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
            .with_state(self.state.clone());

        let listener = TcpListener::bind(self.state.addr).await?;
        debug!("listening on {}", listener.local_addr().unwrap());
        axum::serve(listener, app).await
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
            _ => bail!("Unsupported media type"),
        }

        self.state
            .video_requests
            .insert(request_hash, Request::Http(Box::new(request)))
            .await;

        Ok(base)
    }

    pub async fn register_manget_uri(&self, uri: String) -> anyhow::Result<Url> {
        todo!()
    }
}
