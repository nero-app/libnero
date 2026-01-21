use anyhow::{Result, anyhow};
use http_body_util::BodyExt;
use wasmtime::{Engine, Store};
use wasmtime_wasi_http::{
    bindings::http::types::{Method, Scheme},
    types::HostOutgoingRequest,
};

use crate::{
    extension::WasmState,
    types::{EpisodesPage, FilterCategory, HttpRequest, SearchFilter, Series, SeriesPage, Video},
};

pub mod since_v0_1_0_draft;

pub(super) trait AsyncTryFromWithStore<T>: Sized {
    async fn try_from_with_store(value: T, store: &mut Store<WasmState>) -> Result<Self>;
}

pub(super) trait AsyncTryIntoWithStore<T> {
    async fn try_into_with_store(self, store: &mut Store<WasmState>) -> Result<T>;
}

impl<T, U> AsyncTryIntoWithStore<U> for T
where
    U: AsyncTryFromWithStore<T>,
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
pub enum ExtensionPre {
    V0_1_0_DRAFT(since_v0_1_0_draft::ExtensionPre<WasmState>),
}

impl ExtensionPre {
    pub fn engine(&self) -> &Engine {
        match self {
            ExtensionPre::V0_1_0_DRAFT(extension_pre) => extension_pre.engine(),
        }
    }

    pub async fn instantiate_async(&self, store: &mut Store<WasmState>) -> Result<Extension> {
        match self {
            ExtensionPre::V0_1_0_DRAFT(pre) => {
                let extension = pre.instantiate_async(store).await?;
                Ok(Extension::V0_1_0_DRAFT(extension))
            }
        }
    }
}

#[allow(non_camel_case_types)]
pub enum Extension {
    V0_1_0_DRAFT(since_v0_1_0_draft::Extension),
}

impl Extension {
    pub async fn filters(
        &self,
        mut store: wasmtime::Store<WasmState>,
    ) -> Result<Vec<FilterCategory>> {
        match self {
            Extension::V0_1_0_DRAFT(extension) => {
                let res = extension
                    .nero_extension_extractor()
                    .call_filters(&mut store)
                    .await?
                    .map_err(|err| anyhow!("{err}"))?;

                Ok(res.into_iter().map(Into::into).collect())
            }
        }
    }

    pub async fn search(
        &self,
        mut store: wasmtime::Store<WasmState>,
        query: &str,
        page: Option<u16>,
        filters: Vec<SearchFilter>,
    ) -> Result<SeriesPage> {
        match self {
            Extension::V0_1_0_DRAFT(extension) => {
                let filters = filters.into_iter().map(Into::into).collect::<Vec<_>>();

                let res = extension
                    .nero_extension_extractor()
                    .call_search(&mut store, query, page, &filters)
                    .await?
                    .map_err(|err| anyhow!("{err}"))?;

                res.try_into_with_store(&mut store).await
            }
        }
    }

    pub async fn get_series_info(
        &self,
        mut store: wasmtime::Store<WasmState>,
        series_id: &str,
    ) -> Result<Series> {
        match self {
            Extension::V0_1_0_DRAFT(extension) => {
                let res = extension
                    .nero_extension_extractor()
                    .call_get_series_info(&mut store, series_id)
                    .await?
                    .map_err(|err| anyhow!("{err}"))?;

                res.try_into_with_store(&mut store).await
            }
        }
    }

    pub async fn get_series_episodes(
        &self,
        mut store: wasmtime::Store<WasmState>,
        series_id: &str,
        page: Option<u16>,
    ) -> Result<EpisodesPage> {
        match self {
            Extension::V0_1_0_DRAFT(extension) => {
                let res = extension
                    .nero_extension_extractor()
                    .call_get_series_episodes(&mut store, series_id, page)
                    .await?
                    .map_err(|err| anyhow!("{err}"))?;

                res.try_into_with_store(&mut store).await
            }
        }
    }

    pub async fn get_series_videos(
        &self,
        mut store: wasmtime::Store<WasmState>,
        series_id: &str,
        episode_id: &str,
    ) -> Result<Vec<Video>> {
        match self {
            Extension::V0_1_0_DRAFT(extension) => {
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
}
