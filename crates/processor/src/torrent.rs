use std::path::PathBuf;

use crate::TorrentSource;

#[async_trait::async_trait]
pub trait TorrentBackend: Send + Sync {
    async fn list_files(&self, source: &TorrentSource) -> anyhow::Result<Vec<PathBuf>>;
}

#[cfg(feature = "torrent-librqbit")]
pub struct RqbitTorrentBackend;

#[cfg(feature = "torrent-librqbit")]
#[async_trait::async_trait]
impl TorrentBackend for RqbitTorrentBackend {
    async fn list_files(&self, source: &TorrentSource) -> anyhow::Result<Vec<PathBuf>> {
        todo!()
    }
}
