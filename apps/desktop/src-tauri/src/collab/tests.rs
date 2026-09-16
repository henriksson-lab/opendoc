//! The native transport, against a real service over a real socket.
//!
//! No Tauri window is involved: [`NativeCollab`] takes the document behind an
//! `Arc<Mutex<OpenDocApp>>` and an emitter closure, so everything below this
//! line is the shipped path apart from where the statuses are delivered. That
//! is deliberate — a transport that can only be tested by clicking is a
//! transport with no tests.

use super::*;
use opendoc_service::{serve, Clock, IdentityService, OpenDocService, RunningServer};
use opendoc_store::LocalObjectStore;
use std::sync::atomic::{AtomicU64, Ordering};

const ALICE_KEY: &str = "alice-api-key-0123456789";
const BOB_KEY: &str = "bob-api-key-0123456789";

static NEXT_ROOT: AtomicU64 = AtomicU64::new(1);

struct TempRoot(std::path::PathBuf);

impl TempRoot {
    fn new(label: &str) -> Self {
        let unique = NEXT_ROOT.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "opendoc-native-collab-{label}-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("creating the test root");
        Self(path)
    }

    fn store(&self) -> LocalObjectStore {
        LocalObjectStore::new(self.0.clone())
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn register(identity: &IdentityService) {
    identity
        .register_subject("alice", "actor-alice", ALICE_KEY)
        .expect("registering alice");
    identity
        .register_subject("bob", "actor-bob", BOB_KEY)
        .expect("registering bob");
}

/// A service on an ephemeral port, plus the runtime that drives it.
///
/// The runtime is separate from the session thread's on purpose: in the real
/// shell the service is another process, and a test that shared one executor
/// with it could pass on an interleaving a socket would never produce.
struct Service {
    runtime: tokio::runtime::Runtime,
    service: Arc<OpenDocService<LocalObjectStore>>,
    server: Option<RunningServer>,
    address: SocketAddr,
}

impl Service {
    fn start(root: &TempRoot) -> Self {
        Self::start_with(root, |service| service)
    }

    /// A service whose limits the test chooses. The submit cap is a deployment
    /// setting the welcome carries, so a test can reach it with ten
    /// operations instead of five hundred and thirteen.
    fn start_with(
        root: &TempRoot,
        configure: impl FnOnce(OpenDocService<LocalObjectStore>) -> OpenDocService<LocalObjectStore>,
    ) -> Self {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("a runtime");
        let service = Arc::new(configure(OpenDocService::new(
            root.store(),
            Clock::system(),
        )));
        register(service.identity());
        let server = runtime
            .block_on(serve(
                Arc::clone(&service),
                "127.0.0.1:0".parse().expect("an address"),
            ))
            .expect("binding an ephemeral port");
        let address = server.local_address();
        Self {
            runtime,
            service,
            server: Some(server),
            address,
        }
    }

    fn url(&self) -> String {
        format!("http://{}", self.address)
    }

    fn stop(&mut self) {
        if let Some(server) = self.server.take() {
            self.runtime.block_on(server.shutdown());
        }
    }
}

impl Drop for Service {
    fn drop(&mut self) {
        self.stop();
    }
}

/// The statuses the shell pushed to the page, in order.
#[derive(Clone, Default)]
struct Feed(Arc<Mutex<Vec<CollabStatus>>>);

impl Feed {
    fn emitter(&self) -> Arc<dyn Fn(CollabStatus) + Send + Sync> {
        let feed = Arc::clone(&self.0);
        Arc::new(move |status| {
            if let Ok(mut held) = feed.lock() {
                held.push(status);
            }
        })
    }

    fn last(&self) -> Option<CollabStatus> {
        self.0.lock().ok().and_then(|held| held.last().cloned())
    }

    fn kinds(&self) -> Vec<String> {
        self.0
            .lock()
            .map(|held| {
                held.iter()
                    .filter_map(|status| status.notice.as_ref().map(|notice| notice.kind.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }
}

fn wait_for(label: &str, mut predicate: impl FnMut() -> bool) {
    for _ in 0..600 {
        if predicate() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("timed out waiting for {label}");
}

fn app_with_document() -> Arc<Mutex<OpenDocApp>> {
    Arc::new(Mutex::new(OpenDocApp::new_empty_document()))
}

fn run_text(app: &Arc<Mutex<OpenDocApp>>) -> String {
    let app = app.lock().expect("the app");
    let document = app.source_document();
    document
        .blocks
        .iter()
        .flat_map(|block| block.content.iter())
        .filter_map(|inline| match inline {
            opendoc_core::Inline::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

/// Types through the ordinary command surface, which is the only way a gesture
/// becomes an operation.
fn type_text(app: &Arc<Mutex<OpenDocApp>>, text: &str) {
    let mut held = app.lock().expect("the app");
    let (block_id, inline_id, offset) = {
        let document = held.source_document();
        let block = document.blocks.first().expect("a block to type into");
        let inline = block.content.first().expect("a run to type into");
        let (id, length) = match inline {
            opendoc_core::Inline::Text { id, text, .. } => {
                (id.as_str().to_string(), text.chars().count())
            }
            _ => panic!("the first inline is not text"),
        };
        (block.id.as_str().to_string(), id, length)
    };
    let position = serde_json::json!({
        "block_id": block_id,
        "inline_id": inline_id,
        "offset": offset,
    });
    held.dispatch_command(
        "apply_editor_input",
        serde_json::json!({
            "selection": { "anchor": position, "focus": position },
            "input_type": "insertText",
            "data": text,
        }),
    )
    .expect("typing");
}

fn connect(
    collab: &Arc<NativeCollab>,
    service: &Service,
    subject: &str,
    api_key: &str,
    document_uuid: Option<&str>,
) -> CollabStatus {
    collab
        .connect(ConnectOptions {
            service_url: service.url(),
            subject: subject.to_string(),
            api_key: api_key.to_string(),
            document_uuid: document_uuid.map(ToString::to_string),
            display_name: format!("{subject} native"),
            title: Some("Shared from the shell".to_string()),
        })
        .expect("the session starts")
}

// ---- The tests -----------------------------------------------------------

#[test]
fn the_status_carries_the_fields_the_frontend_reads() {
    // The same field list `opendoc-wasm`'s
    // `the_status_serializes_with_the_fields_the_frontend_reads` pins, so
    // `collab.ts` has one type for both runtimes.
    let value = serde_json::to_value(CollabStatus::idle()).expect("serializing");
    for field in [
        "phase",
        "document_uuid",
        "display_name",
        "commit_seq",
        "acknowledged_seq",
        "pending_operations",
        "can_submit",
        "reconnect_requested",
        "document_changed",
        "notice",
        "session",
    ] {
        assert!(
            value.get(field).is_some(),
            "the frontend reads status.{field}"
        );
    }
    assert_eq!(value["phase"], "idle");
}

#[test]
fn an_https_address_is_refused_rather_than_quietly_downgraded() {
    let error = resolve_address("https://docs.example:443").expect_err("must refuse");
    assert!(error.contains("http/ws only"), "{error}");
    assert!(resolve_address("   ").is_err());
    assert!(resolve_address("127.0.0.1:8787").is_ok());
    assert!(resolve_address("http://127.0.0.1:8787/").is_ok());
}

#[test]
fn native_service_address_never_preserves_ignored_uri_secrets_in_share_links() {
    // The native transport accepts only an authority, unlike the browser's
    // reverse-proxy-aware URL path.  It must reject URI components it cannot
    // use rather than stripping them for the socket and later copying them
    // back out in a credential-free link.
    for value in [
        "http://alice:secret@127.0.0.1:8787",
        "http://127.0.0.1:8787/service",
        "http://127.0.0.1:8787/?token=secret",
        "http://127.0.0.1:8787/#fragment",
    ] {
        assert!(resolve_address(value).is_err(), "must reject {value}");
        assert!(
            canonical_service_url(value).is_err(),
            "must not copy {value}"
        );
    }
    assert_eq!(
        canonical_service_url(" http://127.0.0.1:8787/ ").unwrap(),
        "http://127.0.0.1:8787"
    );
}

/// The native shell as a real client: it signs in, creates the document,
/// adopts the welcome, and the app it drives becomes the service's document.
#[test]
fn the_shell_joins_a_document_it_created_and_the_app_adopts_the_service_answers() {
    let root = TempRoot::new("join");
    let service = Service::start(&root);
    let app = app_with_document();
    let feed = Feed::default();
    let collab = Arc::new(NativeCollab::new(Arc::clone(&app), feed.emitter()));

    let connecting = connect(&collab, &service, "alice", ALICE_KEY, None);
    assert_eq!(connecting.phase, "connecting");

    wait_for("the session to go live", || {
        feed.last().is_some_and(|status| status.phase == "live")
    });
    let live = feed.last().expect("a status");
    let session = live.session.expect("the service's answers");
    assert_eq!(session.subject, "alice");
    assert_eq!(session.actor, "actor-alice");
    assert_eq!(session.role, OpenDocServiceRole::Owner);
    assert!(!session.document_uuid.is_empty());
    assert_eq!(
        app.lock().expect("the app").actor_id(),
        "actor-alice",
        "the actor is the service's, not this process's"
    );
    // The service creates a document with the one paragraph an empty document
    // has, so there is somewhere to put a caret.
    assert_eq!(
        app.lock().expect("the app").source_document().blocks.len(),
        1
    );

    collab.disconnect();
    assert!(
        app.lock().expect("the app").service_session().is_none(),
        "the service's answers must not outlive the session"
    );
}

/// ACL calls are a native bridge capability, not a second permission model:
/// they use the authenticated client held by the shell and the service decides
/// whether the request is allowed.
#[test]
fn native_access_bridge_lists_grants_mutates_them_and_never_returns_a_bearer_link() {
    let root = TempRoot::new("native-access");
    let service = Service::start(&root);
    let app = app_with_document();
    let feed = Feed::default();
    let collab = Arc::new(NativeCollab::new(Arc::clone(&app), feed.emitter()));

    connect(&collab, &service, "alice", ALICE_KEY, None);
    wait_for("owner session to go live", || {
        feed.last().is_some_and(|status| status.phase == "live")
    });
    let document_uuid = feed.last().expect("live status").document_uuid;

    let initial = service
        .runtime
        .block_on(collab.list_grants())
        .expect("the server permits its owner to list grants");
    assert!(initial
        .iter()
        .any(|grant| grant.subject == "alice" && grant.role == Role::Owner));

    service
        .runtime
        .block_on(collab.set_grant("bob".to_string(), Some("commenter".to_string())))
        .expect("the server permits its owner to grant access");
    let changed = service
        .runtime
        .block_on(collab.list_grants())
        .expect("list after grant");
    assert!(changed
        .iter()
        .any(|grant| grant.subject == "bob" && grant.role == Role::Commenter));

    let link = collab.share_link().expect("credential-free share link");
    assert_eq!(
        link,
        format!("{}/v1/documents/{document_uuid}", service.url())
    );
    assert!(!link.contains(ALICE_KEY));
    assert!(!link.contains("token="));

    service
        .runtime
        .block_on(collab.set_grant("bob".to_string(), None))
        .expect("the server permits its owner to revoke access");
    assert!(service
        .runtime
        .block_on(collab.list_grants())
        .expect("list after revoke")
        .iter()
        .all(|grant| grant.subject != "bob"));

    collab.disconnect();
    assert!(
        collab.share_link().is_err(),
        "disconnect clears the access context"
    );
}

/// Two native shells on one document, over real sockets: one types, the other
/// sees it, and both agree byte for byte with the service.
#[test]
fn two_native_shells_converge_through_the_service() {
    let root = TempRoot::new("converge");
    let service = Service::start(&root);

    let alice_app = app_with_document();
    let alice_feed = Feed::default();
    let alice = Arc::new(NativeCollab::new(
        Arc::clone(&alice_app),
        alice_feed.emitter(),
    ));
    connect(&alice, &service, "alice", ALICE_KEY, None);
    wait_for("alice to go live", || {
        alice_feed
            .last()
            .is_some_and(|status| status.phase == "live")
    });
    let document_uuid = alice_feed
        .last()
        .and_then(|status| status.session)
        .expect("a session")
        .document_uuid;

    // Only the service writes grants, and only an owner may ask it to.
    service
        .service
        .set_grant(&document_uuid, "alice", "bob", Some(Role::Editor))
        .expect("granting bob edit access");

    let bob_app = app_with_document();
    let bob_feed = Feed::default();
    let bob = Arc::new(NativeCollab::new(Arc::clone(&bob_app), bob_feed.emitter()));
    connect(&bob, &service, "bob", BOB_KEY, Some(&document_uuid));
    wait_for("bob to go live", || {
        bob_feed.last().is_some_and(|status| status.phase == "live")
    });

    type_text(&alice_app, "alice-typed");
    wait_for("bob to see alice's text", || {
        run_text(&bob_app).contains("alice-typed")
    });
    type_text(&bob_app, "bob-typed");
    wait_for("alice to see bob's text", || {
        run_text(&alice_app).contains("bob-typed")
    });

    // Byte-level agreement, which is the only statement of convergence that
    // means anything (ADR 0007).
    wait_for("the two replicas to agree byte for byte", || {
        bytes_of(&alice_app) == bytes_of(&bob_app)
    });
    let server_document = service
        .runtime
        .block_on(async {
            service
                .service
                .document(&document_uuid)
                .expect("the document")
                .snapshot()
                .await
        })
        .expect("the server's document");
    assert_eq!(
        bytes_of(&alice_app),
        opendoc_format_bytes(&server_document),
        "the shell and the service disagree"
    );

    // Presence: the service attests who is here, and both sides are told.
    wait_for("alice to be told bob is here", || {
        alice_feed
            .last()
            .and_then(|status| status.session)
            .is_some_and(|session| session.peers.iter().any(|peer| peer.subject == "bob"))
    });
    let peers = alice_feed
        .last()
        .and_then(|status| status.session)
        .expect("a session")
        .peers;
    let bob_peer = peers
        .iter()
        .find(|peer| peer.subject == "bob")
        .expect("bob is a peer");
    assert_eq!(bob_peer.actor, "actor-bob");
    assert_eq!(bob_peer.role, OpenDocServiceRole::Editor);
    assert_eq!(bob_peer.connections, 1);

    // And the watermark is durability: the service acknowledges only after the
    // branch head names a manifest naming the segment.
    assert!(
        alice_feed
            .last()
            .is_some_and(|status| status.acknowledged_seq > 0),
        "nothing was acknowledged as durable"
    );

    alice.disconnect();
    bob.disconnect();
}

/// A viewer is told why its typing is not being sent, and the service's state
/// is untouched.
#[test]
fn a_viewer_is_told_why_its_typing_is_not_being_sent() {
    let root = TempRoot::new("viewer");
    let service = Service::start(&root);
    let alice_app = app_with_document();
    let alice_feed = Feed::default();
    let alice = Arc::new(NativeCollab::new(
        Arc::clone(&alice_app),
        alice_feed.emitter(),
    ));
    connect(&alice, &service, "alice", ALICE_KEY, None);
    wait_for("alice to go live", || {
        alice_feed
            .last()
            .is_some_and(|status| status.phase == "live")
    });
    let document_uuid = alice_feed
        .last()
        .and_then(|status| status.session)
        .expect("a session")
        .document_uuid;
    service
        .service
        .set_grant(&document_uuid, "alice", "bob", Some(Role::Viewer))
        .expect("granting bob read access");
    alice.disconnect();

    let bob_app = app_with_document();
    let bob_feed = Feed::default();
    let bob = Arc::new(NativeCollab::new(Arc::clone(&bob_app), bob_feed.emitter()));
    connect(&bob, &service, "bob", BOB_KEY, Some(&document_uuid));
    wait_for("bob to go live", || {
        bob_feed.last().is_some_and(|status| status.phase == "live")
    });
    assert!(
        !bob_feed.last().expect("a status").can_submit,
        "a viewer must not be told it can submit"
    );

    type_text(&bob_app, "viewer-typed");
    wait_for("bob to be told why", || {
        bob_feed.kinds().iter().any(|kind| kind == "not-an-editor")
    });
    let commit_seq = bob_feed.last().expect("a status").commit_seq;
    // Nothing was committed, so the service's state is exactly where it was.
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(
        bob_feed.last().expect("a status").commit_seq,
        commit_seq,
        "a viewer's refused typing must not move the service's commit sequence"
    );
    bob.disconnect();
}

/// A service that is not there is reported as over, not as connecting.
///
/// Nothing retries a sign-in — the reconnect loop starts after it — so a
/// "connecting…" pill against an address with nothing behind it would describe
/// a session thread that has already exited.
#[test]
fn a_service_that_is_not_there_is_reported_as_over_rather_than_as_connecting() {
    // A port that was bound and released: nothing is listening, and nothing
    // else in the test suite owns it.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a free port");
    let address = listener.local_addr().expect("its address");
    drop(listener);

    let app = app_with_document();
    let feed = Feed::default();
    let collab = Arc::new(NativeCollab::new(Arc::clone(&app), feed.emitter()));
    collab
        .connect(ConnectOptions {
            service_url: format!("http://{address}"),
            subject: "alice".to_string(),
            api_key: ALICE_KEY.to_string(),
            document_uuid: None,
            display_name: "Alice".to_string(),
            title: None,
        })
        .expect("the attempt starts");
    wait_for("the shell to give up", || {
        feed.last().is_some_and(|status| status.phase == "closed")
    });
    let notice = feed
        .last()
        .and_then(|status| status.notice)
        .expect("an explanation");
    assert_eq!(notice.kind, "connect-failed");
    assert!(!notice.resumable, "nothing is retrying this");
    assert!(
        app.lock().expect("the app").service_session().is_none(),
        "a failed connection must not leave a session behind"
    );
}

/// A grant revoked under a live session: the service closes the connection,
/// and the shell says plainly that it cannot resume rather than reconnecting
/// into the same refusal.
#[test]
fn a_revoked_grant_ends_the_session_and_says_it_cannot_be_resumed() {
    let root = TempRoot::new("revoked");
    let service = Service::start(&root);
    let alice_app = app_with_document();
    let alice_feed = Feed::default();
    let alice = Arc::new(NativeCollab::new(
        Arc::clone(&alice_app),
        alice_feed.emitter(),
    ));
    connect(&alice, &service, "alice", ALICE_KEY, None);
    wait_for("alice to go live", || {
        alice_feed
            .last()
            .is_some_and(|status| status.phase == "live")
    });
    let document_uuid = alice_feed
        .last()
        .and_then(|status| status.session)
        .expect("a session")
        .document_uuid;
    service
        .service
        .set_grant(&document_uuid, "alice", "bob", Some(Role::Editor))
        .expect("granting bob edit access");

    let bob_app = app_with_document();
    let bob_feed = Feed::default();
    let bob = Arc::new(NativeCollab::new(Arc::clone(&bob_app), bob_feed.emitter()));
    connect(&bob, &service, "bob", BOB_KEY, Some(&document_uuid));
    wait_for("bob to go live", || {
        bob_feed.last().is_some_and(|status| status.phase == "live")
    });

    service
        .service
        .set_grant(&document_uuid, "alice", "bob", None)
        .expect("revoking bob");

    wait_for("bob's session to end", || {
        bob_feed
            .last()
            .is_some_and(|status| status.phase == "closed")
    });
    let notice = bob_feed
        .last()
        .and_then(|status| status.notice)
        .expect("an explanation");
    assert_eq!(
        notice.kind, "closed-forbidden",
        "the service says why it closed, and that is what must be shown: {notice:?}"
    );
    assert!(
        !notice.resumable,
        "a revoked grant refuses the next handshake too, so this is not resumable"
    );
    // And bob keeps the document he had: losing access is not losing the copy
    // he already held.
    assert!(!bytes_of(&bob_app).is_empty());
    alice.disconnect();
    bob.disconnect();
}

/// How many operations this replica has authored, read from the app rather
/// than from a published status: nothing publishes one while the session is
/// disconnected, which is exactly when this question matters.
fn local_operation_count(app: &Arc<Mutex<OpenDocApp>>) -> usize {
    app.lock().expect("the app").local_operations_after(0).len()
}

fn bytes_of(app: &Arc<Mutex<OpenDocApp>>) -> Vec<u8> {
    let app = app.lock().expect("the app");
    opendoc_format_bytes(app.source_document())
}

fn opendoc_format_bytes(document: &opendoc_app::Document) -> Vec<u8> {
    opendoc_format::encode_canonical_cbor(document).expect("canonical CBOR")
}

// ---- A cable that can be cut ---------------------------------------------
//
// The reconnect path is four steps in a fixed order (ADR 0018), and until now
// nothing on this side ran any of them: `two_native_shells_converge` never
// loses a socket, and there is no native equivalent of the browser harness's
// `dropSocket`. `NativeCollab` owns its socket, so a test cannot reach it —
// but it *can* own the wire underneath.
//
// A TCP relay in front of the real service. Cutting it aborts every relay in
// flight and refuses new ones, which is what an unplugged cable looks like
// from inside the process: the socket dies, the intention to be connected
// stays, and the retry loop runs. Mending it lets the next attempt through.
// Everything between the two is shipped code.

struct Cable {
    address: SocketAddr,
    cut: Arc<std::sync::atomic::AtomicBool>,
    relays: Arc<Mutex<Vec<tokio::task::AbortHandle>>>,
    runtime: Option<tokio::runtime::Runtime>,
}

impl Cable {
    fn open(upstream: SocketAddr) -> Self {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("a runtime for the cable");
        // Bound synchronously so the address is known before anything is told
        // to connect to it.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a free port");
        listener.set_nonblocking(true).expect("non-blocking");
        let address = listener.local_addr().expect("its address");
        let cut = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let relays: Arc<Mutex<Vec<tokio::task::AbortHandle>>> = Arc::new(Mutex::new(Vec::new()));
        let held_cut = Arc::clone(&cut);
        let held_relays = Arc::clone(&relays);
        runtime.spawn(async move {
            let listener =
                tokio::net::TcpListener::from_std(listener).expect("adopting the listener");
            loop {
                let Ok((mut client, _)) = listener.accept().await else {
                    continue;
                };
                if held_cut.load(std::sync::atomic::Ordering::SeqCst) {
                    // Accept and drop: the caller sees a connection that dies,
                    // which is the failure a pulled cable produces.
                    continue;
                }
                let relay = tokio::spawn(async move {
                    if let Ok(mut server) = tokio::net::TcpStream::connect(upstream).await {
                        let _ = tokio::io::copy_bidirectional(&mut client, &mut server).await;
                    }
                });
                if let Ok(mut held) = held_relays.lock() {
                    held.push(relay.abort_handle());
                }
            }
        });
        Self {
            address,
            cut,
            relays,
            runtime: Some(runtime),
        }
    }

    fn url(&self) -> String {
        format!("http://{}", self.address)
    }

    fn cut(&self) {
        self.cut.store(true, std::sync::atomic::Ordering::SeqCst);
        if let Ok(mut held) = self.relays.lock() {
            for relay in held.drain(..) {
                relay.abort();
            }
        }
    }

    fn mend(&self) {
        self.cut.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

impl Drop for Cable {
    fn drop(&mut self) {
        self.cut();
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
    }
}

fn connect_to(
    collab: &Arc<NativeCollab>,
    url: String,
    subject: &str,
    api_key: &str,
    document_uuid: Option<&str>,
) -> CollabStatus {
    collab
        .connect(ConnectOptions {
            service_url: url,
            subject: subject.to_string(),
            api_key: api_key.to_string(),
            document_uuid: document_uuid.map(ToString::to_string),
            display_name: format!("{subject} native"),
            title: Some("Shared from the shell".to_string()),
        })
        .expect("the session starts")
}

/// The four reconnect steps, on the native path, over a cable that is cut and
/// mended: take what this replica holds, join from the welcome, read the
/// acknowledged watermark back out of the log the welcome carried, and replay
/// the tail the service never got.
///
/// There was no test for any of this here. The browser has one in `npm run
/// e2e`; this side had `two_native_shells_converge_through_the_service`, which
/// never loses a socket.
#[test]
fn the_shell_replays_the_work_it_was_holding_when_the_cable_is_mended() {
    let root = TempRoot::new("reconnect");
    let service = Service::start(&root);
    let cable = Cable::open(service.address);

    // Alice reaches the service through the cable; Bob goes straight to it, so
    // cutting the cable is Alice's outage and not the service's.
    let alice_app = app_with_document();
    let alice_feed = Feed::default();
    let alice = Arc::new(NativeCollab::new(
        Arc::clone(&alice_app),
        alice_feed.emitter(),
    ));
    connect_to(&alice, cable.url(), "alice", ALICE_KEY, None);
    wait_for("alice to go live", || {
        alice_feed
            .last()
            .is_some_and(|status| status.phase == "live")
    });
    let document_uuid = alice_feed
        .last()
        .and_then(|status| status.session)
        .expect("a session")
        .document_uuid;
    service
        .service
        .set_grant(&document_uuid, "alice", "bob", Some(Role::Editor))
        .expect("granting bob edit access");

    let bob_app = app_with_document();
    let bob_feed = Feed::default();
    let bob = Arc::new(NativeCollab::new(Arc::clone(&bob_app), bob_feed.emitter()));
    connect_to(&bob, service.url(), "bob", BOB_KEY, Some(&document_uuid));
    wait_for("bob to go live", || {
        bob_feed.last().is_some_and(|status| status.phase == "live")
    });

    type_text(&alice_app, "before-the-drop");
    wait_for("bob to see alice's first edit", || {
        run_text(&bob_app).contains("before-the-drop")
    });

    // The cable goes.
    cable.cut();
    wait_for("alice's session to report the drop", || {
        alice_feed
            .last()
            .is_some_and(|status| status.phase == "reconnecting")
    });

    // Work on both sides while alice is away. Hers cannot leave; his is
    // committed and will be in the log her reconnect reads.
    type_text(&alice_app, "-while-cut");
    type_text(&bob_app, "-bob-meanwhile");
    // Read from the app rather than from the status feed: no status is
    // published while nothing is connected, so the feed's last one still says
    // what the connection said before it died.
    assert_eq!(
        local_operation_count(&alice_app),
        2,
        "alice authored one operation before the cut and one after it"
    );

    cable.mend();
    wait_for("alice to reconnect", || {
        alice_feed
            .last()
            .is_some_and(|status| status.phase == "live")
    });

    // Step 2: she is the service's document, so she has what she missed.
    wait_for("alice to catch up on what she missed", || {
        run_text(&alice_app).contains("-bob-meanwhile")
    });
    // Step 4: the tail she was holding reaches him, with its original ids —
    // he already has "before-the-drop", so a re-authored replay would appear
    // as a second copy rather than as the same text.
    wait_for("alice's offline work to reach bob", || {
        run_text(&bob_app).contains("-while-cut")
    });
    wait_for("the two replicas to agree byte for byte", || {
        bytes_of(&alice_app) == bytes_of(&bob_app)
    });
    let text = run_text(&alice_app);
    assert_eq!(
        text.matches("before-the-drop").count(),
        1,
        "the replay must not duplicate work the service already had: {text}"
    );
    assert!(
        alice_feed
            .kinds()
            .iter()
            .any(|kind| kind == "resynchronised"),
        "the user is told the session resynchronised: {:?}",
        alice_feed.kinds()
    );

    alice.disconnect();
    bob.disconnect();
}

/// A replayed disconnection longer than the service's submit cap is chunked,
/// and every operation of it lands.
///
/// This is P1-8 on the native path, in the shape the audit described: a tail
/// built from a long disconnection, sent as one oversized batch, refused —
/// after which `blocked` was never cleared and the session was over in every
/// way except what the pill said.
#[test]
fn a_replayed_disconnection_longer_than_the_cap_is_chunked_and_all_of_it_lands() {
    let root = TempRoot::new("chunked-replay");
    let service = Service::start_with(&root, |service| service.with_max_operations_per_submit(4));
    let cable = Cable::open(service.address);

    let alice_app = app_with_document();
    let alice_feed = Feed::default();
    let alice = Arc::new(NativeCollab::new(
        Arc::clone(&alice_app),
        alice_feed.emitter(),
    ));
    connect_to(&alice, cable.url(), "alice", ALICE_KEY, None);
    wait_for("alice to go live", || {
        alice_feed
            .last()
            .is_some_and(|status| status.phase == "live")
    });
    let document_uuid = alice_feed
        .last()
        .and_then(|status| status.session)
        .expect("a session")
        .document_uuid;
    service
        .service
        .set_grant(&document_uuid, "alice", "bob", Some(Role::Editor))
        .expect("granting bob edit access");
    let bob_app = app_with_document();
    let bob_feed = Feed::default();
    let bob = Arc::new(NativeCollab::new(Arc::clone(&bob_app), bob_feed.emitter()));
    connect_to(&bob, service.url(), "bob", BOB_KEY, Some(&document_uuid));
    wait_for("bob to go live", || {
        bob_feed.last().is_some_and(|status| status.phase == "live")
    });

    cable.cut();
    wait_for("alice's session to report the drop", || {
        alice_feed
            .last()
            .is_some_and(|status| status.phase == "reconnecting")
    });
    // Ten keystrokes against a cap of four: one batch the service would refuse.
    for index in 0..10 {
        type_text(&alice_app, &format!("{index}"));
    }
    assert_eq!(
        local_operation_count(&alice_app),
        10,
        "ten keystrokes while disconnected are ten unsent operations, which is more than the cap of four"
    );

    cable.mend();
    wait_for("alice to reconnect", || {
        alice_feed
            .last()
            .is_some_and(|status| status.phase == "live")
    });
    wait_for("every operation of the tail to be made durable", || {
        alice_feed
            .last()
            .is_some_and(|status| status.pending_operations == 0 && status.acknowledged_seq >= 10)
    });
    let status = alice_feed.last().expect("a status");
    assert!(
        status.can_submit,
        "a chunked replay must leave the session able to send, not blocked: {:?}",
        status.notice
    );
    assert!(
        !alice_feed
            .kinds()
            .iter()
            .any(|kind| kind.starts_with("refused-")),
        "nothing may be refused when the outbox chunks to the service's own cap: {:?}",
        alice_feed.kinds()
    );
    wait_for("bob to see all ten keystrokes", || {
        run_text(&bob_app).contains("0123456789")
    });
    wait_for("the two replicas to agree byte for byte", || {
        bytes_of(&alice_app) == bytes_of(&bob_app)
    });

    alice.disconnect();
    bob.disconnect();
}
