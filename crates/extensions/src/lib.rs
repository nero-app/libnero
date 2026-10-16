mod extension;
mod host;
pub mod types;
mod wit;

use std::sync::Arc;

pub use extension::WasmExtension;
pub use host::{ExtensionOptions, WasmHost};
pub use wasm_metadata::Metadata;

pub use nero_keyvalue_ttl::{Bucket, Error as StoreError, KeyResponse, Store};
pub use nero_portal::{
    CallbackTransport, Cancel, ChoiceItem, Delivery, Interaction, ListenError, Opener,
    ProgressHandle, SelectionRequest, Severity, TextRequest,
};

use anyhow::Result;

use crate::{
    types::{
        EntriesPage, Entry, EntryDetails, EpisodesPage, FilterCategory, SearchFilter, Series,
        SeriesPage, Video, WatchStatus,
    },
    wit::AsyncTryIntoWithStore,
};

pub trait Extension {
    fn metadata(&self) -> Arc<Metadata>;

    fn filters(&self) -> impl std::future::Future<Output = Result<Vec<FilterCategory>>>;

    fn search(
        &self,
        query: &str,
        cursor: Option<&str>,
        filters: Vec<SearchFilter>,
    ) -> impl std::future::Future<Output = Result<SeriesPage>>;

    fn get_series_info(&self, series_id: &str)
    -> impl std::future::Future<Output = Result<Series>>;

    fn get_series_episodes(
        &self,
        series_id: &str,
        cursor: Option<&str>,
    ) -> impl std::future::Future<Output = Result<EpisodesPage>>;

    fn get_series_videos(
        &self,
        series_id: &str,
        episode_id: &str,
    ) -> impl std::future::Future<Output = Result<Vec<Video>>>;
}

pub trait Tracker {
    fn list_entries(
        &self,
        status: WatchStatus,
        cursor: Option<&str>,
    ) -> impl std::future::Future<Output = Result<EntriesPage>>;

    fn get_entry(
        &self,
        series_id: &str,
    ) -> impl std::future::Future<Output = Result<Option<Entry>>>;

    fn save_entry(
        &self,
        series_id: &str,
        status: WatchStatus,
        details: EntryDetails,
    ) -> impl std::future::Future<Output = Result<Entry>>;

    fn remove_entry(&self, series_id: &str) -> impl std::future::Future<Output = Result<()>>;
}
