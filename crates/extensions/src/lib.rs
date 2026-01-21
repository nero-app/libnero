mod extension;
mod host;
pub mod types;
mod wit;

pub use extension::WasmExtension;
pub use host::WasmHost;

use anyhow::Result;
use wasm_metadata::Metadata;

use crate::{
    types::{EpisodesPage, FilterCategory, SearchFilter, Series, SeriesPage, Video},
    wit::AsyncTryIntoWithStore,
};

pub trait Extension {
    fn metadata(&self) -> &Metadata;

    fn filters(&self) -> impl std::future::Future<Output = Result<Vec<FilterCategory>>>;

    fn search(
        &self,
        query: &str,
        page: Option<u16>,
        filters: Vec<SearchFilter>,
    ) -> impl std::future::Future<Output = Result<SeriesPage>>;

    fn get_series_info(&self, series_id: &str)
    -> impl std::future::Future<Output = Result<Series>>;

    fn get_series_episodes(
        &self,
        series_id: &str,
        page: Option<u16>,
    ) -> impl std::future::Future<Output = Result<EpisodesPage>>;

    fn get_series_videos(
        &self,
        series_id: &str,
        episode_id: &str,
    ) -> impl std::future::Future<Output = Result<Vec<Video>>>;
}
