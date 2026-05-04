use std::{path::PathBuf, sync::Arc};

use anyhow::Result;
use nero_file_store::Error as FileStoreError;
use nero_file_store::FileStore;
use tokio::task::spawn_blocking;
use wasmtime::component::{HasData, Resource, ResourceTable, ResourceTableError};

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

impl From<FileStoreError> for Error {
    fn from(err: FileStoreError) -> Self {
        match err {
            FileStoreError::StorageLimitExceeded => Self::StorageLimitExceeded,
            FileStoreError::Io(e) => Self::Other(e.to_string()),
            FileStoreError::Corrupt(msg) => Self::Other(msg),
        }
    }
}

pub struct Bucket;

pub struct KeyValueTTLCtx {
    store: Arc<FileStore>,
}

impl KeyValueTTLCtx {
    pub async fn new(root: PathBuf, max_bytes: Option<u64>) -> Result<Self> {
        let store = spawn_blocking(move || FileStore::new(root, max_bytes))
            .await
            .unwrap()?;

        Ok(Self {
            store: Arc::new(store),
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
        let store = self.ctx.store.clone();
        Ok(spawn_blocking(move || store.get(&key)).await.unwrap()?)
    }

    async fn set(
        &mut self,
        _bucket: Resource<Bucket>,
        key: String,
        value: Vec<u8>,
        ttl_ms: Option<u32>,
    ) -> Result<(), Error> {
        let store = self.ctx.store.clone();
        Ok(spawn_blocking(move || store.set(&key, value, ttl_ms))
            .await
            .unwrap()?)
    }

    async fn delete(&mut self, _bucket: Resource<Bucket>, key: String) -> Result<(), Error> {
        let store = self.ctx.store.clone();
        Ok(spawn_blocking(move || store.delete(&key)).await.unwrap()?)
    }

    async fn exists(&mut self, _bucket: Resource<Bucket>, key: String) -> Result<bool, Error> {
        let store = self.ctx.store.clone();
        Ok(spawn_blocking(move || store.exists(&key)).await.unwrap()?)
    }

    async fn list_keys(
        &mut self,
        _bucket: Resource<Bucket>,
        cursor: Option<String>,
    ) -> Result<keyvalue_ttl::store::KeyResponse, Error> {
        let store = self.ctx.store.clone();
        let result = spawn_blocking(move || store.list_keys(cursor.as_deref()))
            .await
            .unwrap()?;

        Ok(keyvalue_ttl::store::KeyResponse {
            keys: result.0,
            cursor: result.1,
        })
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
