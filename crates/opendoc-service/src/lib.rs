//! Authenticated collaboration service for OpenDoc.
//!
//! This crate is the network boundary and nothing else. It authenticates a
//! subject, decides from its own durable state what that subject may do,
//! accepts typed operations, makes them durable before acknowledging them,
//! serializes commits so two clients can never interleave into an invalid
//! head, and fans commits and presence out to everyone connected.
//!
//! Document semantics are not here and must not come here. Operations are
//! `opendoc_merge::Operation`, ordering and convergence are
//! `opendoc_merge::merge_operations`, storage is `opendoc_store::Repository`.
//! The crate deliberately does not depend on `opendoc-app`: it dispatches no
//! commands, so it needs neither the app facade nor layout, render, import or
//! spreadsheet evaluation.
//!
//! See `docs/adr/0015-service-crate-readiness-and-scope.md` for the readiness
//! assessment against ADR 0004's gate, what this enforces, and what it still
//! does not do.
//!
//! # Shape
//!
//! ```text
//!  websocket / http        one thread per document        object store
//!  ----------------        ----------------------         ------------
//!  server.rs  ───────────▶ document.rs ─────────────────▶ log.rs
//!    authenticate            validate authorship            snapshot object
//!    (identity.rs)           re-check the grant             segment object
//!                            (permission.rs)                manifest object
//!                            merge_operations               compare-and-swap
//!                            ◀── fan out ───                    the head
//! ```

pub mod client;
pub mod clock;
pub mod document;
pub mod error;
pub mod identity;
pub mod log;
pub mod origin;
pub mod permission;
pub mod protocol;
pub mod server;
pub mod service;
pub mod store;

pub use clock::{Clock, ManualClock};
pub use document::{DocumentHandle, DocumentStatus, RunningDocument, SubmitReceipt};
pub use error::{ServiceError, ServiceResult};
pub use identity::{AuthenticatedSubject, IdentityService, IssuedSession, SubjectRecord};
pub use log::{Commit, DocumentLog, SERVICE_BRANCH, SERVICE_DOCUMENT_FORMAT};
pub use origin::OriginPolicy;
pub use permission::{Action, GrantAuditEvent, PermissionService, Role};
pub use protocol::{
    ClientMessage, GrantAuditView, GrantView, PeerView, ServerMessage, PROTOCOL_VERSION,
};
pub use server::{router, serve, RunningServer};
pub use service::OpenDocService;
pub use store::SharedStore;

#[cfg(test)]
mod test_support;

#[cfg(test)]
mod app_client_tests;

#[cfg(test)]
mod identity_tests;
#[cfg(test)]
mod log_tests;
#[cfg(test)]
mod origin_tests;
#[cfg(test)]
mod permission_tests;
#[cfg(test)]
mod registry_tests;
#[cfg(test)]
mod transport_tests;
#[cfg(test)]
mod wire_fixture_tests;
