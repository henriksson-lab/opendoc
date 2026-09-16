//! The canonical OpenDoc document model.
//!
//! Everything the rest of the workspace stores, signs, merges, renders or
//! imports is expressed in these types. The crate is a flat namespace by
//! design — every model type is re-exported at the crate root — but each
//! domain owns its own module: identity and hashing, the document root and
//! its validation walk, blocks, tables, images, inlines, annotations,
//! citations, page setup, measurements, block properties, and diagnostics.

mod annotation;
mod block;
mod block_properties;
mod bookmark;
mod citation;
mod document;
mod ids;
mod image;
mod inline;
mod list;
mod measure;
mod page;
mod table;
mod text_sequence;
mod warning;

pub use annotation::*;
pub use block::*;
pub use block_properties::*;
pub use bookmark::*;
pub use citation::*;
pub use document::*;
pub use ids::*;
pub use image::*;
pub use inline::*;
pub use list::*;
pub use measure::*;
pub use page::*;
pub use table::*;
pub use text_sequence::*;
pub use warning::*;

#[cfg(test)]
mod annotation_tests;
#[cfg(test)]
mod bookmark_tests;
#[cfg(test)]
mod citation_tests;
#[cfg(test)]
mod document_tests;
#[cfg(test)]
mod identity_tests;
#[cfg(test)]
mod inline_tests;
#[cfg(test)]
mod page_tests;
#[cfg(test)]
mod property_tests;
#[cfg(test)]
mod table_tests;
