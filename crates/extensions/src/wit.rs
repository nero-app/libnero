use anyhow::{Result, anyhow};
use http_body_util::BodyExt;
use wasmtime::{Engine, Store};
use wasmtime_wasi_http::{
    bindings::http::types::{Method, Scheme},
    types::HostOutgoingRequest,
};

use crate::{
    extension::WasmState,
    types::{
        EntriesPage, Entry, EntryDetails, EpisodesPage, FilterCategory, HttpRequest, SearchFilter,
        Series, SeriesPage, Video, WatchStatus,
    },
};

pub(super) mod since_v0_1_0_draft;

pub(super) trait TryFromWithStore<T>: Sized {
    async fn try_from_with_store(value: T, store: &mut Store<WasmState>) -> Result<Self>;
}

pub(super) trait TryIntoWithStore<T> {
    async fn try_into_with_store(self, store: &mut Store<WasmState>) -> Result<T>;
}

impl<T, U> TryIntoWithStore<U> for T
where
    U: TryFromWithStore<T>,
{
    async fn try_into_with_store(self, store: &mut Store<WasmState>) -> Result<U> {
        U::try_from_with_store(self, store).await
    }
}

pub(super) trait IntoHttpRequest {
    async fn into_http_request(self) -> Result<HttpRequest>;
}

impl IntoHttpRequest for HostOutgoingRequest {
    async fn into_http_request(self) -> Result<HttpRequest> {
        let mut builder = http::Uri::builder();
        if let Some(scheme) = &self.scheme {
            builder = builder.scheme(match scheme {
                Scheme::Http => http::uri::Scheme::HTTP.as_str(),
                Scheme::Https => http::uri::Scheme::HTTPS.as_str(),
                Scheme::Other(s) => s.as_str(),
            });
        }
        if let Some(a) = &self.authority {
            builder = builder.authority(a.as_str());
        }
        if let Some(pq) = &self.path_with_query {
            builder = builder.path_and_query(pq.as_str());
        }
        let uri = builder.build()?;

        let http_method = match self.method {
            Method::Get => http::Method::GET,
            Method::Head => http::Method::HEAD,
            Method::Post => http::Method::POST,
            Method::Put => http::Method::PUT,
            Method::Delete => http::Method::DELETE,
            Method::Connect => http::Method::CONNECT,
            Method::Options => http::Method::OPTIONS,
            Method::Trace => http::Method::TRACE,
            Method::Patch => http::Method::PATCH,
            Method::Other(other) => http::Method::from_bytes(other.as_bytes())?,
        };
        let mut builder = http::Request::builder().method(http_method).uri(uri);
        if let Some(headers) = builder.headers_mut() {
            *headers = self.headers.clone();
        }
        let bytes = match self.body {
            Some(body) => {
                let collected = body.collect().await?;
                Some(collected.to_bytes())
            }
            None => None,
        };
        let request = builder.body(bytes)?;

        Ok(request)
    }
}

#[allow(non_camel_case_types)]
pub(super) enum ExtensionPre {
    V0_1_0_DRAFT(since_v0_1_0_draft::ExtensionPre<WasmState>),
}

impl ExtensionPre {
    pub(super) fn engine(&self) -> &Engine {
        match self {
            ExtensionPre::V0_1_0_DRAFT(extension_pre) => extension_pre.engine(),
        }
    }

    pub(super) async fn instantiate(&self, store: &mut Store<WasmState>) -> Result<Extension> {
        match self {
            ExtensionPre::V0_1_0_DRAFT(pre) => {
                let instance = pre.instance_pre().instantiate_async(&mut *store).await?;
                let extension = since_v0_1_0_draft::ExtensionIndices::new(pre.instance_pre())?
                    .load(&mut *store, &instance)?;
                let tracker = since_v0_1_0_draft::TrackerIndices::new(pre.instance_pre())
                    .ok()
                    .map(|indices| indices.load(&mut *store, &instance))
                    .transpose()?;
                Ok(Extension::V0_1_0_DRAFT(extension, tracker))
            }
        }
    }

    pub(super) fn supports_tracker(&self) -> bool {
        match self {
            ExtensionPre::V0_1_0_DRAFT(pre) => {
                since_v0_1_0_draft::TrackerIndices::new(pre.instance_pre()).is_ok()
            }
        }
    }
}

#[allow(non_camel_case_types)]
pub(super) enum Extension {
    V0_1_0_DRAFT(
        since_v0_1_0_draft::Extension,
        Option<since_v0_1_0_draft::Tracker>,
    ),
}

impl Extension {
    pub(super) async fn filters(
        &self,
        mut store: wasmtime::Store<WasmState>,
    ) -> Result<Vec<FilterCategory>> {
        match self {
            Extension::V0_1_0_DRAFT(extension, _) => {
                let res = extension
                    .nero_extension_extractor()
                    .call_filters(&mut store)
                    .await?
                    .map_err(|err| anyhow!("{err}"))?;

                Ok(res.into_iter().map(Into::into).collect())
            }
        }
    }

    pub(super) async fn search(
        &self,
        mut store: wasmtime::Store<WasmState>,
        query: &str,
        cursor: Option<&str>,
        filters: Vec<SearchFilter>,
    ) -> Result<SeriesPage> {
        match self {
            Extension::V0_1_0_DRAFT(extension, _) => {
                let filters = filters.into_iter().map(Into::into).collect::<Vec<_>>();

                let res = extension
                    .nero_extension_extractor()
                    .call_search(&mut store, query, cursor, &filters)
                    .await?
                    .map_err(|err| anyhow!("{err}"))?;

                res.try_into_with_store(&mut store).await
            }
        }
    }

    pub(super) async fn get_series_info(
        &self,
        mut store: wasmtime::Store<WasmState>,
        series_id: &str,
    ) -> Result<Series> {
        match self {
            Extension::V0_1_0_DRAFT(extension, _) => {
                let res = extension
                    .nero_extension_extractor()
                    .call_get_series_info(&mut store, series_id)
                    .await?
                    .map_err(|err| anyhow!("{err}"))?;

                res.try_into_with_store(&mut store).await
            }
        }
    }

    pub(super) async fn get_series_episodes(
        &self,
        mut store: wasmtime::Store<WasmState>,
        series_id: &str,
        cursor: Option<&str>,
    ) -> Result<EpisodesPage> {
        match self {
            Extension::V0_1_0_DRAFT(extension, _) => {
                let res = extension
                    .nero_extension_extractor()
                    .call_get_series_episodes(&mut store, series_id, cursor)
                    .await?
                    .map_err(|err| anyhow!("{err}"))?;

                res.try_into_with_store(&mut store).await
            }
        }
    }

    pub(super) async fn get_series_videos(
        &self,
        mut store: wasmtime::Store<WasmState>,
        series_id: &str,
        episode_id: &str,
    ) -> Result<Vec<Video>> {
        match self {
            Extension::V0_1_0_DRAFT(extension, _) => {
                let res = extension
                    .nero_extension_extractor()
                    .call_get_series_videos(&mut store, series_id, episode_id)
                    .await?
                    .map_err(|err| anyhow!("{err}"))?;

                let mut items = Vec::new();
                for video in res {
                    items.push(video.try_into_with_store(&mut store).await?);
                }
                Ok(items)
            }
        }
    }

    pub(super) async fn list_entries(
        &self,
        mut store: wasmtime::Store<WasmState>,
        status: WatchStatus,
        cursor: Option<&str>,
    ) -> Result<EntriesPage> {
        match self {
            Extension::V0_1_0_DRAFT(_, Some(tracker)) => {
                let result = tracker
                    .call_list_entries(&mut store, status.into(), cursor)
                    .await?
                    .map_err(|error| anyhow!("{error}"))?;
                result.try_into_with_store(&mut store).await
            }
            Extension::V0_1_0_DRAFT(_, None) => anyhow::bail!("extension does not export tracker"),
        }
    }

    pub(super) async fn get_entry(
        &self,
        mut store: wasmtime::Store<WasmState>,
        series_id: &str,
    ) -> Result<Option<Entry>> {
        match self {
            Extension::V0_1_0_DRAFT(_, Some(tracker)) => {
                let result = tracker
                    .call_get_entry(&mut store, series_id)
                    .await?
                    .map_err(|error| anyhow!("{error}"))?;
                match result {
                    Some(entry) => Ok(Some(entry.try_into_with_store(&mut store).await?)),
                    None => Ok(None),
                }
            }
            Extension::V0_1_0_DRAFT(_, None) => anyhow::bail!("extension does not export tracker"),
        }
    }

    pub(super) async fn save_entry(
        &self,
        mut store: wasmtime::Store<WasmState>,
        series_id: &str,
        status: WatchStatus,
        details: EntryDetails,
    ) -> Result<Entry> {
        match self {
            Extension::V0_1_0_DRAFT(_, Some(tracker)) => {
                let result = tracker
                    .call_save_entry(&mut store, series_id, status.into(), &details.into())
                    .await?
                    .map_err(|error| anyhow!("{error}"))?;
                result.try_into_with_store(&mut store).await
            }
            Extension::V0_1_0_DRAFT(_, None) => anyhow::bail!("extension does not export tracker"),
        }
    }

    pub(super) async fn remove_entry(
        &self,
        mut store: wasmtime::Store<WasmState>,
        series_id: &str,
    ) -> Result<()> {
        match self {
            Extension::V0_1_0_DRAFT(_, Some(tracker)) => tracker
                .call_remove_entry(&mut store, series_id)
                .await?
                .map_err(|error| anyhow!("{error}")),
            Extension::V0_1_0_DRAFT(_, None) => anyhow::bail!("extension does not export tracker"),
        }
    }
}
