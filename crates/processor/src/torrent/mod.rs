#[cfg(feature = "torrent-librqbit")]
pub mod librqbit;

use std::path::PathBuf;

use anyhow::Result;
use http::{Request, Response};

use crate::TorrentSource;

#[derive(Clone, Debug)]
pub struct AddTorrentOptions {
    pub file_indices: Vec<usize>,
}

#[derive(Debug, Clone)]
pub struct Torrent {
    pub id: String,
    pub name: Option<String>,
    pub files: Vec<TorrentFile>,
}

#[derive(Debug, Clone)]
pub struct TorrentFile {
    pub index: usize,
    pub name: String,
    pub path: PathBuf,
}

#[async_trait::async_trait]
pub trait TorrentBackend: Send + Sync {
    async fn list_files(&self, source: &TorrentSource) -> Result<Vec<TorrentFile>>;

    async fn add_torrent(
        &self,
        source: TorrentSource,
        options: Option<AddTorrentOptions>,
    ) -> Result<Torrent>;

    async fn handle_stream_request(
        &self,
        torrent_id: &str,
        file_index: usize,
        request: Request<axum::body::Body>,
    ) -> Result<Response<axum::body::Body>>;

    async fn cancel_torrent(&self, torrent: &str) -> Result<()>;
}
