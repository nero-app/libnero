use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use nero_keyvalue_ttl::{KeyValueTTL, KeyValueTTLCtx, KeyValueTTLView};
use nero_portal::{Portal, PortalCtx, PortalView};
use semver::Version;
use wasm_metadata::Metadata;
use wasmtime::{Store, component::Component};
use wasmtime_wasi::{ResourceTable, WasiCtx, WasiCtxView, WasiView};
use wasmtime_wasi_http::{WasiHttpCtx, WasiHttpView};

use crate::{
    Extractor, Tracker,
    types::{
        EntriesPage, Entry, EntryDetails, EpisodesPage, FilterCategory, SearchFilter, Series,
        SeriesPage, Video, WatchStatus,
    },
    wit::{ExtensionPre, since_v0_1_0_draft},
};

#[derive(Default)]
pub(crate) struct ExtensionCtx {
    pub(crate) keyvalue: Option<KeyValueTTLCtx>,
    pub(crate) portal: Option<PortalCtx>,
}

pub struct WasmState {
    table: ResourceTable,
    ctx: WasiCtx,
    http_ctx: WasiHttpCtx,
    extension_ctx: Arc<ExtensionCtx>,
}

impl WasmState {
    fn new(extension_ctx: Arc<ExtensionCtx>) -> Self {
        Self {
            table: ResourceTable::new(),
            ctx: WasiCtx::builder().build(),
            http_ctx: WasiHttpCtx::new(),
            extension_ctx,
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
        let ctx = self
            .extension_ctx
            .keyvalue
            .as_ref()
            .expect("keyvalue imports are linked only when a store is configured");
        KeyValueTTL::new(ctx, &mut self.table)
    }
}

impl PortalView for WasmState {
    fn portal(&mut self) -> Portal<'_> {
        let ctx = self
            .extension_ctx
            .portal
            .as_ref()
            .expect("portal imports are linked only when portal services are configured");
        Portal::new(ctx, &mut self.table)
    }
}

pub struct WasmExtension {
    extension_pre: ExtensionPre,
    metadata: Arc<Metadata>,
    extension_ctx: Arc<ExtensionCtx>,
    supports_tracker: bool,
}

impl WasmExtension {
    pub(crate) fn instantiate(
        version: Version,
        component: &Component,
        metadata: Metadata,
        extension_ctx: ExtensionCtx,
    ) -> Result<Self> {
        let extension_pre = match version {
            v if v == *since_v0_1_0_draft::VERSION => {
                let linker = since_v0_1_0_draft::linker(component.engine(), &extension_ctx)?;
                let pre = linker.instantiate_pre(component).context(
                    "extension imports are incompatible with the configured host capabilities",
                )?;
                Ok(ExtensionPre::V0_1_0_DRAFT(
                    since_v0_1_0_draft::ExtensionPre::new(pre)?,
                ))
            }
            _ => Err(anyhow!("unsupported extension version")),
        }?;

        let supports_tracker = extension_pre.supports_tracker();

        Ok(Self {
            extension_pre,
            metadata: Arc::new(metadata),
            extension_ctx: Arc::new(extension_ctx),
            supports_tracker,
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

    pub fn metadata(&self) -> Arc<Metadata> {
        self.metadata.clone()
    }

    pub fn supports_tracker(&self) -> bool {
        self.supports_tracker
    }
}

impl Extractor for WasmExtension {
    async fn filters(&self) -> Result<Vec<FilterCategory>> {
        let mut store = Store::new(
            self.extension_pre.engine(),
            WasmState::new(self.extension_ctx.clone()),
        );

        let extension = self.extension_pre.instantiate(&mut store).await?;
        extension.filters(store).await
    }

    async fn search(
        &self,
        query: &str,
        cursor: Option<&str>,
        filters: Vec<SearchFilter>,
    ) -> Result<SeriesPage> {
        let mut store = Store::new(
            self.extension_pre.engine(),
            WasmState::new(self.extension_ctx.clone()),
        );

        let extension = self.extension_pre.instantiate(&mut store).await?;
        extension.search(store, query, cursor, filters).await
    }

    async fn get_series_info(&self, series_id: &str) -> Result<Series> {
        let mut store = Store::new(
            self.extension_pre.engine(),
            WasmState::new(self.extension_ctx.clone()),
        );

        let extension = self.extension_pre.instantiate(&mut store).await?;
        extension.get_series_info(store, series_id).await
    }

    async fn get_series_episodes(
        &self,
        series_id: &str,
        cursor: Option<&str>,
    ) -> Result<EpisodesPage> {
        let mut store = Store::new(
            self.extension_pre.engine(),
            WasmState::new(self.extension_ctx.clone()),
        );

        let extension = self.extension_pre.instantiate(&mut store).await?;
        extension
            .get_series_episodes(store, series_id, cursor)
            .await
    }

    async fn get_series_videos(&self, series_id: &str, episode_id: &str) -> Result<Vec<Video>> {
        let mut store = Store::new(
            self.extension_pre.engine(),
            WasmState::new(self.extension_ctx.clone()),
        );

        let extension = self.extension_pre.instantiate(&mut store).await?;
        extension
            .get_series_videos(store, series_id, episode_id)
            .await
    }
}

impl Tracker for WasmExtension {
    async fn list_entries(&self, status: WatchStatus, cursor: Option<&str>) -> Result<EntriesPage> {
        let mut store = Store::new(
            self.extension_pre.engine(),
            WasmState::new(self.extension_ctx.clone()),
        );
        let extension = self.extension_pre.instantiate(&mut store).await?;
        extension.list_entries(store, status, cursor).await
    }

    async fn get_entry(&self, series_id: &str) -> Result<Option<Entry>> {
        let mut store = Store::new(
            self.extension_pre.engine(),
            WasmState::new(self.extension_ctx.clone()),
        );
        let extension = self.extension_pre.instantiate(&mut store).await?;
        extension.get_entry(store, series_id).await
    }

    async fn save_entry(
        &self,
        series_id: &str,
        status: WatchStatus,
        details: EntryDetails,
    ) -> Result<Entry> {
        let mut store = Store::new(
            self.extension_pre.engine(),
            WasmState::new(self.extension_ctx.clone()),
        );
        let extension = self.extension_pre.instantiate(&mut store).await?;
        extension
            .save_entry(store, series_id, status, details)
            .await
    }

    async fn remove_entry(&self, series_id: &str) -> Result<()> {
        let mut store = Store::new(
            self.extension_pre.engine(),
            WasmState::new(self.extension_ctx.clone()),
        );
        let extension = self.extension_pre.instantiate(&mut store).await?;
        extension.remove_entry(store, series_id).await
    }
}
