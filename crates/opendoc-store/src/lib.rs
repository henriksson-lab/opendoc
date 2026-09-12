//! Content-addressed object stores and the repository built on top of them.
//!
//! `ObjectStore` is the backend-neutral contract (in-memory, on-disk,
//! browser-mirrored, OpenDAL); `Repository` is the document-level layer that
//! writes manifests, moves heads with compare-and-swap, keeps candidate heads
//! for concurrent writers, and maintains lookup indexes, tombstones and
//! version labels on top of whichever store it was handed.

mod error;
mod flat_store;
mod keys;
mod local_store;
mod mirrored;
mod object_store;
#[cfg(feature = "opendal")]
mod opendal_store;
mod pack;
mod repository;
mod repository_types;

pub use error::StoreError;
pub use flat_store::FlatObjectStore;
pub use local_store::LocalObjectStore;
pub use mirrored::{
    browser_volume, install_browser_volume, MirroredObjectStore, MirroredVolume, VolumeMutation,
};
pub use object_store::{
    verify_object_store_contract, ObjectStore, ObjectStoreLayout, StoreCapabilities,
};
#[cfg(feature = "opendal")]
pub use opendal_store::OpenDalObjectStore;
pub use pack::PackStats;
pub use repository::Repository;
pub use repository_types::*;

#[cfg(test)]
mod record_tests;
#[cfg(test)]
mod repository_tests;
#[cfg(test)]
mod store_tests;
#[cfg(test)]
mod version_history_tests;
