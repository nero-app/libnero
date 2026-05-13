pub mod types;
mod utils;

use nero_processor::Processor;
pub use wasm_metadata::Metadata as ExtensionMetadata;

use std::{path::Path, sync::Arc};

use anyhow::bail;
use nero_extensions::{Extension as ExtensionTrait, WasmExtension, WasmHost};
use wasm_metadata::Payload;

use crate::{
    types::{
        EpisodesPage, ExtensionOptions, FilterCategory, SearchFilter, Series, SeriesPage, Video,
    },
    utils::AyncTryIntoWithProcessor,
};

pub struct ExtensionHost {
    host: WasmHost,
    processor: Arc<Processor>,
}

impl ExtensionHost {
    pub fn new(processor: Processor) -> Self {
        Self {
            host: WasmHost::default(),
            processor: Arc::new(processor),
        }
    }

    pub fn processor(&self) -> &Arc<Processor> {
        &self.processor
    }

    pub async fn load(
        &self,
        file_path: impl AsRef<Path>,
        options: ExtensionOptions,
    ) -> anyhow::Result<Extension> {
        let extension = self
            .host
            .load_extension_async(file_path, options.into())
            .await?;

        Ok(Extension {
            inner: extension,
            processor: Arc::clone(&self.processor),
        })
    }

    pub async fn get_extension_metadata(
        file_path: impl AsRef<Path>,
    ) -> anyhow::Result<ExtensionMetadata> {
        let bytes = tokio::fs::read(file_path).await?;
        let payload = Payload::from_binary(&bytes)?;
        match payload {
            Payload::Component { metadata, .. } => Ok(metadata),
            Payload::Module(_) => bail!("unsupported wasm module"),
        }
    }
}

pub struct Extension {
    inner: WasmExtension,
    processor: Arc<Processor>,
}

impl Extension {
    pub fn metadata(&self) -> Arc<ExtensionMetadata> {
        self.inner.metadata()
    }

    pub async fn get_filters(&self) -> anyhow::Result<Vec<FilterCategory>> {
        let categories = self.inner.filters().await?;
        Ok(categories.into_iter().map(Into::into).collect())
    }

    pub async fn search(
        &self,
        query: &str,
        page: Option<u16>,
        filters: Vec<SearchFilter>,
    ) -> anyhow::Result<SeriesPage> {
        let ext_filters = filters.into_iter().map(Into::into).collect();
        let page = self.inner.search(query, page, ext_filters).await?;
        page.async_try_into_with_processor(&self.processor).await
    }

    pub async fn get_series_info(&self, series_id: &str) -> anyhow::Result<Series> {
        let series = self.inner.get_series_info(series_id).await?;
        series.async_try_into_with_processor(&self.processor).await
    }

    pub async fn get_series_episodes(
        &self,
        series_id: &str,
        page: Option<u16>,
    ) -> anyhow::Result<EpisodesPage> {
        let page = self.inner.get_series_episodes(series_id, page).await?;
        page.async_try_into_with_processor(&self.processor).await
    }

    pub async fn get_series_videos(
        &self,
        series_id: &str,
        episode_id: &str,
    ) -> anyhow::Result<Vec<Video>> {
        let extension_videos = self.inner.get_series_videos(series_id, episode_id).await?;

        let mut videos = Vec::with_capacity(extension_videos.len());
        for video in extension_videos {
            let video = video.async_try_into_with_processor(&self.processor).await?;
            videos.push(video);
        }

        Ok(videos)
    }
}
