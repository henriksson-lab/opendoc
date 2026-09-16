//! Sharing one object store between the permission table and every document
//! actor.
//!
//! `opendoc-store` implements [`ObjectStore`] for `Box<T>` and `&T` but not
//! for `Arc<T>`, and that crate is not this one's to edit. A newtype costs
//! nothing and keeps the delegation in one readable place.

use opendoc_core::HashRef;
use opendoc_store::{ObjectStore, PackStats, StoreCapabilities, StoreError};
use std::sync::Arc;

#[derive(Debug)]
pub struct SharedStore<S: ObjectStore> {
    inner: Arc<S>,
}

impl<S: ObjectStore> SharedStore<S> {
    pub fn new(store: S) -> Self {
        Self {
            inner: Arc::new(store),
        }
    }

    pub fn inner(&self) -> &Arc<S> {
        &self.inner
    }
}

impl<S: ObjectStore> Clone for SharedStore<S> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<S: ObjectStore> ObjectStore for SharedStore<S> {
    fn capabilities(&self) -> StoreCapabilities {
        self.inner.capabilities()
    }

    fn put_if_absent(&self, hash: &HashRef, bytes: &[u8]) -> Result<bool, StoreError> {
        self.inner.put_if_absent(hash, bytes)
    }

    fn get(&self, hash: &HashRef) -> Result<Option<Vec<u8>>, StoreError> {
        self.inner.get(hash)
    }

    fn exists(&self, hash: &HashRef) -> Result<bool, StoreError> {
        self.inner.exists(hash)
    }

    fn put_named(&self, path: &str, bytes: &[u8]) -> Result<(), StoreError> {
        self.inner.put_named(path, bytes)
    }

    fn get_named(&self, path: &str) -> Result<Option<Vec<u8>>, StoreError> {
        self.inner.get_named(path)
    }

    fn list_prefix(&self, prefix: &str) -> Result<Vec<String>, StoreError> {
        self.inner.list_prefix(prefix)
    }

    fn compare_and_swap_head(
        &self,
        document_uuid: &str,
        branch: &str,
        expected: Option<&HashRef>,
        new: &HashRef,
    ) -> Result<bool, StoreError> {
        self.inner
            .compare_and_swap_head(document_uuid, branch, expected, new)
    }

    fn read_head(&self, document_uuid: &str, branch: &str) -> Result<Option<HashRef>, StoreError> {
        self.inner.read_head(document_uuid, branch)
    }

    fn compact_loose_objects_to_pack(&self, pack_name: &str) -> Result<PackStats, StoreError> {
        self.inner.compact_loose_objects_to_pack(pack_name)
    }
}
