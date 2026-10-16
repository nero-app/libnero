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
    pub async fn inspect(path: impl AsRef<Path>) -> anyhow::Result<Metadata> {
        let bytes = tokio::fs::read(path).await?;

        match Payload::from_binary(&bytes)? {
            Payload::Component { metadata, .. } => Ok(metadata),
            Payload::Module(_) => anyhow::bail!("unsupported wasm module"),
        }
    }

    pub async fn load_extension_async<P: AsRef<Path>>(
        &self,
        path: P,
        options: ExtensionOptions,
    ) -> wasmtime::Result<WasmExtension> {
        let path = path.as_ref();

        let wasm_bytes = std::fs::read(path)?;
        let version = WasmExtension::get_version(&wasm_bytes)?;
        let component = Component::from_file(&self.engine, path)?;
        let metadata = match Payload::from_binary(&wasm_bytes)? {
            Payload::Component { metadata, .. } => metadata,
            Payload::Module(..) => unreachable!(),
        };

        let extension =
            WasmExtension::instantiate_async(version, &component, metadata, options.into()).await?;

        Ok(extension)
    }
}
