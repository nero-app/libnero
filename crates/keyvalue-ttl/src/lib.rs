#![allow(dead_code, unused_variables)]

use anyhow::Result;
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

pub enum Error {
    NoSuchStore,
    AccessDenied,
    StorageLimitExceeded,
    Other(String),
}

impl From<ResourceTableError> for Error {
    fn from(err: ResourceTableError) -> Self {
        Self::Other(err.to_string())
    }
}

pub struct Bucket;

pub struct KeyValueTTL<'a> {
    table: &'a mut ResourceTable,
}

impl<'a> KeyValueTTL<'a> {
    pub fn new(table: &'a mut ResourceTable) -> Self {
        Self { table }
    }
}

impl keyvalue_ttl::store::Host for KeyValueTTL<'_> {
    async fn open(&mut self, identifier: String) -> Result<Resource<Bucket>, Error> {
        todo!()
    }

    fn convert_error(&mut self, err: Error) -> Result<keyvalue_ttl::store::Error> {
        todo!()
    }
}

impl keyvalue_ttl::store::HostBucket for KeyValueTTL<'_> {
    async fn get(
        &mut self,
        bucket: Resource<Bucket>,
        key: String,
    ) -> Result<Option<Vec<u8>>, Error> {
        todo!()
    }

    async fn set(
        &mut self,
        bucket: Resource<Bucket>,
        key: String,
        value: Vec<u8>,
        ttl_ms: Option<u32>,
    ) -> Result<(), Error> {
        todo!()
    }

    async fn delete(&mut self, bucket: Resource<Bucket>, key: String) -> Result<(), Error> {
        todo!()
    }

    async fn exists(&mut self, bucket: Resource<Bucket>, key: String) -> Result<bool, Error> {
        todo!()
    }

    async fn list_keys(
        &mut self,
        bucket: Resource<Bucket>,
        cursor: Option<String>,
    ) -> Result<keyvalue_ttl::store::KeyResponse, Error> {
        todo!()
    }

    async fn drop(&mut self, rep: Resource<Bucket>) -> wasmtime::Result<()> {
        todo!()
    }
}

pub fn add_to_linker<T: Send>(
    l: &mut wasmtime::component::Linker<T>,
    f: fn(&mut T) -> KeyValueTTL<'_>,
) -> Result<()> {
    keyvalue_ttl::store::add_to_linker::<T, HasKeyValueTTL>(l, f)
}

struct HasKeyValueTTL;
impl HasData for HasKeyValueTTL {
    type Data<'a> = KeyValueTTL<'a>;
}
