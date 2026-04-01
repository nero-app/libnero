mod file;
mod store;

use std::path::PathBuf;

use anyhow::Result;
use wasmtime::component::{HasData, Resource, ResourceTable, ResourceTableError};

use crate::store::FileStore;

pub use self::generated::nero::*;

mod generated {
    wasmtime::component::bindgen!({
        path: "wit",
        world: "nero:keyvalue-ttl/imports",
        imports: { default: async | trappable },
        with: {
            "nero:keyvalue-ttl/store/bucket": crate::Bucket,
        },
        trappable_error_type: {
            "nero:keyvalue-ttl/store/error" => crate::Error,
        },
    });
}

#[derive(Debug)]
pub enum Error {
    NoSuchBucket,
    AccessDenied,
    StorageLimitExceeded,
    Other(String),
}

impl From<ResourceTableError> for Error {
    fn from(err: ResourceTableError) -> Self {
        Self::Other(err.to_string())
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Self::Other(err.to_string())
    }
}

impl From<anyhow::Error> for Error {
    fn from(err: anyhow::Error) -> Self {
        Self::Other(err.to_string())
    }
}

pub struct Bucket;

pub struct KeyValueTTLCtx {
    store: FileStore,
}

impl KeyValueTTLCtx {
    pub async fn new(root: PathBuf, max_bytes: Option<u64>) -> Result<Self> {
        Ok(Self {
            store: FileStore::new(root, max_bytes).await?,
        })
    }
}

pub struct KeyValueTTL<'a> {
    ctx: &'a KeyValueTTLCtx,
    table: &'a mut ResourceTable,
}

impl<'a> KeyValueTTL<'a> {
    pub fn new(ctx: &'a KeyValueTTLCtx, table: &'a mut ResourceTable) -> Self {
        Self { ctx, table }
    }
}

impl keyvalue_ttl::store::Host for KeyValueTTL<'_> {
    async fn open(&mut self, identifier: String) -> Result<Resource<Bucket>, Error> {
        if !identifier.is_empty() {
            return Err(Error::NoSuchBucket);
        }
        Ok(self.table.push(Bucket)?)
    }

    fn convert_error(&mut self, err: Error) -> Result<keyvalue_ttl::store::Error> {
        Ok(match err {
            Error::NoSuchBucket => keyvalue_ttl::store::Error::NoSuchBucket,
            Error::AccessDenied => keyvalue_ttl::store::Error::AccessDenied,
            Error::StorageLimitExceeded => keyvalue_ttl::store::Error::StorageLimitExceeded,
            Error::Other(msg) => keyvalue_ttl::store::Error::Other(msg),
        })
    }
}

impl keyvalue_ttl::store::HostBucket for KeyValueTTL<'_> {
    async fn get(
        &mut self,
        _bucket: Resource<Bucket>,
        key: String,
    ) -> Result<Option<Vec<u8>>, Error> {
        self.ctx.store.get(&key).await
    }

    async fn set(
        &mut self,
        _bucket: Resource<Bucket>,
        key: String,
        value: Vec<u8>,
        ttl_ms: Option<u32>,
    ) -> Result<(), Error> {
        self.ctx.store.set(&key, value, ttl_ms).await
    }

    async fn delete(&mut self, _bucket: Resource<Bucket>, key: String) -> Result<(), Error> {
        self.ctx.store.delete(&key).await
    }

    async fn exists(&mut self, _bucket: Resource<Bucket>, key: String) -> Result<bool, Error> {
        self.ctx.store.exists(&key).await
    }

    async fn list_keys(
        &mut self,
        _bucket: Resource<Bucket>,
        cursor: Option<String>,
    ) -> Result<keyvalue_ttl::store::KeyResponse, Error> {
        let (keys, cursor) = self.ctx.store.list_keys(cursor.as_deref()).await?;
        Ok(keyvalue_ttl::store::KeyResponse { keys, cursor })
    }

    async fn drop(&mut self, bucket: Resource<Bucket>) -> wasmtime::Result<()> {
        self.table.delete(bucket)?;
        Ok(())
    }
}

pub trait KeyValueTTLView {
    fn keyvalue_ttl(&mut self) -> KeyValueTTL<'_>;
}

pub fn add_to_linker<T: KeyValueTTLView + Send>(
    l: &mut wasmtime::component::Linker<T>,
) -> Result<()> {
    keyvalue_ttl::store::add_to_linker::<T, HasKeyValueTTL>(l, T::keyvalue_ttl)
}

struct HasKeyValueTTL;
impl HasData for HasKeyValueTTL {
    type Data<'a> = KeyValueTTL<'a>;
}
