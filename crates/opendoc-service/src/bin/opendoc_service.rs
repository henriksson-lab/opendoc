//! Runs the collaboration service.
//!
//! Configuration is environment only, because a service that needs a config
//! file format needs a config file parser and this one does not yet earn one.
//!
//!   OPENDOC_SERVICE_ADDR      default 127.0.0.1:8787
//!   OPENDOC_SERVICE_ROOT      object store root, default ./opendoc-service-repo
//!   OPENDOC_SERVICE_SUBJECTS  "subject:actor:apikey" entries, comma separated
//!   OPENDOC_SERVICE_ORIGINS   browser origins allowed to connect, comma
//!                             separated (e.g. http://127.0.0.1:10084).
//!                             Empty — the default — means no page may
//!                             connect; programs are unaffected.
//!   OPENDOC_SERVICE_MAX_OPERATIONS_PER_SUBMIT
//!                             largest batch one Submit may carry, default
//!                             512. Every welcome carries the effective
//!                             value, so clients chunk to whatever this is
//!                             rather than to a number they restated.
//!   OPENDOC_SERVICE_MAX_OPEN_DOCUMENTS
//!                             documents this process will own threads for
//!                             at once, default 1024.
//!   OPENDOC_SERVICE_MAX_DOCUMENTS_PER_SUBJECT
//!                             documents one subject may create, default 256.
//!
//! Subjects are provisioned from the environment because there is no user
//! management here; see "What this does not do" in docs/adr/0015.

use opendoc_service::{serve, Clock, OpenDocService, OriginPolicy};
use opendoc_store::LocalObjectStore;
use std::net::SocketAddr;
use std::sync::Arc;

/// An optional numeric setting. A value that is present and unreadable is a
/// configuration error rather than a reason to fall back to the default: an
/// operator who set a limit and got the default instead would have no way to
/// tell.
fn usize_env(name: &str) -> Result<Option<usize>, Box<dyn std::error::Error>> {
    match std::env::var(name) {
        Ok(value) if value.trim().is_empty() => Ok(None),
        Ok(value) => {
            Ok(Some(value.trim().parse::<usize>().map_err(|error| {
                format!("{name} is not a number: {error}")
            })?))
        }
        Err(_) => Ok(None),
    }
}

/// The same, in milliseconds.
fn u64_env(name: &str) -> Result<Option<u64>, Box<dyn std::error::Error>> {
    match std::env::var(name) {
        Ok(value) if value.trim().is_empty() => Ok(None),
        Ok(value) => {
            Ok(Some(value.trim().parse::<u64>().map_err(|error| {
                format!("{name} is not a number: {error}")
            })?))
        }
        Err(_) => Ok(None),
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let address: SocketAddr = std::env::var("OPENDOC_SERVICE_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:8787".to_string())
        .parse()?;
    let root = std::env::var("OPENDOC_SERVICE_ROOT")
        .unwrap_or_else(|_| "./opendoc-service-repo".to_string());
    let store = LocalObjectStore::new(&root);
    let origins =
        OriginPolicy::parse_list(&std::env::var("OPENDOC_SERVICE_ORIGINS").unwrap_or_default());
    let mut service =
        OpenDocService::new(store, Clock::system()).with_allowed_origins(origins.clone());
    if let Some(limit) = usize_env("OPENDOC_SERVICE_MAX_OPERATIONS_PER_SUBMIT")? {
        service = service.with_max_operations_per_submit(limit);
    }
    if let Some(limit) = usize_env("OPENDOC_SERVICE_MAX_OPEN_DOCUMENTS")? {
        service = service.with_max_open_documents(limit);
    }
    if let Some(limit) = usize_env("OPENDOC_SERVICE_MAX_DOCUMENTS_PER_SUBJECT")? {
        service = service.with_max_documents_per_subject(limit);
    }
    // How long a document nothing is using keeps its thread. Beside the
    // limits because it is the other half of the same decision: the limit
    // says how many documents this process serves at once, and this says how
    // quickly one that nobody is using gives its place up.
    if let Some(idle_timeout_ms) = u64_env("OPENDOC_SERVICE_DOCUMENT_IDLE_TIMEOUT_MS")? {
        service = service.with_document_idle_timeout_ms(idle_timeout_ms);
    }
    let service = Arc::new(service);

    let subjects = std::env::var("OPENDOC_SERVICE_SUBJECTS").unwrap_or_default();
    let mut registered = 0usize;
    for entry in subjects.split(',').map(str::trim).filter(|e| !e.is_empty()) {
        let parts: Vec<&str> = entry.splitn(3, ':').collect();
        let [subject, actor, api_key] = parts.as_slice() else {
            return Err(format!("subject entry {entry} is not subject:actor:apikey").into());
        };
        service
            .identity()
            .register_subject(subject, actor, api_key)?;
        registered += 1;
    }
    if registered == 0 {
        eprintln!(
            "opendoc-service: no subjects registered; set OPENDOC_SERVICE_SUBJECTS or nobody can sign in"
        );
    }

    let running = serve(service, address).await?;
    eprintln!(
        "opendoc-service: listening on {} with {registered} subject(s), store at {root}",
        running.local_address()
    );
    if origins.is_empty() {
        eprintln!(
            "opendoc-service: no browser origin is allowed; set OPENDOC_SERVICE_ORIGINS or only native clients can connect"
        );
    } else {
        eprintln!(
            "opendoc-service: browser origins allowed: {}",
            origins.entries().join(", ")
        );
    }

    // Run until interrupted. `RunningServer::shutdown` *sends* the shutdown
    // signal and then waits for the server task, so calling it here — as this
    // binary used to — started the listener and immediately tore it down: the
    // process printed that it was listening, served nothing, and exited. The
    // only client this crate had was its own test suite, which binds its own
    // server, so nothing noticed until a browser tried to connect.
    match tokio::signal::ctrl_c().await {
        Ok(()) => eprintln!("opendoc-service: interrupted, shutting down"),
        Err(error) => {
            eprintln!("opendoc-service: cannot listen for interrupts ({error}), shutting down")
        }
    }
    running.shutdown().await;
    Ok(())
}
