mod error;
pub mod resources;
mod routes;
#[cfg(feature = "torrent")]
pub mod torrent;
pub mod utils;

use std::collections::HashMap;
use std::sync::Arc;

use axum::{Router, routing::get};
use bytes::Bytes;
use tokio::sync::RwLock;

#[cfg(feature = "torrent")]
use crate::torrent::Torrent;
use crate::{
    resources::{MediaReference, Resource},
    routes::handle_media_request,
};

pub type HttpRequest = http::Request<Option<Bytes>>;

#[cfg(feature = "torrent")]
pub struct TorrentConfig {
    pub torrent_backend: Arc<dyn torrent::TorrentBackend>,
    pub torrent_file_selector: Option<Arc<dyn torrent::TorrentFileSelector>>,
}

pub struct ServerState {
    http_client: reqwest::Client,
    #[cfg(feature = "torrent")]
    torrent_backend: Arc<dyn torrent::TorrentBackend>,
    #[cfg(feature = "torrent")]
    torrent_file_selector: Option<Arc<dyn torrent::TorrentFileSelector>>,

    resources: RwLock<HashMap<String, Resource>>,

    #[cfg(feature = "torrent")]
    active_torrents: RwLock<HashMap<String, Torrent>>,
}

pub struct MediaProxy {
    state: Arc<ServerState>,
}

impl MediaProxy {
    #[cfg(not(feature = "torrent"))]
    pub fn new(http_client: reqwest::Client) -> Self {
        let state = ServerState {
            http_client,
            resources: RwLock::new(HashMap::new()),
        };

        Self {
            state: Arc::new(state),
        }
    }

    #[cfg(feature = "torrent")]
    pub fn new(http_client: reqwest::Client, config: TorrentConfig) -> Self {
        let state = ServerState {
            http_client,
            torrent_backend: config.torrent_backend,
            torrent_file_selector: config.torrent_file_selector,

            resources: RwLock::new(HashMap::new()),
            active_torrents: RwLock::new(HashMap::new()),
        };

        Self {
            state: Arc::new(state),
        }
    }

    pub async fn register(&self, id: impl Into<String>, resource: Resource) -> MediaReference {
        let id = id.into();

        #[cfg(feature = "torrent")]
        {
            self.state.resources.write().await.remove(&id);
            self.release_torrent(&id).await;
        }

        self.state
            .resources
            .write()
            .await
            .insert(id.clone(), resource);

        MediaReference::new(id)
    }

    pub async fn unregister(&self, id: &str) -> Option<Resource> {
        let resource = self.state.resources.write().await.remove(id);

        #[cfg(feature = "torrent")]
        self.release_torrent(id).await;

        resource
    }

    #[cfg(feature = "torrent")]
    async fn release_torrent(&self, resource_id: &str) {
        let torrent = self.state.active_torrents.write().await.remove(resource_id);

        if let Some(torrent) = torrent
            && let Err(error) = self.state.torrent_backend.cancel_torrent(&torrent.id).await
        {
            tracing::warn!(%error, %resource_id, "failed to release torrent resource");
        }
    }

    pub fn router(&self) -> Router {
        let base = Router::new().route("/media/{resource_id}", get(handle_media_request));

        #[cfg(feature = "torrent")]
        let base = base.route(
            "/media/{resource_id}/stream/{file_index}",
            get(routes::handle_torrent_stream_request),
        );

        base.with_state(self.state.clone())
    }
}
