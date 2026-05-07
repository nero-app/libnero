use std::{path::PathBuf, sync::Arc};

use anyhow::{Result, anyhow};
use nero_keyvalue_ttl::{KeyValueTTL, KeyValueTTLCtx, KeyValueTTLView};
use semver::Version;
use wasm_metadata::Metadata;
use wasmtime::{Store, component::Component};
use wasmtime_wasi::{ResourceTable, WasiCtx, WasiCtxView, WasiView};
use wasmtime_wasi_http::{WasiHttpCtx, WasiHttpView};

use crate::{
    Extension,
    types::{EpisodesPage, FilterCategory, SearchFilter, Series, SeriesPage, Video},
    wit::{ExtensionPre, since_v0_1_0_draft},
};

pub struct WasmState {
    table: ResourceTable,
    ctx: WasiCtx,
    http_ctx: WasiHttpCtx,
    keyvalue_ctx: Arc<KeyValueTTLCtx>,
}

impl WasmState {
    pub fn new(keyvalue_ctx: Arc<KeyValueTTLCtx>) -> Self {
        Self {
            table: ResourceTable::new(),
            ctx: WasiCtx::builder().build(),
            http_ctx: WasiHttpCtx::new(),
            keyvalue_ctx,
        }
    }
}

impl WasiView for WasmState {
    fn ctx(&mut self) -> wasmtime_wasi::WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.ctx,
            table: &mut self.table,
        }
    }
}

impl WasiHttpView for WasmState {
    fn ctx(&mut self) -> &mut WasiHttpCtx {
        &mut self.http_ctx
    }

    fn table(&mut self) -> &mut ResourceTable {
        &mut self.table
    }
}

impl KeyValueTTLView for WasmState {
    fn keyvalue_ttl(&mut self) -> KeyValueTTL<'_> {
        KeyValueTTL::new(&self.keyvalue_ctx, &mut self.table)
    }
}

pub struct ExtensionOptions {
    pub cache_dir: PathBuf,
    pub max_cache_size: Option<u64>,
}

pub struct WasmExtension {
    extension_pre: ExtensionPre,
    metadata: Arc<Metadata>,
    keyvalue_ctx: Arc<KeyValueTTLCtx>,
}

impl WasmExtension {
    pub(crate) async fn instantiate_async(
        version: Version,
        component: &Component,
        metadata: Metadata,
        options: ExtensionOptions,
    ) -> Result<Self> {
        let extension_pre = match version {
            v if v >= *since_v0_1_0_draft::MIN_VER => {
                let linker = since_v0_1_0_draft::linker(component.engine())?;
                let pre = linker.instantiate_pre(component)?;
                Ok(ExtensionPre::V0_1_0_DRAFT(
                    since_v0_1_0_draft::ExtensionPre::new(pre)?,
                ))
            }
            _ => Err(anyhow!("unsupported extension version")),
        }?;

        let kv_ctx = KeyValueTTLCtx::new(options.cache_dir, options.max_cache_size).await?;

        Ok(Self {
            extension_pre,
            metadata: Arc::new(metadata),
            keyvalue_ctx: Arc::new(kv_ctx),
        })
    }

    pub(crate) fn get_version(wasm_bytes: &[u8]) -> Result<Version> {
        const PACKAGE_NAMESPACE: &str = "nero";
        const PACKAGE_NAME: &str = "extension";

        let decoded = wit_component::decode(wasm_bytes)?;
        let resolve = decoded.resolve();

        for (_, pkg) in resolve.packages.iter() {
            if pkg.name.namespace == PACKAGE_NAMESPACE
                && pkg.name.name == PACKAGE_NAME
                && let Some(version) = &pkg.name.version
            {
                return Ok(version.clone());
            }
        }

        anyhow::bail!(
            "wasm does not contain a '{PACKAGE_NAMESPACE}:{PACKAGE_NAME}' package with a version"
        )
    }
}

impl Extension for WasmExtension {
    fn metadata(&self) -> Arc<Metadata> {
        self.metadata.clone()
    }

    async fn filters(&self) -> Result<Vec<FilterCategory>> {
        let mut store = Store::new(
            self.extension_pre.engine(),
            WasmState::new(self.keyvalue_ctx.clone()),
        );

        let extension = self.extension_pre.instantiate_async(&mut store).await?;
        extension.filters(store).await
    }

    async fn search(
        &self,
        query: &str,
        page: Option<u16>,
        filters: Vec<SearchFilter>,
    ) -> Result<SeriesPage> {
        let mut store = Store::new(
            self.extension_pre.engine(),
            WasmState::new(self.keyvalue_ctx.clone()),
        );

        let extension = self.extension_pre.instantiate_async(&mut store).await?;
        extension.search(store, query, page, filters).await
    }

    async fn get_series_info(&self, series_id: &str) -> Result<Series> {
        let mut store = Store::new(
            self.extension_pre.engine(),
            WasmState::new(self.keyvalue_ctx.clone()),
        );

        let extension = self.extension_pre.instantiate_async(&mut store).await?;
        extension.get_series_info(store, series_id).await
    }

    async fn get_series_episodes(
        &self,
        series_id: &str,
        page: Option<u16>,
    ) -> Result<EpisodesPage> {
        let mut store = Store::new(
            self.extension_pre.engine(),
            WasmState::new(self.keyvalue_ctx.clone()),
        );

        let extension = self.extension_pre.instantiate_async(&mut store).await?;
        extension.get_series_episodes(store, series_id, page).await
    }

    async fn get_series_videos(&self, series_id: &str, episode_id: &str) -> Result<Vec<Video>> {
        let mut store = Store::new(
            self.extension_pre.engine(),
            WasmState::new(self.keyvalue_ctx.clone()),
        );

        let extension = self.extension_pre.instantiate_async(&mut store).await?;
        extension
            .get_series_videos(store, series_id, episode_id)
            .await
    }
}
