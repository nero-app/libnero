mod error;
mod mime;
pub mod resources;
mod routes;
#[cfg(feature = "torrent")]
pub mod torrent;
pub mod utils;

use std::{net::SocketAddr, sync::Arc};

use axum::{Router, routing::get};
use bytes::Bytes;
use tokio::sync::RwLock;

#[cfg(feature = "torrent")]
use crate::torrent::Torrent;
use crate::{
    resources::{Resource, ResourceStore, ResourceStoreConfig},
    routes::{handle_image_request, handle_video_request},
};

pub type HttpRequest = http::Request<Option<Bytes>>;

#[derive(Default)]
pub struct MediaProxyConfig {
    pub resource_store: ResourceStoreConfig,
    #[cfg(feature = "torrent")]
    pub torrent_backend: Option<Arc<dyn torrent::TorrentBackend>>,
    #[cfg(feature = "torrent")]
    pub torrent_file_selector: Option<Arc<dyn torrent::TorrentFileSelector>>,
}

pub struct ServerState {
    #[cfg(feature = "torrent")]
    addr: SocketAddr,

    http_client: reqwest::Client,
    #[cfg(feature = "torrent")]
    torrent_backend: Option<Arc<dyn torrent::TorrentBackend>>,
    #[cfg(feature = "torrent")]
    torrent_file_selector: Option<Arc<dyn torrent::TorrentFileSelector>>,

    resource_store: ResourceStore,

    current_video: RwLock<Option<Resource>>,
    #[cfg(feature = "torrent")]
    current_torrent: RwLock<Option<Torrent>>,
}

pub struct MediaProxy {
    state: Arc<ServerState>,
}

impl MediaProxy {
    pub fn new(addr: SocketAddr, http_client: reqwest::Client, config: MediaProxyConfig) -> Self {
        let state = ServerState {
            #[cfg(feature = "torrent")]
            addr,
            http_client: http_client.clone(),

            #[cfg(feature = "torrent")]
            torrent_backend: config.torrent_backend,
            #[cfg(feature = "torrent")]
            torrent_file_selector: config.torrent_file_selector,

            resource_store: ResourceStore::new(addr, http_client, config.resource_store),
            current_video: RwLock::new(None),
            #[cfg(feature = "torrent")]
            current_torrent: RwLock::new(None),
        };

        Self {
            state: Arc::new(state),
        }
    }

    pub fn resource_store(&self) -> &ResourceStore {
        &self.state.resource_store
    }

    pub fn router(&self) -> Router {
        let base = Router::new()
            .route("/image/{resource_id}", get(handle_image_request))
            .route("/video/{resource_id}", get(handle_video_request));

        #[cfg(feature = "torrent")]
        let base = if self.state.torrent_backend.is_some() {
            base.route(
                "/torrent/{resource_id}",
                get(routes::handle_torrent_request),
            )
            .route(
                "/torrent/{torrent_id}/stream/{file_index}",
                get(routes::handle_torrent_stream_request),
            )
        } else {
            base
        };

        base.with_state(self.state.clone())
    }
}
