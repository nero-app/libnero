use std::path::Path;

use wasm_metadata::Payload;
use wasmtime::{Engine, component::Component};

use crate::extension::WasmExtension;

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
    pub async fn load_extension_async<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> wasmtime::Result<WasmExtension> {
        let path = path.as_ref();

        let wasm_bytes = std::fs::read(path)?;
        let version = WasmExtension::get_version(&wasm_bytes)?;
        let component = Component::from_file(&self.engine, path)?;
        let metadata = match Payload::from_binary(&wasm_bytes)? {
            Payload::Component { metadata, .. } => metadata,
            Payload::Module(..) => unreachable!(),
        };

        let extension = WasmExtension::instantiate_async(version, &component, metadata).await?;

        Ok(extension)
    }
}
