use std::sync::Arc;

use anyhow::Result;
use wasmtime::component::{HasData, Resource, ResourceTable, ResourceTableError};

pub use self::keyvalue_ttl::store::KeyResponse;

pub use self::generated::nero::*;

mod generated {
    wasmtime::component::bindgen!({
        path: "wit",
        world: "nero:keyvalue-ttl/imports",
        imports: { default: async | trappable },
        with: {
            "nero:keyvalue-ttl/store/bucket": crate::OpenBucket,
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

#[async_trait::async_trait]
pub trait Store: Send + Sync {
    async fn open(&self, identifier: &str) -> Result<Arc<dyn Bucket>, Error>;
}

#[async_trait::async_trait]
pub trait Bucket: Send + Sync {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, Error>;

    async fn set(&self, key: &str, value: Vec<u8>, ttl_ms: Option<u32>) -> Result<(), Error>;

    async fn delete(&self, key: &str) -> Result<(), Error>;

    async fn exists(&self, key: &str) -> Result<bool, Error>;

    async fn list_keys(&self, cursor: Option<&str>) -> Result<KeyResponse, Error>;
}

pub struct KeyValueTTLCtx {
    store: Arc<dyn Store>,
}

impl KeyValueTTLCtx {
    pub fn new(store: Arc<dyn Store>) -> Self {
        Self { store }
    }
}

#[doc(hidden)]
pub struct OpenBucket(Arc<dyn Bucket>);

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
    async fn open(&mut self, identifier: String) -> Result<Resource<OpenBucket>, Error> {
        let bucket = self.ctx.store.open(&identifier).await?;
        Ok(self.table.push(OpenBucket(bucket))?)
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
        bucket: Resource<OpenBucket>,
        key: String,
    ) -> Result<Option<Vec<u8>>, Error> {
        Arc::clone(&self.table.get(&bucket)?.0).get(&key).await
    }

    async fn set(
        &mut self,
        bucket: Resource<OpenBucket>,
        key: String,
        value: Vec<u8>,
        ttl_ms: Option<u32>,
    ) -> Result<(), Error> {
        Arc::clone(&self.table.get(&bucket)?.0)
            .set(&key, value, ttl_ms)
            .await
    }

    async fn delete(&mut self, bucket: Resource<OpenBucket>, key: String) -> Result<(), Error> {
        Arc::clone(&self.table.get(&bucket)?.0).delete(&key).await
    }

    async fn exists(&mut self, bucket: Resource<OpenBucket>, key: String) -> Result<bool, Error> {
        Arc::clone(&self.table.get(&bucket)?.0).exists(&key).await
    }

    async fn list_keys(
        &mut self,
        bucket: Resource<OpenBucket>,
        cursor: Option<String>,
    ) -> Result<KeyResponse, Error> {
        Arc::clone(&self.table.get(&bucket)?.0)
            .list_keys(cursor.as_deref())
            .await
    }

    async fn drop(&mut self, bucket: Resource<OpenBucket>) -> wasmtime::Result<()> {
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
