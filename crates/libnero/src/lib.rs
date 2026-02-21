#[cfg(feature = "torrent")]
mod file_resolver;
pub mod types;
mod utils;

pub use nero_processor::*;
pub use wasm_metadata::Metadata as ExtensionMetadata;

use std::sync::Arc;

use anyhow::bail;
use nero_extensions::{Extension, WasmExtension, WasmHost};
use tokio::sync::RwLock;
use wasm_metadata::{Metadata, Payload};

#[cfg(feature = "torrent")]
use crate::types::TorrentContext;
use crate::{
    types::{EpisodesPage, FilterCategory, SearchFilter, Series, SeriesPage, Video},
    utils::AyncTryIntoWithProcessor,
};

pub struct Nero {
    host: WasmHost,
    extension: RwLock<Option<WasmExtension>>,
    processor: Arc<Processor>,
}

impl Nero {
    pub fn new(processor: Processor) -> Self {
        Self {
            host: WasmHost::default(),
            extension: RwLock::new(None),
            processor: Arc::new(processor),
        }
    }

    pub fn processor(&self) -> &Arc<Processor> {
        &self.processor
    }

    pub async fn get_extension_metadata(
        file_path: impl AsRef<std::path::Path>,
    ) -> anyhow::Result<Metadata> {
        let bytes = tokio::fs::read(file_path).await?;
        let payload = Payload::from_binary(&bytes)?;
        match payload {
            Payload::Component { metadata, .. } => Ok(metadata),
            Payload::Module(_) => bail!("unsupported wasm module"),
        }
    }

    pub async fn load_extension(
        &self,
        file_path: impl AsRef<std::path::Path>,
    ) -> anyhow::Result<()> {
        let extension = self.host.load_extension_async(file_path).await?;
        self.extension.write().await.replace(extension);

        Ok(())
    }

    // TODO: options
    #[cfg(feature = "torrent")]
    pub async fn enable_torrent_support(
        &self,
        output_folder: std::path::PathBuf,
        client: reqwest::Client,
    ) -> anyhow::Result<()> {
        use librqbit::Session;
        use nero_processor::torrent::RqbitTorrentBackend;

        let session = Session::new(output_folder).await?;
        let backend = RqbitTorrentBackend::new(session, client);
        self.processor.set_torrent_backend(backend).await;

        Ok(())
    }

    #[cfg(feature = "torrent")]
    pub async fn disable_torrent_support(&self) -> anyhow::Result<()> {
        self.processor.remove_torrent_backend().await;

        Ok(())
    }

    pub async fn get_filters(&self) -> anyhow::Result<Vec<FilterCategory>> {
        let guard = self.extension.read().await;
        let extension = guard
            .as_ref()
            .ok_or(anyhow::anyhow!("extension not loaded"))?;

        let categories = extension.filters().await?;
        Ok(categories.into_iter().map(Into::into).collect())
    }

    pub async fn search(
        &self,
        query: &str,
        page: Option<u16>,
        filters: Vec<SearchFilter>,
    ) -> anyhow::Result<SeriesPage> {
        let guard = self.extension.read().await;
        let extension = guard
            .as_ref()
            .ok_or(anyhow::anyhow!("extension not loaded"))?;

        let ext_filters = filters.into_iter().map(Into::into).collect();
        let page = extension.search(query, page, ext_filters).await?;

        page.async_try_into_with_processor(&self.processor).await
    }

    pub async fn get_series_info(&self, series_id: &str) -> anyhow::Result<Series> {
        let guard = self.extension.read().await;
        let extension = guard
            .as_ref()
            .ok_or(anyhow::anyhow!("extension not loaded"))?;

        let series = extension.get_series_info(series_id).await?;

        series.async_try_into_with_processor(&self.processor).await
    }

    pub async fn get_series_episodes(
        &self,
        series_id: &str,
        page: Option<u16>,
    ) -> anyhow::Result<EpisodesPage> {
        let guard = self.extension.read().await;
        let extension = guard
            .as_ref()
            .ok_or(anyhow::anyhow!("extension not loaded"))?;

        let page = extension.get_series_episodes(series_id, page).await?;

        page.async_try_into_with_processor(&self.processor).await
    }

    pub async fn get_series_videos(
        &self,
        series_id: &str,
        episode_id: &str,
        #[cfg(feature = "torrent")] episode_number: u32,
    ) -> anyhow::Result<Vec<Video>> {
        let guard = self.extension.read().await;
        let extension = guard
            .as_ref()
            .ok_or(anyhow::anyhow!("extension not loaded"))?;

        let extension_videos = extension.get_series_videos(series_id, episode_id).await?;

        #[cfg(feature = "torrent")]
        let torrent_ctx = TorrentContext {
            extension,
            series_id,
            episode_number,
        };

        let mut videos = Vec::with_capacity(extension_videos.len());
        for video in extension_videos {
            let video = Video::from_extension_video(
                video,
                &self.processor,
                #[cfg(feature = "torrent")]
                &torrent_ctx,
            )
            .await?;

            videos.push(video);
        }

        Ok(videos)
    }
}
