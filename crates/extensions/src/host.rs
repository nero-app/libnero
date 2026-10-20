use std::path::Path;

use wasm_metadata::{Metadata, Payload};
use wasmtime::{Engine, component::Component};

use std::sync::Arc;

use nero_keyvalue_ttl::{KeyValueTTLCtx, Store};
use nero_portal::{CallbackTransport, Interaction, Opener, PortalCtx};

use crate::extension::{ExtensionCtx, WasmExtension};

#[derive(Default)]
pub struct ExtensionOptions {
    ctx: ExtensionCtx,
}

impl ExtensionOptions {
    pub fn with_keyvalue(mut self, store: Arc<dyn Store>) -> Self {
        self.ctx.keyvalue = Some(KeyValueTTLCtx::new(store));
        self
    }

    pub fn with_portal(
        mut self,
        interaction: Arc<dyn Interaction>,
        opener: Arc<dyn Opener>,
        callbacks: Arc<dyn CallbackTransport>,
    ) -> Self {
        self.ctx.portal = Some(PortalCtx::new(interaction, opener, callbacks));
        self
    }
}

impl From<ExtensionOptions> for ExtensionCtx {
    fn from(options: ExtensionOptions) -> Self {
        options.ctx
    }
}

pub struct WasmHost {
    engine: Engine,
}

impl Default for WasmHost {
    fn default() -> Self {
        Self {
            engine: {
                let mut config = wasmtime::Config::new();
                config.async_support(true);
                config.wasm_component_model(true);
                wasmtime::Engine::new(&config).unwrap()
            },
        }
    }
}

impl WasmHost {
    #[tracing::instrument(skip_all, fields(path = %path.as_ref().display()), err)]
    pub fn inspect(path: impl AsRef<Path>) -> anyhow::Result<Metadata> {
        tracing::debug!("inspecting component");
        let bytes = std::fs::read(path)?;

        match Payload::from_binary(&bytes)? {
            Payload::Component { metadata, .. } => {
                tracing::debug!(bytes = bytes.len(), "component inspected");
                Ok(metadata)
            }
            Payload::Module(_) => anyhow::bail!("unsupported wasm module"),
        }
    }

    #[tracing::instrument(
        skip_all,
        fields(path = %path.as_ref().display(), extension, wit_version),
        err
    )]
    pub fn load_extension<P: AsRef<Path>>(
        &self,
        path: P,
        options: ExtensionOptions,
    ) -> wasmtime::Result<WasmExtension> {
        let path = path.as_ref();
        tracing::debug!(
            keyvalue = options.ctx.keyvalue.is_some(),
            portal = options.ctx.portal.is_some(),
            "loading extension"
        );

        let wasm_bytes = std::fs::read(path)?;
        tracing::debug!(bytes = wasm_bytes.len(), "component read");
        let metadata = match Payload::from_binary(&wasm_bytes)? {
            Payload::Component { metadata, .. } => metadata,
            Payload::Module(..) => anyhow::bail!("unsupported wasm module"),
        };
        if let (Some(name), Some(version)) = (&metadata.name, &metadata.version) {
            tracing::Span::current().record("extension", format!("{name}@{version}"));
        }
        let version = WasmExtension::get_version(&wasm_bytes)?;
        tracing::Span::current().record("wit_version", tracing::field::display(&version));
        tracing::debug!("compiling component");
        let component = Component::new(&self.engine, &wasm_bytes)?;
        tracing::debug!("component compiled");

        let extension = WasmExtension::instantiate(version, &component, metadata, options.into())?;
        tracing::debug!(tracker = extension.supports_tracker(), "extension loaded");

        Ok(extension)
    }
}
