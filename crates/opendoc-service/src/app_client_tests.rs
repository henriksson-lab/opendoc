//! The loop, closed: a real `OpenDocApp` as a client of this service.
//!
//! Everything else in this crate proves the service is correct against its own
//! client. That client was written next to the server and shares its
//! assumptions. These tests replace it on one side with `opendoc_app::OpenDocApp`
//! — the same facade Tauri and the browser drive — authoring through
//! `dispatch_command` and ingesting through `apply_remote_operations`, over a
//! real TCP socket, while a second client edits the same text run concurrently.
//!
//! `opendoc-app` is a **dev**-dependency. ADR 0004 and ADR 0015 forbid the
//! service depending on the app facade, and that still holds for the shipped
//! crate; this is the test graph, which links nothing into the binary.

use crate::client::{DocumentSession, ServiceClient};
use crate::permission::Role;
use crate::protocol::ServerMessage;
use crate::server::{serve, RunningServer};
use crate::service::OpenDocService;
use crate::test_support::{register_default_subjects, TempRoot, ALICE_KEY, BOB_KEY};
use crate::{Clock, SERVICE_BRANCH, SERVICE_DOCUMENT_FORMAT};
use opendoc_app::{
    AppApiError, AppCommandResult, OpenDocApp, OpenDocAuthorizationDecision,
    OpenDocAuthorizationSource, OpenDocPresencePeer, OpenDocServiceRole, OpenDocServiceSession,
    OpenDocSyncBatchOutcome, OpenDocSyncRelayResult,
};
use opendoc_core::{
    Anchor, Block, BlockKind, Document, Inline, InsertPosition, StableId, Suggestion,
    SuggestionKind, SuggestionState,
};
use opendoc_merge::{Operation, OperationKind};
use opendoc_store::LocalObjectStore;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

/// Every wait is bounded: a service that simply stops sending must fail a
/// test, not hang the suite.
const EVENT_TIMEOUT: Duration = Duration::from_secs(20);

const BLOCK_ID: &str = "blk-appclient-001";
const RUN_ID: &str = "inl-appclient-001";
const BASE_TEXT: &str = "abcdefgh";

struct Harness {
    service: Arc<OpenDocService<LocalObjectStore>>,
    server: RunningServer,
}

impl Harness {
    async fn start(root: &TempRoot) -> Self {
        Self::start_with(root, |service| service).await
    }

    /// A service whose limits the test chooses. Every limit below has a
    /// deployment default; a test that wants to reach one without authoring
    /// hundreds of operations sets it small, which is also the honest shape of
    /// the feature — the cap is the service's, and the welcome carries it.
    async fn start_with(
        root: &TempRoot,
        configure: impl FnOnce(OpenDocService<LocalObjectStore>) -> OpenDocService<LocalObjectStore>,
    ) -> Self {
        let service = Arc::new(configure(OpenDocService::new(
            root.store(),
            Clock::system(),
        )));
        register_default_subjects(service.identity());
        let server = serve(Arc::clone(&service), "127.0.0.1:0".parse().unwrap())
            .await
            .expect("binding an ephemeral port");
        Self { service, server }
    }

    fn address(&self) -> SocketAddr {
        self.server.local_address()
    }

    async fn stop(self) {
        self.server.shutdown().await;
    }
}

/// The conversion a real transport performs: the welcome frame's server state,
/// turned into the DTO `opendoc-api` hands to `authorize_runtime_command`.
///
/// This is the whole of how a client comes to hold a permission decision. It
/// reads the socket and copies; it computes nothing, and there is no argument
/// anywhere in the app that could take its place.
fn service_session_of(session: &DocumentSession) -> OpenDocServiceSession {
    OpenDocServiceSession::new(
        session.subject(),
        session.actor().0.clone(),
        session.document_uuid(),
        role_of(session.role()),
    )
    .with_peers(session.peers().iter().map(presence_peer_of).collect())
}

fn role_of(role: Role) -> OpenDocServiceRole {
    OpenDocServiceRole::parse(role.as_str()).expect("the two crates agree on role names")
}

fn presence_peer_of(peer: &crate::protocol::PeerView) -> OpenDocPresencePeer {
    OpenDocPresencePeer {
        subject: peer.subject.clone(),
        actor: peer.actor.0.clone(),
        display_name: peer.display_name.clone(),
        role: role_of(peer.role),
        cursor_anchor: peer.cursor_anchor.clone(),
        selection_anchor: peer.selection_anchor.clone(),
        last_seen_ms: peer.last_seen_ms,
        connections: peer.connections as u32,
    }
}

/// `authorize_runtime_command` as a client would call it: mode and nothing
/// else. There is no grants argument to leave out.
fn authorization(app: &mut OpenDocApp, command: &str) -> OpenDocAuthorizationDecision {
    match app
        .dispatch_command(
            "authorize_runtime_command",
            serde_json::json!({
                "mode": "multi-user-service",
                "storageBackends": [],
                "signingEnabled": null,
                "commandName": command,
            }),
        )
        .expect("the command surface answers")
    {
        AppCommandResult::AuthorizationDecision(decision) => decision,
        other => panic!("expected an authorization decision, got {other:?}"),
    }
}

/// The preflight a transport runs before putting a batch on the wire.
fn preflight(app: &mut OpenDocApp, batch: &[Operation]) -> OpenDocSyncRelayResult {
    let operations = batch
        .iter()
        .map(|operation| {
            serde_json::json!({
                "actor": operation.id.actor.0,
                "seq": operation.id.seq,
                "kind": "rich-document",
            })
        })
        .collect::<Vec<_>>();
    match app
        .dispatch_command(
            "relay_runtime_sync",
            serde_json::json!({
                "mode": "multi-user-service",
                "storageBackends": [],
                "signingEnabled": null,
                "operations": operations,
            }),
        )
        .expect("the command surface answers")
    {
        AppCommandResult::SyncRelay(result) => result,
        other => panic!("expected a sync preflight, got {other:?}"),
    }
}

fn block_id() -> StableId {
    StableId::parse(BLOCK_ID).expect("block id")
}

fn run_id() -> StableId {
    StableId::parse(RUN_ID).expect("run id")
}

fn seed_block() -> OperationKind {
    OperationKind::InsertBlock {
        position: InsertPosition::Last,
        block: Block {
            id: block_id(),
            kind: BlockKind::Paragraph,
            properties: Default::default(),
            content: vec![Inline::Text {
                id: run_id(),
                text: BASE_TEXT.to_string(),
                marks: Vec::new(),
            }],
        },
    }
}

fn run_text(document: &Document) -> String {
    for block in &document.blocks {
        for inline in &block.content {
            if let Inline::Text { id, text, .. } = inline {
                if *id == run_id() {
                    return text.clone();
                }
            }
        }
    }
    String::new()
}

fn bytes_of(document: &Document) -> Vec<u8> {
    opendoc_format::encode_canonical_cbor(document).expect("canonical CBOR")
}

/// The document the app currently holds, as the canonical model.
fn app_document(app: &OpenDocApp) -> Document {
    app.source_document().clone()
}

async fn next_event(session: &mut DocumentSession) -> ServerMessage {
    tokio::time::timeout(EVENT_TIMEOUT, session.next_event())
        .await
        .expect("a server event within the timeout")
        .expect("reading a server event")
}

/// Drains one session to `commit_seq`, handing every commit the socket
/// delivers to the app — including the echo of the app's own work, which the
/// app must recognise as something it already has.
async fn drain_into_app(session: &mut DocumentSession, app: &mut OpenDocApp, commit_seq: u64) {
    while session.commit_seq() < commit_seq {
        match next_event(session).await {
            ServerMessage::Committed { operations, .. } => {
                app.apply_remote_operations(operations, None)
                    .expect("the app ingests the fanout");
            }
            ServerMessage::Rejected { code, message, .. } => {
                panic!("the service rejected the app's batch: {code}: {message}")
            }
            ServerMessage::Closed { code, message } => {
                panic!("connection closed while draining: {code}: {message}")
            }
            _ => {}
        }
    }
}

async fn drain_to(session: &mut DocumentSession, commit_seq: u64) {
    while session.commit_seq() < commit_seq {
        match next_event(session).await {
            ServerMessage::Rejected { code, message, .. } => {
                panic!("unexpected rejection while draining: {code}: {message}")
            }
            ServerMessage::Closed { code, message } => {
                panic!("connection closed while draining: {code}: {message}")
            }
            _ => {}
        }
    }
}

/// alice owns a seeded document; bob is an editor on it.
async fn seeded_document(harness: &Harness) -> (ServiceClient, ServiceClient, String) {
    let alice = ServiceClient::open_session(harness.address(), "alice", ALICE_KEY)
        .await
        .expect("alice signs in");
    let bob = ServiceClient::open_session(harness.address(), "bob", BOB_KEY)
        .await
        .expect("bob signs in");
    let document_uuid = alice
        .create_document("Shared")
        .await
        .expect("creating the document");
    alice
        .set_grant(&document_uuid, "bob", Some(Role::Editor))
        .await
        .expect("granting bob edit access");

    let mut session = alice
        .connect(&document_uuid, "Alice")
        .await
        .expect("connecting");
    let seed = session.author(seed_block());
    session
        .submit("seed", vec![seed])
        .await
        .expect("submitting the seed");
    drain_to(&mut session, 1).await;
    session.close().await;
    (alice, bob, document_uuid)
}

/// An `apply_editor_input` "insertText" at `offset` of the shared run, dispatched
/// through the ordinary command surface — the path a keystroke takes.
fn type_into_run(app: &mut OpenDocApp, offset: usize, text: &str) {
    let position = serde_json::json!({
        "block_id": BLOCK_ID,
        "inline_id": RUN_ID,
        "offset": offset,
    });
    app.dispatch_command(
        "apply_editor_input",
        serde_json::json!({
            "selection": { "anchor": position, "focus": position },
            "input_type": "insertText",
            "data": text,
        }),
    )
    .expect("the app types into the shared run");
}

// ---------------------------------------------------------------------------
// The test that matters.
// ---------------------------------------------------------------------------

/// Two clients on one document, one of them a real `OpenDocApp`, editing the
/// same text run concurrently over real sockets — and afterwards the app, the
/// other client, the app's own transport session and the server's materialised
/// document all encode to identical canonical CBOR.
///
/// The app authors through `dispatch_command`, which is the only way a gesture
/// becomes an operation, and ingests through `apply_remote_operations`, which
/// re-merges from the session's merge base rather than folding each commit
/// into the previous result. Those two halves meeting is the loop.
#[tokio::test]
async fn an_app_and_a_second_client_converge_byte_identically_over_the_transport() {
    let root = TempRoot::new("app-client-converge");
    let harness = Harness::start(&root).await;
    let (alice, bob, document_uuid) = seeded_document(&harness).await;

    let mut alice_session = alice
        .connect(&document_uuid, "Alice")
        .await
        .expect("alice connects");
    let mut bob_session = bob
        .connect(&document_uuid, "Bob")
        .await
        .expect("bob connects");

    // The app becomes alice's replica, from the welcome alone.
    let mut app = OpenDocApp::new_empty_document();
    app.join_collaboration_session(
        service_session_of(&alice_session),
        alice_session.base().clone(),
        alice_session.operations().to_vec(),
    )
    .expect("the app joins from the welcome");
    assert_eq!(
        bytes_of(&app_document(&app)),
        alice_session.document_bytes().expect("session bytes"),
        "the app must reach the service's document from the welcome alone"
    );
    assert_eq!(run_text(&app_document(&app)), BASE_TEXT);

    // A deterministic schedule, so a failure is reproducible. Both sides author
    // against what they can see *before* either round's commits land, which is
    // what makes the two batches genuinely concurrent.
    let mut seed = 0x5eed_a11c_e000_0001u64;
    let mut next = move || {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (seed >> 33) as usize
    };

    let mut acknowledged_seq = app
        .local_operations_after(0)
        .last()
        .map(|operation| operation.id.seq)
        .unwrap_or(0);
    let rounds = 8u64;
    let mut commit_seq = alice_session.commit_seq();
    for round in 0..rounds {
        let app_text = run_text(&app_document(&app));
        let bob_text = run_text(&bob_session.document().expect("bob materialises"));

        type_into_run(&mut app, next() % (app_text.chars().count() + 1), "A");
        let app_batch = app.local_operations_after(acknowledged_seq);
        assert!(
            !app_batch.is_empty(),
            "the editor command must have authored an operation"
        );
        assert!(
            app.local_operations_are_dense_after(acknowledged_seq),
            "the service refuses a gap in an actor's sequence"
        );
        acknowledged_seq = app_batch
            .last()
            .map(|operation| operation.id.seq)
            .expect("a non-empty batch");

        let bob_batch = if round % 3 == 1 && bob_text.chars().count() > 3 {
            let length = bob_text.chars().count();
            let start = next() % (length - 2);
            bob_session.author_batch(vec![OperationKind::DeleteText {
                inline_id: run_id(),
                start,
                end: start + 1,
            }])
        } else {
            let offset = next() % (bob_text.chars().count() + 1);
            bob_session.author_batch(vec![
                OperationKind::InsertText {
                    inline_id: run_id(),
                    offset,
                    text: format!("B{round}"),
                },
                OperationKind::SetDocumentTitle {
                    title: format!("bob round {round}"),
                },
            ])
        };

        alice_session
            .submit(&format!("app-{round}"), app_batch)
            .await
            .expect("the app's batch goes to the service");
        bob_session
            .submit(&format!("bob-{round}"), bob_batch)
            .await
            .expect("bob's batch goes to the service");

        commit_seq += 2;
        drain_into_app(&mut alice_session, &mut app, commit_seq).await;
        drain_to(&mut bob_session, commit_seq).await;

        // Checked every round, not only at the end. The app re-materialises
        // from the merge base on a local edit too, so a round's local typing
        // would repair an intake that had folded the commit into the previous
        // result — and an end-of-run assertion would then be one coin flip
        // away from proving nothing.
        assert_eq!(
            bytes_of(&app_document(&app)),
            alice_session.document_bytes().expect("alice materialises"),
            "round {round}: the app and its own transport session disagree\napp: {:?}\nsession: {:?}",
            run_text(&app_document(&app)),
            run_text(&alice_session.document().expect("alice materialises")),
        );
        assert_eq!(
            bytes_of(&app_document(&app)),
            bob_session.document_bytes().expect("bob materialises"),
            "round {round}: the app and bob's replica disagree",
        );
    }

    let app_bytes = bytes_of(&app_document(&app));
    let alice_bytes = alice_session.document_bytes().expect("alice materialises");
    let bob_bytes = bob_session.document_bytes().expect("bob materialises");
    let server_document = harness
        .service
        .document(&document_uuid)
        .expect("the document thread is running")
        .snapshot()
        .await
        .expect("the server materialises");
    let server_bytes = bytes_of(&server_document);

    assert_eq!(
        app_bytes,
        server_bytes,
        "the app and the server must be the same document\napp:    {:?}\nserver: {:?}",
        run_text(&app_document(&app)),
        run_text(&server_document),
    );
    assert_eq!(alice_bytes, server_bytes, "alice's replica must agree");
    assert_eq!(bob_bytes, server_bytes, "bob's replica must agree");

    // And the run must actually have been edited by both sides, or the
    // assertions above are comparing copies of the base.
    let text = run_text(&server_document);
    assert!(text.contains('A'), "the app's typing is missing: {text:?}");
    assert!(text.contains('B'), "bob's typing is missing: {text:?}");
    assert_ne!(text, BASE_TEXT);

    harness.stop().await;
}

/// A table row the app adds keeps the cells it typed when a peer deletes
/// **another** column at the same moment — over real sockets, so the cell to
/// column binding has to survive the protocol's encoding, not only the merge.
///
/// Until ADR 0019 a row payload named no column, so the merge could only place
/// its cells by index. Every replica agreed on the same wrong table, and the
/// only report was a generic `table-geometry-repaired` warning. This is that
/// case end to end: the app's row is authored against the grid it can see, a
/// peer removes a column from underneath it, and the content has to end up in
/// the column it was typed into on every replica and on the server.
#[tokio::test]
async fn a_row_the_app_adds_keeps_its_cells_when_a_peer_deletes_another_column() {
    let root = TempRoot::new("app-client-table");
    let harness = Harness::start(&root).await;
    let (alice, bob, document_uuid) = seeded_document(&harness).await;

    let mut alice_session = alice
        .connect(&document_uuid, "Alice")
        .await
        .expect("alice connects");
    let mut bob_session = bob
        .connect(&document_uuid, "Bob")
        .await
        .expect("bob connects");

    let mut app = OpenDocApp::new_empty_document();
    app.join_collaboration_session(
        service_session_of(&alice_session),
        alice_session.base().clone(),
        alice_session.operations().to_vec(),
    )
    .expect("the app joins from the welcome");

    let mut acknowledged_seq = app
        .local_operations_after(0)
        .last()
        .map(|operation| operation.id.seq)
        .unwrap_or(0);
    let mut commit_seq = alice_session.commit_seq();

    // Round one: the app puts a table in, and everyone sees it.
    app.dispatch_command(
        "insert_table_after",
        serde_json::json!({ "afterBlockId": BLOCK_ID }),
    )
    .expect("the app inserts a table");
    let batch = app.local_operations_after(acknowledged_seq);
    acknowledged_seq = batch.last().expect("a batch").id.seq;
    alice_session
        .submit("table", batch)
        .await
        .expect("the table goes to the service");
    commit_seq += 1;
    drain_into_app(&mut alice_session, &mut app, commit_seq).await;
    drain_to(&mut bob_session, commit_seq).await;

    let (table_block_id, columns) = table_ids(&app_document(&app));
    assert!(columns.len() >= 2, "the table needs a column to spare");

    // Round two, concurrent: the app adds two rows against the grid it can
    // see while bob removes the *second* column. The text the app types lands
    // in the first cell of each row, which is the column nobody touched.
    app.dispatch_command(
        "add_table_row",
        serde_json::json!({ "tableBlockId": table_block_id.to_string(), "text": "kept-one" }),
    )
    .expect("the app adds a row");
    app.dispatch_command(
        "add_table_row",
        serde_json::json!({ "tableBlockId": table_block_id.to_string(), "text": "kept-two" }),
    )
    .expect("the app adds another row");
    let app_batch = app.local_operations_after(acknowledged_seq);
    assert!(!app_batch.is_empty());
    let bob_batch = bob_session.author_batch(vec![OperationKind::DeleteTableColumn {
        table_block_id: table_block_id.clone(),
        column_id: columns[1].clone(),
    }]);

    alice_session
        .submit("rows", app_batch)
        .await
        .expect("the rows go to the service");
    bob_session
        .submit("column", bob_batch)
        .await
        .expect("the column delete goes to the service");
    commit_seq += 2;
    drain_into_app(&mut alice_session, &mut app, commit_seq).await;
    drain_to(&mut bob_session, commit_seq).await;

    let server_document = harness
        .service
        .document(&document_uuid)
        .expect("the document thread is running")
        .snapshot()
        .await
        .expect("the server materialises");
    let app_bytes = bytes_of(&app_document(&app));
    assert_eq!(
        app_bytes,
        bytes_of(&server_document),
        "the app and the server must be the same document"
    );
    assert_eq!(
        app_bytes,
        alice_session.document_bytes().expect("alice materialises"),
        "alice's replica must agree"
    );
    assert_eq!(
        app_bytes,
        bob_session.document_bytes().expect("bob materialises"),
        "bob's replica must agree"
    );

    // The oracle: the column bob deleted is gone, the column he did not is
    // intact, and the text the app typed is in it — in *both* of its rows.
    let (_, surviving) = table_ids(&server_document);
    assert_eq!(
        surviving,
        vec![columns[0].clone()],
        "exactly the column nobody deleted is left"
    );
    let texts = first_column_texts(&server_document, &table_block_id);
    assert!(
        texts.contains(&"kept-one".to_string()) && texts.contains(&"kept-two".to_string()),
        "both rows kept the cell they were typed into: {texts:?}"
    );

    harness.stop().await;
}

/// The first table in `document`: its block id and its column ids.
fn table_ids(document: &Document) -> (StableId, Vec<StableId>) {
    for block in &document.blocks {
        if let BlockKind::Table { columns, .. } = &block.kind {
            return (
                block.id.clone(),
                columns.iter().map(|column| column.id.clone()).collect(),
            );
        }
    }
    panic!("no table in the document");
}

/// The text of every row's first cell, in row order.
fn first_column_texts(document: &Document, table_block_id: &StableId) -> Vec<String> {
    for block in &document.blocks {
        if let BlockKind::Table { rows, .. } = &block.kind {
            if &block.id != table_block_id {
                continue;
            }
            return rows
                .iter()
                .map(|row| {
                    row.cells
                        .first()
                        .map(|cell| {
                            cell.blocks
                                .iter()
                                .flat_map(|block| block.content.iter())
                                .filter_map(|inline| match inline {
                                    Inline::Text { text, .. } => Some(text.as_str()),
                                    _ => None,
                                })
                                .collect::<String>()
                        })
                        .unwrap_or_default()
                })
                .collect();
        }
    }
    panic!("no table {table_block_id} in the document");
}

/// One command can author several operations, and the service checks each
/// one's Lamport timestamp against everything its vector clock names. A batch
/// whose later operations do not say they observed its earlier ones is refused
/// — so this is the test that the app's batch contexts are honest, not merely
/// ordered.
#[tokio::test]
async fn the_service_accepts_a_multi_operation_batch_the_app_authored() {
    let root = TempRoot::new("app-client-batch");
    let harness = Harness::start(&root).await;
    let (alice, _bob, document_uuid) = seeded_document(&harness).await;

    let mut alice_session = alice.connect(&document_uuid, "Alice").await.unwrap();
    let mut app = OpenDocApp::new_empty_document();
    app.join_collaboration_session(
        service_session_of(&alice_session),
        alice_session.base().clone(),
        alice_session.operations().to_vec(),
    )
    .unwrap();
    let acknowledged = app
        .local_operations_after(0)
        .last()
        .map(|operation| operation.id.seq)
        .unwrap_or(0);

    let blocks_before = app_document(&app).blocks.len();
    app.split_paragraph_at_inline(RUN_ID)
        .expect("splitting the paragraph");
    let batch = app.local_operations_after(acknowledged);
    assert!(batch.len() >= 2, "one command, several operations");
    assert!(app.local_operations_are_dense_after(acknowledged));

    alice_session.submit("batch", batch).await.unwrap();
    drain_into_app(&mut alice_session, &mut app, 2).await;

    let server_document = harness
        .service
        .document(&document_uuid)
        .unwrap()
        .snapshot()
        .await
        .unwrap();
    assert_eq!(
        bytes_of(&app_document(&app)),
        bytes_of(&server_document),
        "the batch reached the service and both sides agree"
    );
    // Relative to what was there, not an absolute count: the genesis document
    // this service creates carries the one paragraph an empty document has
    // (`OpenDocService::create_document`), so the absolute number is about
    // the genesis rather than about the split.
    assert_eq!(
        server_document.blocks.len(),
        blocks_before + 1,
        "the paragraph was split"
    );

    harness.stop().await;
}

/// A latecomer built only from the welcome must reach the same bytes as a
/// replica that watched every commit arrive — the welcome is the whole state.
#[tokio::test]
async fn an_app_joining_late_reaches_the_same_bytes_from_the_welcome_alone() {
    let root = TempRoot::new("app-client-late");
    let harness = Harness::start(&root).await;
    let (alice, bob, document_uuid) = seeded_document(&harness).await;

    let mut alice_session = alice.connect(&document_uuid, "Alice").await.unwrap();
    let mut bob_session = bob.connect(&document_uuid, "Bob").await.unwrap();

    let mut app = OpenDocApp::new_empty_document();
    app.join_collaboration_session(
        service_session_of(&alice_session),
        alice_session.base().clone(),
        alice_session.operations().to_vec(),
    )
    .unwrap();

    let bob_batch = bob_session.author_batch(vec![OperationKind::InsertText {
        inline_id: run_id(),
        offset: 4,
        text: "BOB".to_string(),
    }]);
    bob_session.submit("bob", bob_batch).await.unwrap();
    drain_into_app(&mut alice_session, &mut app, 2).await;
    drain_to(&mut bob_session, 2).await;

    let mut latecomer = OpenDocApp::new_empty_document();
    let late_session = bob.connect(&document_uuid, "Bob late").await.unwrap();
    latecomer
        .join_collaboration_session(
            service_session_of(&late_session),
            late_session.base().clone(),
            late_session.operations().to_vec(),
        )
        .unwrap();

    assert_eq!(
        bytes_of(&app_document(&latecomer)),
        bytes_of(&app_document(&app)),
        "a replica bootstrapped from the welcome must equal one that watched"
    );
    assert!(run_text(&app_document(&latecomer)).contains("BOB"));

    late_session.close().await;
    harness.stop().await;
}

/// A viewer's app can consume the fanout but its submissions are refused, and
/// the refusal leaves the service's state where it was.
#[tokio::test]
async fn a_viewers_app_receives_the_fanout_but_cannot_write() {
    let root = TempRoot::new("app-client-viewer");
    let harness = Harness::start(&root).await;
    let (alice, bob, document_uuid) = seeded_document(&harness).await;
    alice
        .set_grant(&document_uuid, "bob", Some(Role::Viewer))
        .await
        .unwrap();

    let mut alice_session = alice.connect(&document_uuid, "Alice").await.unwrap();
    let mut bob_session = bob.connect(&document_uuid, "Bob").await.unwrap();
    let mut viewer = OpenDocApp::new_empty_document();
    viewer
        .join_collaboration_session(
            service_session_of(&bob_session),
            bob_session.base().clone(),
            bob_session.operations().to_vec(),
        )
        .unwrap();

    let alice_batch = alice_session.author_batch(vec![OperationKind::InsertText {
        inline_id: run_id(),
        offset: 0,
        text: "ALICE".to_string(),
    }]);
    alice_session.submit("alice", alice_batch).await.unwrap();
    drain_into_app(&mut bob_session, &mut viewer, 2).await;

    assert!(
        run_text(&app_document(&viewer)).contains("ALICE"),
        "a viewer still reads the document"
    );

    let before = harness
        .service
        .document(&document_uuid)
        .unwrap()
        .status()
        .await
        .unwrap()
        .commit_seq;
    let refused = bob_session.author_batch(vec![OperationKind::InsertText {
        inline_id: run_id(),
        offset: 0,
        text: "NO".to_string(),
    }]);
    bob_session.submit("refused", refused).await.unwrap();
    loop {
        match next_event(&mut bob_session).await {
            ServerMessage::Rejected { code, .. } => {
                assert_eq!(code, "forbidden");
                break;
            }
            ServerMessage::Accepted { .. } => panic!("a viewer's write was accepted"),
            _ => {}
        }
    }
    let after = harness
        .service
        .document(&document_uuid)
        .unwrap()
        .status()
        .await
        .unwrap()
        .commit_seq;
    assert_eq!(before, after, "a refusal must not move the head");

    harness.stop().await;
}

// ---------------------------------------------------------------------------
// The storage format.
// ---------------------------------------------------------------------------

/// The constants the two crates have to agree on, checked rather than assumed.
///
/// `opendoc-app` restates them because it must not depend on this crate and
/// this crate must not depend on it (ADR 0015). A restated constant is only as
/// good as the test that compares it.
#[test]
fn the_app_reads_the_format_and_branch_this_service_writes() {
    assert_eq!(
        SERVICE_DOCUMENT_FORMAT,
        opendoc_app::SERVICE_DOCUMENT_FORMAT
    );
    assert_eq!(SERVICE_BRANCH, "main");
}

/// A repository this service wrote opens in `OpenDocApp` — the same manifest
/// chain, the same segment chaining, the same head — and materialises to the
/// document the service holds, operation history included.
#[tokio::test]
async fn a_repository_the_service_wrote_opens_in_the_app() {
    let root = TempRoot::new("app-client-repo");
    let harness = Harness::start(&root).await;
    let (alice, bob, document_uuid) = seeded_document(&harness).await;

    let mut alice_session = alice.connect(&document_uuid, "Alice").await.unwrap();
    let mut bob_session = bob.connect(&document_uuid, "Bob").await.unwrap();
    let alice_batch = alice_session.author_batch(vec![OperationKind::InsertText {
        inline_id: run_id(),
        offset: 2,
        text: "AL".to_string(),
    }]);
    alice_session.submit("alice", alice_batch).await.unwrap();
    let bob_batch = bob_session.author_batch(vec![
        OperationKind::InsertText {
            inline_id: run_id(),
            offset: 6,
            text: "BO".to_string(),
        },
        OperationKind::SetDocumentTitle {
            title: "Written by the service".to_string(),
        },
    ]);
    bob_session.submit("bob", bob_batch).await.unwrap();
    drain_to(&mut alice_session, 3).await;
    drain_to(&mut bob_session, 3).await;

    let server_document = harness
        .service
        .document(&document_uuid)
        .unwrap()
        .snapshot()
        .await
        .unwrap();

    // Everything the service acknowledged is durable, so the object store on
    // disk is a complete repository with no help from the running process.
    let mut app = OpenDocApp::new_empty_document();
    app.open_saved_projection(root.path(), &document_uuid)
        .expect("the app opens a repository the service wrote");

    assert_eq!(
        bytes_of(app.source_document()),
        bytes_of(&server_document),
        "the reopened document must be the service's document"
    );
    let operations = app.collaboration_operations();
    assert_eq!(
        operations.len(),
        4,
        "the seed, alice's insert and bob's two operations are all in the history"
    );
    assert!(
        !app.has_unsaved_changes(),
        "a freshly opened repository holds no unsaved work"
    );

    // And the app can carry it on: a local edit saves back into the same chain
    // and reopens, which is what "byte-compatible with a local save" buys.
    app.dispatch_command(
        "set_document_title",
        serde_json::json!({ "title": "Continued locally" }),
    )
    .expect("a local edit");
    app.save_to_local_repository(root.path())
        .expect("saving into the service's repository");
    let mut reopened = OpenDocApp::new_empty_document();
    reopened
        .open_saved_projection(root.path(), &document_uuid)
        .expect("reopening");
    assert_eq!(
        bytes_of(reopened.source_document()),
        bytes_of(app.source_document()),
    );
    assert_eq!(
        reopened.collaboration_operations().len(),
        5,
        "the local edit joined the service's history"
    );

    harness.stop().await;
}

/// An `OpenDocApp` refuses to place a remote operation when it has no merge
/// base to place it against, rather than re-anchoring it positionally against
/// whatever document happens to be open. ADR 0007 is why.
#[test]
fn an_app_without_a_session_refuses_remote_operations() {
    let mut app = OpenDocApp::new_empty_document();
    let operation: Operation = Operation::new(
        opendoc_merge::OperationId {
            actor: opendoc_merge::ActorId("actor-alice".to_string()),
            seq: 1,
        },
        OperationKind::SetDocumentTitle {
            title: "no".to_string(),
        },
    );
    let error = app
        .apply_remote_operations(vec![operation], None)
        .expect_err("there is no merge base");
    assert!(matches!(error, AppApiError::Conflict(_)), "{error:?}");
}

// ---------------------------------------------------------------------------
// Envelope identity and operation identity are different things.
// ---------------------------------------------------------------------------

/// The whole point of splitting the counters, proved against the real service
/// rather than against a unit test of the numbering.
///
/// `OpenDocApp` journals an envelope for everything it does, including things
/// that are not typed document operations: a blob upload, a spreadsheet edit,
/// an undo marker. While one counter numbered both, each of those consumed a
/// document-operation id, so an actor's operation sequence acquired a hole —
/// and this service refuses a non-dense sequence rather than storing one
/// (ADR 0015, "Sequence density"). A user who attached a file mid-session and
/// then kept typing was refused for it.
///
/// So: type, attach a blob, edit a sheet, undo, type again, and put the whole
/// stream on the wire. Every operation must be accepted and both replicas must
/// end up at the same bytes.
#[tokio::test]
async fn the_service_accepts_a_stream_interrupted_by_non_document_work() {
    let root = TempRoot::new("app-client-mixed-envelopes");
    let harness = Harness::start(&root).await;
    let (alice, _bob, document_uuid) = seeded_document(&harness).await;

    let mut alice_session = alice.connect(&document_uuid, "Alice").await.unwrap();
    let mut app = OpenDocApp::new_empty_document();
    app.join_collaboration_session(
        service_session_of(&alice_session),
        alice_session.base().clone(),
        alice_session.operations().to_vec(),
    )
    .expect("the app joins from the welcome");
    let acknowledged = app
        .local_operations_after(0)
        .last()
        .map(|operation| operation.id.seq)
        .unwrap_or(0);

    type_into_run(&mut app, 0, "A");
    // Three journal entries that carry no typed document operation. Each used
    // to burn an operation id.
    app.dispatch_command(
        "add_binary_blob",
        serde_json::json!({
            "name": "note.txt",
            "mediaType": "text/plain",
            "bytes": [104, 105],
        }),
    )
    .expect("a blob command");
    app.dispatch_command(
        "set_spreadsheet_cell",
        serde_json::json!({ "address": "A1", "value": "7" }),
    )
    .expect("a spreadsheet command");
    type_into_run(&mut app, 1, "B");
    app.dispatch_command("undo_current_edit", serde_json::json!({}))
        .expect("undo");
    type_into_run(&mut app, 1, "C");

    let batch = app.local_operations_after(acknowledged);
    assert!(batch.len() >= 2, "the session authored several operations");
    assert!(
        app.local_operations_are_dense_after(acknowledged),
        "the app's own numbering must be dense: {:?}",
        batch.iter().map(|op| op.id.seq).collect::<Vec<_>>()
    );

    // The batch also carries envelopes the journal numbered differently, which
    // is the point: envelope identity moved on while operation identity did
    // not, and neither collided.
    alice_session
        .submit("mixed", batch)
        .await
        .expect("the batch goes to the service");
    loop {
        match next_event(&mut alice_session).await {
            ServerMessage::Accepted { batch_id, .. } => {
                assert_eq!(batch_id, "mixed");
                break;
            }
            ServerMessage::Rejected { code, message, .. } => {
                panic!("the service refused a stream a real client produced: {code}: {message}")
            }
            ServerMessage::Closed { code, message } => {
                panic!("connection closed: {code}: {message}")
            }
            _ => {}
        }
    }
    drain_into_app(&mut alice_session, &mut app, 2).await;

    let server_document = harness
        .service
        .document(&document_uuid)
        .unwrap()
        .snapshot()
        .await
        .unwrap();
    assert_eq!(
        bytes_of(&app_document(&app)),
        bytes_of(&server_document),
        "the app and the service must hold the same document"
    );
    let text = run_text(&server_document);
    assert!(text.contains('A') && text.contains('C'), "{text:?}");

    harness.stop().await;
}

/// The same stream, one round further: after the service acknowledged a batch
/// that followed a blob and an undo, the *next* keystroke is still in
/// sequence. A watermark taken from the envelope numbering instead of the
/// operation numbering would skip ids here and the service would refuse.
#[tokio::test]
async fn a_keystroke_after_acknowledged_non_document_work_is_still_in_sequence() {
    let root = TempRoot::new("app-client-mixed-followup");
    let harness = Harness::start(&root).await;
    let (alice, _bob, document_uuid) = seeded_document(&harness).await;

    let mut alice_session = alice.connect(&document_uuid, "Alice").await.unwrap();
    let mut app = OpenDocApp::new_empty_document();
    app.join_collaboration_session(
        service_session_of(&alice_session),
        alice_session.base().clone(),
        alice_session.operations().to_vec(),
    )
    .unwrap();
    let mut acknowledged = app
        .local_operations_after(0)
        .last()
        .map(|operation| operation.id.seq)
        .unwrap_or(0);
    let mut commit_seq = alice_session.commit_seq();

    for round in 0..3u32 {
        app.dispatch_command(
            "add_binary_blob",
            serde_json::json!({
                "name": format!("note-{round}.txt"),
                "mediaType": "text/plain",
                "bytes": [104, 105, round as u8],
            }),
        )
        .expect("a blob command");
        type_into_run(&mut app, 0, "X");
        let batch = app.local_operations_after(acknowledged);
        assert!(
            app.local_operations_are_dense_after(acknowledged),
            "round {round}"
        );
        acknowledged = batch.last().expect("a non-empty batch").id.seq;
        alice_session
            .submit(&format!("round-{round}"), batch)
            .await
            .unwrap();
        commit_seq += 1;
        drain_into_app(&mut alice_session, &mut app, commit_seq).await;
        app.acknowledge_service_operations(acknowledged);
    }

    let server_document = harness
        .service
        .document(&document_uuid)
        .unwrap()
        .snapshot()
        .await
        .unwrap();
    assert_eq!(bytes_of(&app_document(&app)), bytes_of(&server_document));
    assert_eq!(run_text(&server_document).matches('X').count(), 3);

    harness.stop().await;
}

// ---------------------------------------------------------------------------
// Permissions are answers, not assertions.
// ---------------------------------------------------------------------------

/// `opendoc-api` restates this crate's role vocabulary because it must not
/// depend on it. A restated definition is only as good as the test that
/// compares it, so compare it.
#[test]
fn the_app_and_the_service_agree_on_roles_and_what_each_allows() {
    for role in [Role::Viewer, Role::Commenter, Role::Editor, Role::Owner] {
        let restated = OpenDocServiceRole::parse(role.as_str())
            .unwrap_or_else(|| panic!("opendoc-api does not know role {}", role.as_str()));
        assert_eq!(restated.as_str(), role.as_str());
        for action in [
            crate::permission::Action::Read,
            crate::permission::Action::Present,
            crate::permission::Action::Comment,
            crate::permission::Action::Write,
            crate::permission::Action::Share,
        ] {
            assert_eq!(
                restated.allows_action(action.as_str()),
                role.allows(action),
                "{} / {}",
                role.as_str(),
                action.as_str()
            );
        }
    }
    // And the ordering, which is what makes "at least commenter" expressible.
    assert!(OpenDocServiceRole::Viewer < OpenDocServiceRole::Commenter);
    assert!(OpenDocServiceRole::Commenter < OpenDocServiceRole::Editor);
    assert!(OpenDocServiceRole::Editor < OpenDocServiceRole::Owner);
}

/// A client's authorization decision is the service's answer, and the service
/// enforces the same answer on the wire.
///
/// There is no argument to `authorize_runtime_command` that could change this:
/// the decision is read from the session the welcome established. Bob is
/// demoted to viewer, reconnects, and both halves refuse together — the local
/// decision says the service attested `viewer`, and the service refuses the
/// submit.
#[tokio::test]
async fn a_clients_authorization_is_the_services_answer_not_its_own_claim() {
    let root = TempRoot::new("app-client-authorization");
    let harness = Harness::start(&root).await;
    let (alice, bob, document_uuid) = seeded_document(&harness).await;

    // As an editor, the service's answer permits writing.
    let editor_session = bob.connect(&document_uuid, "Bob").await.unwrap();
    let mut app = OpenDocApp::new_empty_document();
    app.join_collaboration_session(
        service_session_of(&editor_session),
        editor_session.base().clone(),
        editor_session.operations().to_vec(),
    )
    .unwrap();
    let decision = authorization(&mut app, "add_paragraph");
    assert!(decision.allowed, "{}", decision.reason);
    assert_eq!(
        decision.decided_by,
        OpenDocAuthorizationSource::ServiceAnswer
    );
    assert_eq!(decision.role, Some(OpenDocServiceRole::Editor));
    assert_eq!(decision.subject.as_deref(), Some("bob"));
    editor_session.close().await;

    // Demoted to viewer and reconnected, the same command is refused — and the
    // refusal names the service as the decider.
    alice
        .set_grant(&document_uuid, "bob", Some(Role::Viewer))
        .await
        .unwrap();
    let mut viewer_session = bob.connect(&document_uuid, "Bob").await.unwrap();
    let mut viewer = OpenDocApp::new_empty_document();
    viewer
        .join_collaboration_session(
            service_session_of(&viewer_session),
            viewer_session.base().clone(),
            viewer_session.operations().to_vec(),
        )
        .unwrap();
    let decision = authorization(&mut viewer, "add_paragraph");
    assert!(!decision.allowed);
    assert_eq!(
        decision.decided_by,
        OpenDocAuthorizationSource::ServiceAnswer
    );
    assert_eq!(decision.role, Some(OpenDocServiceRole::Viewer));
    assert!(authorization(&mut viewer, "get_document").allowed);

    // And the wire agrees: the local decision is a copy of the service's, not
    // a second opinion.
    let refused = viewer_session.author_batch(vec![OperationKind::InsertText {
        inline_id: run_id(),
        offset: 0,
        text: "NO".to_string(),
    }]);
    viewer_session.submit("refused", refused).await.unwrap();
    loop {
        match next_event(&mut viewer_session).await {
            ServerMessage::Rejected { code, .. } => {
                assert_eq!(code, "forbidden");
                break;
            }
            ServerMessage::Accepted { .. } => panic!("a viewer's write was accepted"),
            _ => {}
        }
    }

    // Without a session there is no answer at all, and no local fallback.
    let mut unconnected = OpenDocApp::new_empty_document();
    let decision = authorization(&mut unconnected, "get_document");
    assert!(!decision.allowed);
    assert_eq!(
        decision.decided_by,
        OpenDocAuthorizationSource::ServiceAnswerMissing
    );
    assert!(decision.subject.is_none());

    harness.stop().await;
}

/// The sync preflight answers the way the service does.
///
/// It exists so a transport learns about a batch it must not send without
/// discovering it as a rejection, which is only worth anything if the two
/// agree. A dense batch the preflight calls `accepted` is accepted; a batch
/// with a hole in it, which the preflight calls `refused`, is refused — with
/// the service's own out-of-sequence message.
#[tokio::test]
async fn the_sync_preflight_and_the_service_give_the_same_answer() {
    let root = TempRoot::new("app-client-preflight");
    let harness = Harness::start(&root).await;
    let (alice, _bob, document_uuid) = seeded_document(&harness).await;

    let mut alice_session = alice.connect(&document_uuid, "Alice").await.unwrap();
    let mut app = OpenDocApp::new_empty_document();
    app.join_collaboration_session(
        service_session_of(&alice_session),
        alice_session.base().clone(),
        alice_session.operations().to_vec(),
    )
    .unwrap();
    let acknowledged = app
        .local_operations_after(0)
        .last()
        .map(|operation| operation.id.seq)
        .unwrap_or(0);
    app.acknowledge_service_operations(acknowledged);

    type_into_run(&mut app, 0, "A");
    let batch = app.local_operations_after(acknowledged);
    assert_eq!(
        preflight(&mut app, &batch).outcome,
        OpenDocSyncBatchOutcome::Accepted
    );
    alice_session.submit("dense", batch.clone()).await.unwrap();
    loop {
        match next_event(&mut alice_session).await {
            ServerMessage::Accepted { .. } => break,
            ServerMessage::Rejected { code, message, .. } => {
                panic!("the preflight said accepted: {code}: {message}")
            }
            _ => {}
        }
    }
    drain_into_app(&mut alice_session, &mut app, 2).await;
    let acknowledged = batch.last().expect("a batch").id.seq;
    app.acknowledge_service_operations(acknowledged);

    // Resending exactly what the service already holds is a retry.
    assert_eq!(
        preflight(&mut app, &batch).outcome,
        OpenDocSyncBatchOutcome::Retried
    );

    // A hole in the sequence: the preflight refuses, and so does the service.
    let mut sparse = alice_session.author_batch(vec![OperationKind::InsertText {
        inline_id: run_id(),
        offset: 0,
        text: "Z".to_string(),
    }]);
    sparse[0].id.seq += 1;
    let result = preflight(&mut app, &sparse);
    assert_eq!(result.outcome, OpenDocSyncBatchOutcome::Refused);
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("out of sequence")),
        "{:?}",
        result.warnings
    );
    alice_session.submit("sparse", sparse).await.unwrap();
    loop {
        match next_event(&mut alice_session).await {
            ServerMessage::Rejected { message, .. } => {
                assert!(message.contains("out of sequence"), "{message}");
                break;
            }
            ServerMessage::Accepted { .. } => panic!("the service accepted a gap"),
            _ => {}
        }
    }

    harness.stop().await;
}

/// Presence carries the two fields `PeerView` always had and the DTO did not:
/// the actor a cursor's operations are authored under, and how many
/// connections one subject holds.
#[tokio::test]
async fn presence_reaches_the_app_with_the_actor_and_connection_count() {
    let root = TempRoot::new("app-client-presence");
    let harness = Harness::start(&root).await;
    let (alice, bob, document_uuid) = seeded_document(&harness).await;

    let mut alice_session = alice.connect(&document_uuid, "Alice").await.unwrap();
    let mut app = OpenDocApp::new_empty_document();
    app.join_collaboration_session(
        service_session_of(&alice_session),
        alice_session.base().clone(),
        alice_session.operations().to_vec(),
    )
    .unwrap();

    let bob_first = bob.connect(&document_uuid, "Bob").await.unwrap();
    let bob_second = bob.connect(&document_uuid, "Bob elsewhere").await.unwrap();
    let peers = loop {
        match next_event(&mut alice_session).await {
            ServerMessage::Presence { peers } => {
                if peers
                    .iter()
                    .any(|peer| peer.subject == "bob" && peer.connections == 2)
                {
                    break peers;
                }
            }
            ServerMessage::Closed { code, message } => {
                panic!("connection closed: {code}: {message}")
            }
            _ => {}
        }
    };
    app.apply_service_presence(peers.iter().map(presence_peer_of).collect());

    let session = app.service_session().expect("a session");
    let bob_peer = session
        .peers
        .iter()
        .find(|peer| peer.subject == "bob")
        .expect("bob is present");
    assert_eq!(
        bob_peer.actor.as_str(),
        bob_second.actor().0.as_str(),
        "a cursor must be mappable to the operations that produced it"
    );
    assert_eq!(
        bob_peer.connections, 2,
        "one person in two tabs is one peer"
    );
    assert_eq!(bob_peer.role, OpenDocServiceRole::Editor);

    bob_first.close().await;
    bob_second.close().await;
    harness.stop().await;
}

// ---------------------------------------------------------------------------
// Collaborative undo (ADR 0017)
// ---------------------------------------------------------------------------

/// Drains to `commit_seq` and reports which batch ids the service accepted.
///
/// The commits go into the app on the way past, as a real transport would do
/// with them; an `Accepted` frame is recorded rather than ignored, because for
/// an undo the question is not only "did it converge" but "was it allowed".
async fn drain_into_app_recording_accepts(
    session: &mut DocumentSession,
    app: &mut OpenDocApp,
    commit_seq: u64,
) -> Vec<String> {
    let mut accepted = Vec::new();
    while session.commit_seq() < commit_seq {
        match next_event(session).await {
            ServerMessage::Committed { operations, .. } => {
                app.apply_remote_operations(operations, None)
                    .expect("the app ingests the fanout");
            }
            ServerMessage::Accepted { batch_id, .. } => accepted.push(batch_id),
            ServerMessage::Rejected { code, message, .. } => {
                panic!("the service rejected the app's batch: {code}: {message}")
            }
            ServerMessage::Closed { code, message } => {
                panic!("connection closed while draining: {code}: {message}")
            }
            _ => {}
        }
    }
    accepted
}

/// **The test that matters.** Two actors on one document over a real socket: A
/// edits, B edits, A undoes — and B's work survives, with both sides and the
/// server converging byte-identically.
///
/// Three separate things are proved here and each of them was false before
/// ADR 0017:
///
/// 1. **B's edit survives A's undo.** The old undo restored a whole-state
///    snapshot taken before B's edit arrived, which deleted it locally while
///    the service still held it. That is why `apply_remote_operations` used to
///    drop the undo stack outright; it no longer does, and this is what makes
///    that safe.
/// 2. **The service accepts the undo.** The old undo rolled this actor's
///    operation counter back below the sequence the service had already
///    acknowledged, and re-submitting an acknowledged id is history rewriting,
///    which the service refuses. The inverse operations carry *new* ids above
///    the watermark, so there is nothing to refuse — asserted on the
///    `Accepted` frame, not inferred.
/// 3. **The undo is per-actor.** B's edit is the most recent thing that
///    happened to the document, and A's undo still reverses A's edit.
#[tokio::test]
async fn an_apps_undo_reverses_only_its_own_work_and_the_service_accepts_it() {
    let root = TempRoot::new("app-client-undo");
    let harness = Harness::start(&root).await;
    let (alice, bob, document_uuid) = seeded_document(&harness).await;

    let mut alice_session = alice
        .connect(&document_uuid, "Alice")
        .await
        .expect("alice connects");
    let mut bob_session = bob
        .connect(&document_uuid, "Bob")
        .await
        .expect("bob connects");

    let mut app = OpenDocApp::new_empty_document();
    app.join_collaboration_session(
        service_session_of(&alice_session),
        alice_session.base().clone(),
        alice_session.operations().to_vec(),
    )
    .expect("the app joins from the welcome");
    assert_eq!(run_text(&app_document(&app)), BASE_TEXT);

    let mut commit_seq = alice_session.commit_seq();
    let mut acknowledged = app
        .local_operations_after(0)
        .last()
        .map(|operation| operation.id.seq)
        .unwrap_or(0);

    // A types, and waits for the service to make it durable. The undo has to
    // survive *acknowledgement*: an unacknowledged operation could in
    // principle be withdrawn before it was sent, which would prove nothing.
    type_into_run(&mut app, 0, "A");
    let typed = app.local_operations_after(acknowledged);
    assert!(!typed.is_empty(), "the keystroke authored an operation");
    alice_session
        .submit("alice-typing", typed.clone())
        .await
        .expect("the keystroke goes to the service");
    commit_seq += 1;
    let accepted = drain_into_app_recording_accepts(&mut alice_session, &mut app, commit_seq).await;
    assert!(accepted.contains(&"alice-typing".to_string()));
    drain_to(&mut bob_session, commit_seq).await;
    acknowledged = typed
        .last()
        .map(|operation| operation.id.seq)
        .expect("a non-empty batch");
    app.acknowledge_service_operations(acknowledged);

    // B edits the same run, having seen A's keystroke.
    let bob_text = run_text(&bob_session.document().expect("bob materialises"));
    assert_eq!(bob_text, "Aabcdefgh");
    let bob_batch = bob_session.author_batch(vec![OperationKind::InsertText {
        inline_id: run_id(),
        offset: 4,
        text: "BOB".to_string(),
    }]);
    bob_session
        .submit("bob-typing", bob_batch)
        .await
        .expect("bob's batch goes to the service");
    commit_seq += 1;
    drain_into_app(&mut alice_session, &mut app, commit_seq).await;
    drain_to(&mut bob_session, commit_seq).await;
    assert_eq!(
        run_text(&app_document(&app)),
        "AabcBOBdefgh",
        "both edits are in the document before the undo"
    );

    // A undoes. The step is A's keystroke, which the service acknowledged two
    // commits ago.
    app.dispatch_command("undo_current_edit", serde_json::json!({}))
        .expect("undo is a command like any other");
    let undo_batch = app.local_operations_after(acknowledged);
    assert!(
        !undo_batch.is_empty(),
        "an undo has to author operations, not rewind the log"
    );
    assert!(
        undo_batch
            .iter()
            .all(|operation| operation.id.seq > acknowledged),
        "every id an undo mints must be above the acknowledged watermark: {:?}",
        undo_batch
            .iter()
            .map(|operation| operation.id.seq)
            .collect::<Vec<_>>()
    );
    assert!(
        app.local_operations_are_dense_after(acknowledged),
        "the service refuses a gap in an actor's sequence"
    );
    // The preflight and the service have to agree about it, too.
    let preflight = preflight(&mut app, &undo_batch);
    assert_eq!(preflight.outcome, OpenDocSyncBatchOutcome::Accepted);

    alice_session
        .submit("alice-undo", undo_batch)
        .await
        .expect("the undo goes on the wire");
    commit_seq += 1;
    let accepted = drain_into_app_recording_accepts(&mut alice_session, &mut app, commit_seq).await;
    assert!(
        accepted.contains(&"alice-undo".to_string()),
        "the service has to accept an undo as new history, not refuse it as a rewrite: {accepted:?}"
    );
    drain_to(&mut bob_session, commit_seq).await;

    let undone = run_text(&app_document(&app));
    assert_eq!(
        undone, "abcBOBdefgh",
        "the undo removes A's character and leaves B's word exactly where it was"
    );

    // Convergence, at the byte level, across every replica and the server.
    let app_bytes = bytes_of(&app_document(&app));
    assert_eq!(
        app_bytes,
        alice_session.document_bytes().expect("alice materialises"),
        "the app and its own transport session disagree"
    );
    assert_eq!(
        app_bytes,
        bob_session.document_bytes().expect("bob materialises"),
        "the app and bob's replica disagree after the undo"
    );
    let server_document = harness
        .service
        .document(&document_uuid)
        .unwrap()
        .snapshot()
        .await
        .unwrap();
    assert_eq!(
        app_bytes,
        bytes_of(&server_document),
        "the server and the app disagree after the undo"
    );

    // And a redo is ordinary history again: it puts A's character back without
    // disturbing B's.
    app.dispatch_command("redo_current_edit", serde_json::json!({}))
        .expect("redo");
    let redo_batch = app.local_operations_after(acknowledged);
    alice_session
        .submit("alice-redo", redo_batch)
        .await
        .expect("the redo goes on the wire");
    commit_seq += 1;
    let accepted = drain_into_app_recording_accepts(&mut alice_session, &mut app, commit_seq).await;
    assert!(accepted.contains(&"alice-redo".to_string()));
    drain_to(&mut bob_session, commit_seq).await;
    assert_eq!(run_text(&app_document(&app)), "AabcBOBdefgh");
    assert_eq!(
        bytes_of(&app_document(&app)),
        bob_session.document_bytes().expect("bob materialises"),
        "the replicas disagree after the redo"
    );

    alice_session.close().await;
    bob_session.close().await;
    harness.stop().await;
}

/// The gap ADR 0017 named, closed over a real socket: A deletes the **first**
/// block, B edits another block concurrently, A undoes — and the block comes
/// back *first*, not appended at the end.
///
/// Before `InsertBlock` carried an `InsertPosition` this was
/// `Inversion::Irreversible("insert-block-cannot-say-first")`, so the step fell
/// back to the whole-state snapshot, and inside a session that is refused
/// outright. The undo simply could not happen. What is asserted here is that it
/// happens, that it happens as *new history* the service accepts, that it puts
/// the block back where it was rather than somewhere plausible, and that B's
/// concurrent edit is untouched by it.
#[tokio::test]
async fn an_undo_over_the_service_puts_a_deleted_first_block_back_at_the_front() {
    let root = TempRoot::new("app-client-undo-first");
    let harness = Harness::start(&root).await;
    let (alice, bob, document_uuid) = seeded_document(&harness).await;

    let mut alice_session = alice
        .connect(&document_uuid, "Alice")
        .await
        .expect("alice connects");
    let mut bob_session = bob
        .connect(&document_uuid, "Bob")
        .await
        .expect("bob connects");

    let mut app = OpenDocApp::new_empty_document();
    app.join_collaboration_session(
        service_session_of(&alice_session),
        alice_session.base().clone(),
        alice_session.operations().to_vec(),
    )
    .expect("the app joins from the welcome");

    // The service's new document starts with a paragraph of its own, and
    // `seeded_document` appended the seed block after it — so there really is
    // a first block that is not the last one.
    let ids_before: Vec<String> = app
        .source_document()
        .blocks
        .iter()
        .map(|block| block.id.to_string())
        .collect();
    assert!(
        ids_before.len() > 1,
        "the fixture needs more than one block for \"first\" to mean anything: {ids_before:?}"
    );
    let first_block_id = ids_before[0].clone();
    assert_ne!(first_block_id, BLOCK_ID, "the seed block is not the first");

    let mut commit_seq = alice_session.commit_seq();
    let mut acknowledged = app
        .local_operations_after(0)
        .last()
        .map(|operation| operation.id.seq)
        .unwrap_or(0);

    // A deletes the first block, and waits for the service to make it durable:
    // the undo has to survive acknowledgement, or it would prove nothing about
    // history immutability.
    app.dispatch_command(
        "delete_block",
        serde_json::json!({ "blockId": first_block_id.clone() }),
    )
    .expect("deleting the first block");
    let delete_batch = app.local_operations_after(acknowledged);
    assert!(!delete_batch.is_empty(), "the delete authored an operation");
    alice_session
        .submit("alice-delete-first", delete_batch.clone())
        .await
        .expect("the delete goes to the service");
    commit_seq += 1;
    let accepted = drain_into_app_recording_accepts(&mut alice_session, &mut app, commit_seq).await;
    assert!(accepted.contains(&"alice-delete-first".to_string()));
    drain_to(&mut bob_session, commit_seq).await;
    acknowledged = delete_batch
        .last()
        .map(|operation| operation.id.seq)
        .expect("a non-empty batch");
    app.acknowledge_service_operations(acknowledged);
    assert_eq!(
        app.source_document()
            .blocks
            .iter()
            .map(|block| block.id.to_string())
            .collect::<Vec<_>>(),
        ids_before[1..].to_vec(),
        "the first block is gone and the rest kept their order"
    );

    // B, having seen the delete, edits a block that is still there.
    let bob_batch = bob_session.author_batch(vec![OperationKind::InsertText {
        inline_id: run_id(),
        offset: 0,
        text: "BOB".to_string(),
    }]);
    bob_session
        .submit("bob-typing", bob_batch)
        .await
        .expect("bob's batch goes to the service");
    commit_seq += 1;
    drain_into_app(&mut alice_session, &mut app, commit_seq).await;
    drain_to(&mut bob_session, commit_seq).await;
    assert_eq!(run_text(&app_document(&app)), "BOBabcdefgh");

    // A undoes the delete.
    app.dispatch_command("undo_current_edit", serde_json::json!({}))
        .expect("the undo is expressible, which is the whole point");
    let undo_batch = app.local_operations_after(acknowledged);
    assert!(
        !undo_batch.is_empty(),
        "an undo has to author operations, not rewind the log"
    );
    assert!(
        undo_batch
            .iter()
            .all(|operation| operation.id.seq > acknowledged),
        "every id an undo mints must be above the acknowledged watermark"
    );
    // The inverse says `First` — a position, not an anchor that could go
    // missing and degrade to an append.
    assert!(
        undo_batch.iter().any(|operation| matches!(
            &operation.kind,
            OperationKind::InsertBlock {
                position: InsertPosition::First,
                block,
            } if block.id.to_string() == first_block_id
        )),
        "the undo must re-insert the block at First: {:?}",
        undo_batch
            .iter()
            .map(|operation| &operation.kind)
            .collect::<Vec<_>>()
    );
    let preflight = preflight(&mut app, &undo_batch);
    assert_eq!(preflight.outcome, OpenDocSyncBatchOutcome::Accepted);

    alice_session
        .submit("alice-undo", undo_batch)
        .await
        .expect("the undo goes on the wire");
    commit_seq += 1;
    let accepted = drain_into_app_recording_accepts(&mut alice_session, &mut app, commit_seq).await;
    assert!(
        accepted.contains(&"alice-undo".to_string()),
        "the service has to accept the undo as new history: {accepted:?}"
    );
    drain_to(&mut bob_session, commit_seq).await;

    // The restored block is first again, and B's word is still in the block B
    // typed into.
    let document = app_document(&app);
    assert_eq!(
        document
            .blocks
            .iter()
            .map(|block| block.id.to_string())
            .collect::<Vec<_>>(),
        ids_before,
        "the undone delete put the block back at the front, not on the end"
    );
    assert_eq!(
        run_text(&document),
        "BOBabcdefgh",
        "B's concurrent edit survived A's undo"
    );

    // Convergence at the byte level across every replica and the server.
    let app_bytes = bytes_of(&document);
    assert_eq!(
        app_bytes,
        alice_session.document_bytes().expect("alice materialises"),
        "the app and its own transport session disagree"
    );
    assert_eq!(
        app_bytes,
        bob_session.document_bytes().expect("bob materialises"),
        "the app and bob's replica disagree after the undo"
    );
    let server_document = harness
        .service
        .document(&document_uuid)
        .unwrap()
        .snapshot()
        .await
        .unwrap();
    assert_eq!(
        app_bytes,
        bytes_of(&server_document),
        "the server and the app disagree after the undo"
    );

    alice_session.close().await;
    bob_session.close().await;
    harness.stop().await;
}

/// Undoing an insert a collaborator has since typed *inside*.
///
/// This is the case the merge machinery has to resolve rather than the undo:
/// the inverse names only the characters A inserted, so B's word survives in
/// the middle of the hole A's undo leaves, and the undo goes out as two
/// deletes rather than one.
#[tokio::test]
async fn undoing_an_insert_a_collaborator_typed_inside_keeps_the_collaborators_text() {
    let root = TempRoot::new("app-client-undo-inside");
    let harness = Harness::start(&root).await;
    let (alice, bob, document_uuid) = seeded_document(&harness).await;

    let mut alice_session = alice.connect(&document_uuid, "Alice").await.unwrap();
    let mut bob_session = bob.connect(&document_uuid, "Bob").await.unwrap();

    let mut app = OpenDocApp::new_empty_document();
    app.join_collaboration_session(
        service_session_of(&alice_session),
        alice_session.base().clone(),
        alice_session.operations().to_vec(),
    )
    .expect("the app joins from the welcome");

    let mut commit_seq = alice_session.commit_seq();
    let acknowledged = app
        .local_operations_after(0)
        .last()
        .map(|operation| operation.id.seq)
        .unwrap_or(0);

    // A types three characters as one coalesced gesture: one undo step.
    for (offset, letter) in [(0usize, "X"), (1, "Y"), (2, "Z")] {
        type_into_run(&mut app, offset, letter);
    }
    assert_eq!(run_text(&app_document(&app)), "XYZabcdefgh");
    let typed = app.local_operations_after(acknowledged);
    alice_session.submit("alice", typed.clone()).await.unwrap();
    commit_seq += 1;
    drain_into_app(&mut alice_session, &mut app, commit_seq).await;
    drain_to(&mut bob_session, commit_seq).await;
    let watermark = typed.last().map(|operation| operation.id.seq).unwrap();
    app.acknowledge_service_operations(watermark);

    // B types in the middle of what A typed.
    let bob_batch = bob_session.author_batch(vec![OperationKind::InsertText {
        inline_id: run_id(),
        offset: 2,
        text: "b".to_string(),
    }]);
    bob_session.submit("bob", bob_batch).await.unwrap();
    commit_seq += 1;
    drain_into_app(&mut alice_session, &mut app, commit_seq).await;
    drain_to(&mut bob_session, commit_seq).await;
    assert_eq!(run_text(&app_document(&app)), "XYbZabcdefgh");

    app.dispatch_command("undo_current_edit", serde_json::json!({}))
        .expect("undo");
    let undo_batch = app.local_operations_after(watermark);
    assert_eq!(
        undo_batch.len(),
        2,
        "A's characters sit in two visible ranges now, so the undo is two deletes: {:?}",
        undo_batch
            .iter()
            .map(|operation| &operation.kind)
            .collect::<Vec<_>>()
    );
    alice_session
        .submit("alice-undo", undo_batch)
        .await
        .unwrap();
    commit_seq += 1;
    let accepted = drain_into_app_recording_accepts(&mut alice_session, &mut app, commit_seq).await;
    assert!(accepted.contains(&"alice-undo".to_string()));
    drain_to(&mut bob_session, commit_seq).await;

    assert_eq!(
        run_text(&app_document(&app)),
        "babcdefgh",
        "B's character survives in place; every character A typed is gone"
    );
    assert_eq!(
        bytes_of(&app_document(&app)),
        bob_session.document_bytes().expect("bob materialises"),
    );
    alice_session.close().await;
    bob_session.close().await;
    harness.stop().await;
}

// ---------------------------------------------------------------------------
// The submit cap: the service's number, and what happens on either side of it.
// ---------------------------------------------------------------------------

/// The welcome names the cap, a batch over it is refused with the log
/// untouched, and the same work sent in chunks within the cap is taken and
/// reaches the second client byte for byte.
///
/// This is the whole of P1-8 from the service's side. The cap is configured
/// small here so the test authors ten operations rather than five hundred and
/// thirteen — which is only possible *because* the cap is a deployment
/// setting the welcome carries, rather than a constant three sides restate.
#[tokio::test]
async fn the_welcome_names_the_submit_cap_and_only_batches_within_it_are_taken() {
    let root = TempRoot::new("submit-cap");
    let harness =
        Harness::start_with(&root, |service| service.with_max_operations_per_submit(4)).await;
    let (alice, bob, document_uuid) = seeded_document(&harness).await;

    let mut alice_session = alice
        .connect(&document_uuid, "Alice")
        .await
        .expect("alice connects");
    assert_eq!(
        alice_session.max_operations_per_submit(),
        4,
        "a client that cannot read the cap out of the welcome cannot chunk to it"
    );
    let mut bob_session = bob
        .connect(&document_uuid, "Bob")
        .await
        .expect("bob connects");
    let before = alice_session.commit_seq();

    // One batch of ten against a cap of four: refused, and nothing is stored.
    let oversized = alice_session.author_batch(
        (0..10)
            .map(|index| OperationKind::InsertText {
                inline_id: run_id(),
                offset: index,
                text: "x".to_string(),
            })
            .collect(),
    );
    alice_session
        .submit("oversized", oversized.clone())
        .await
        .expect("the frame goes out");
    match next_submit_reply(&mut alice_session).await {
        ServerMessage::Rejected { code, message, .. } => {
            assert_eq!(code, "bad-request");
            assert!(
                message.contains("10") && message.contains("limit of 4"),
                "the refusal must name both numbers: {message}"
            );
        }
        other => panic!("an oversized batch must be refused, got {other:?}"),
    }
    assert_eq!(
        harness
            .service
            .document(&document_uuid)
            .expect("the document")
            .status()
            .await
            .expect("its status")
            .commit_seq,
        before,
        "a refused batch must leave the log exactly where it was"
    );

    // The same ten operations, chunked to the cap the welcome named. The ids
    // are the ones the refused batch carried, which is the point: chunking is
    // not re-authoring.
    for (index, chunk) in oversized.chunks(4).enumerate() {
        alice_session
            .submit(&format!("chunk-{index}"), chunk.to_vec())
            .await
            .expect("a chunk goes out");
    }
    let mut accepted_ids: Vec<u64> = Vec::new();
    let mut durable_through = 0u64;
    while accepted_ids.len() < 10 {
        match next_submit_reply(&mut alice_session).await {
            ServerMessage::Accepted {
                operation_ids,
                commit_seq,
                ..
            } => {
                accepted_ids.extend(operation_ids.iter().map(|id| id.seq));
                durable_through = durable_through.max(commit_seq);
            }
            other => panic!("a chunk within the cap must be accepted, got {other:?}"),
        }
    }
    accepted_ids.sort_unstable();
    assert_eq!(
        accepted_ids,
        (2..=11).collect::<Vec<u64>>(),
        "every operation of the refused batch is durable once it is chunked (the seed is #1)"
    );

    drain_to(&mut bob_session, durable_through).await;
    drain_to(&mut alice_session, durable_through).await;
    assert_eq!(
        bob_session.document_bytes().expect("bob's bytes"),
        alice_session.document_bytes().expect("alice's bytes"),
        "the second client must reach the same document from the chunks"
    );
    alice_session.close().await;
    bob_session.close().await;
    harness.stop().await;
}

// ---------------------------------------------------------------------------
// The Commenter role, which could not submit anything at all.
// ---------------------------------------------------------------------------

/// The next answer to a submit, skipping the presence and commit traffic a
/// live document produces on its own.
async fn next_submit_reply(session: &mut DocumentSession) -> ServerMessage {
    loop {
        match next_event(session).await {
            reply @ (ServerMessage::Accepted { .. } | ServerMessage::Rejected { .. }) => {
                return reply
            }
            ServerMessage::Closed { code, message } => {
                panic!("the connection closed while waiting for a reply: {code}: {message}")
            }
            _ => {}
        }
    }
}

fn comment_thread(id: &str, author: &str, body: &str) -> OperationKind {
    OperationKind::AddCommentThread {
        thread: opendoc_core::CommentThread {
            id: StableId::parse(id).expect("thread id"),
            anchor: opendoc_core::Anchor::Document,
            comments: vec![opendoc_core::Comment {
                id: StableId::parse(format!("{id}-c1")).expect("comment id"),
                author: author.to_string(),
                body: vec![Inline::Text {
                    id: StableId::parse(format!("{id}-b1")).expect("body id"),
                    text: body.to_string(),
                    marks: Vec::new(),
                }],
                created_at_ms: 1_700_000_000_000,
                deleted: false,
            }],
            state: opendoc_core::CommentThreadState::Open,
            resolved_by: None,
            resolved_at_ms: None,
            action_assignee: None,
            action_due_at_ms: None,
            action_completed_by: None,
            action_completed_at_ms: None,
            reactions: Vec::new(),
            deleted: false,
        },
    }
}

/// A `Commenter` can submit a comment and cannot submit a keystroke.
///
/// `submit` used to require `Action::Write` whatever the batch contained, so
/// `Action::Comment` was enforced for nothing and the Commenter role was a
/// Viewer with a different word on the pill. The permission a batch needs is
/// now decided from what is in it.
#[tokio::test]
async fn a_commenter_may_comment_and_may_not_type() {
    let root = TempRoot::new("commenter");
    let harness = Harness::start(&root).await;
    let (alice, bob, document_uuid) = seeded_document(&harness).await;
    alice
        .set_grant(&document_uuid, "bob", Some(Role::Commenter))
        .await
        .expect("bob becomes a commenter");

    let mut alice_session = alice
        .connect(&document_uuid, "Alice")
        .await
        .expect("alice connects");
    let mut bob_session = bob
        .connect(&document_uuid, "Bob")
        .await
        .expect("bob connects");
    assert_eq!(bob_session.role(), Role::Commenter);

    // The author is the *subject*, not the display name: the service binds a
    // comment's author field to whoever is authenticated. See
    // `a_comment_attributed_to_another_subject_is_refused`.
    let comment = bob_session.author(comment_thread("cmt-service-0001", "bob", "a note"));
    bob_session
        .submit("comment", vec![comment])
        .await
        .expect("the frame goes out");
    // The commit sequence comes out of the acknowledgement, not out of the
    // session: a client's own `Accepted` can arrive before the `Committed`
    // that carries the same work back to it.
    let durable_through = match next_submit_reply(&mut bob_session).await {
        ServerMessage::Accepted { commit_seq, .. } => commit_seq,
        other => panic!("a commenter must be able to comment, got {other:?}"),
    };
    drain_to(&mut alice_session, durable_through).await;
    assert_eq!(
        alice_session
            .document()
            .expect("alice's document")
            .comments
            .len(),
        1,
        "the comment reaches the other client like any other commit"
    );

    // Bob reads his own commit back before typing. Without this his next
    // operation would reuse sequence 1 and be refused as a history rewrite —
    // which would leave the refusal below proving nothing about permissions,
    // since it would be refused either way.
    drain_to(&mut bob_session, durable_through).await;

    // A keystroke from the same session is refused, and the document text is
    // exactly what it was.
    let typed = bob_session.author(OperationKind::InsertText {
        inline_id: run_id(),
        offset: 0,
        text: "not allowed".to_string(),
    });
    bob_session
        .submit("typed", vec![typed])
        .await
        .expect("the frame goes out");
    match next_submit_reply(&mut bob_session).await {
        ServerMessage::Rejected { code, message, .. } => {
            assert_eq!(code, "forbidden");
            assert!(message.contains("write"), "{message}");
        }
        other => panic!("a commenter must not be able to type, got {other:?}"),
    }
    let server_document = harness
        .service
        .document(&document_uuid)
        .expect("the document")
        .snapshot()
        .await
        .expect("its snapshot");
    assert_eq!(run_text(&server_document), BASE_TEXT);
    alice_session.close().await;
    bob_session.close().await;
    harness.stop().await;
}

/// Hiding another author's controls in the review pane is not sufficient:
/// callers can author an operation directly.  The service must keep a
/// commenter from rewriting or deleting words attributed to somebody else.
#[tokio::test]
async fn comment_mutations_are_bound_to_the_original_author() {
    let root = TempRoot::new("comment-mutation-author");
    let harness = Harness::start(&root).await;
    let (alice, bob, document_uuid) = seeded_document(&harness).await;
    alice
        .set_grant(&document_uuid, "bob", Some(Role::Commenter))
        .await
        .expect("bob becomes a commenter");

    let mut alice_session = alice
        .connect(&document_uuid, "Alice")
        .await
        .expect("alice connects");
    let mut bob_session = bob
        .connect(&document_uuid, "Bob")
        .await
        .expect("bob connects");
    let thread_id = "cmt-mutation-author";
    let comment_id = "cmt-mutation-author-c1";
    bob_session
        .submit(
            "add bob comment",
            vec![bob_session.author(comment_thread(thread_id, "bob", "Bob's note"))],
        )
        .await
        .expect("the frame goes out");
    let committed = match next_submit_reply(&mut bob_session).await {
        ServerMessage::Accepted { commit_seq, .. } => commit_seq,
        other => panic!("bob's own comment must be accepted, got {other:?}"),
    };
    drain_to(&mut alice_session, committed).await;
    drain_to(&mut bob_session, committed).await;

    let forged_update = alice_session.author(OperationKind::UpdateCommentBody {
        thread_id: StableId::parse(thread_id).unwrap(),
        comment_id: StableId::parse(comment_id).unwrap(),
        body: vec![Inline::text("Alice rewrote Bob")],
    });
    alice_session
        .submit("forge comment edit", vec![forged_update])
        .await
        .expect("the frame goes out");
    match next_submit_reply(&mut alice_session).await {
        ServerMessage::Rejected { code, message, .. } => {
            assert_eq!(code, "forbidden");
            assert!(
                message.contains("bob") && message.contains("alice"),
                "{message}"
            );
        }
        other => panic!("another author's edit must be refused, got {other:?}"),
    }

    // Proposal body text has the same durable authorship boundary.  It is
    // especially important because an accepted proposal becomes document
    // content, while its edit operation carries no author field itself.
    let suggestion_id = StableId::parse("suggestion-mutation-author").unwrap();
    bob_session
        .submit(
            "add bob suggestion",
            vec![bob_session.author(OperationKind::AddSuggestion {
                suggestion: Suggestion {
                    id: suggestion_id.clone(),
                    author: "bob".to_string(),
                    kind: SuggestionKind::Insert {
                        anchor: Anchor::Document,
                        content: vec![Inline::text("Bob's proposal")],
                    },
                    state: SuggestionState::Proposed,
                    provenance: Vec::new(),
                },
            })],
        )
        .await
        .expect("the frame goes out");
    let suggestion_committed = match next_submit_reply(&mut bob_session).await {
        ServerMessage::Accepted { commit_seq, .. } => commit_seq,
        other => panic!("bob's own suggestion must be accepted, got {other:?}"),
    };
    drain_to(&mut alice_session, suggestion_committed).await;
    alice_session
        .submit(
            "forge suggestion edit",
            vec![
                alice_session.author(OperationKind::UpdateSuggestionInsertContent {
                    suggestion_id,
                    content: vec![Inline::text("Alice rewrote Bob's proposal")],
                }),
            ],
        )
        .await
        .expect("the frame goes out");
    match next_submit_reply(&mut alice_session).await {
        ServerMessage::Rejected { code, message, .. } => {
            assert_eq!(code, "forbidden");
            assert!(
                message.contains("bob") && message.contains("alice"),
                "{message}"
            );
        }
        other => panic!("another author's suggestion edit must be refused, got {other:?}"),
    }

    // The original author remains able to make the same supported mutation.
    drain_to(&mut bob_session, suggestion_committed).await;
    bob_session
        .submit(
            "edit own comment",
            vec![bob_session.author(OperationKind::UpdateCommentBody {
                thread_id: StableId::parse(thread_id).unwrap(),
                comment_id: StableId::parse(comment_id).unwrap(),
                body: vec![Inline::text("Bob's corrected note")],
            })],
        )
        .await
        .expect("the frame goes out");
    let edited = match next_submit_reply(&mut bob_session).await {
        ServerMessage::Accepted { commit_seq, .. } => commit_seq,
        other => panic!("the original author's edit must be accepted, got {other:?}"),
    };
    drain_to(&mut alice_session, edited).await;
    let document = harness
        .service
        .document(&document_uuid)
        .expect("document")
        .snapshot()
        .await
        .expect("snapshot");
    match &document.comments[0].comments[0].body[..] {
        [Inline::Text { text, .. }] => assert_eq!(text, "Bob's corrected note"),
        other => panic!("expected the updated plain-text comment body, got {other:?}"),
    }

    alice_session.close().await;
    bob_session.close().await;
    harness.stop().await;
}

/// A comment a client attributes to somebody else is refused, and nothing is
/// written.
///
/// `Comment::author` is free text *inside* the operation payload, and the
/// payload is what gets merged into the document and signed. The actor binding
/// does not cover it: bob's session is honestly bob's, the operation id is
/// honestly bob's, and the comment still said alice wrote it. Rewriting the
/// field would make the server's copy of the operation differ from the
/// client's under one id, so the service refuses instead — and the same
/// comment under bob's own name goes through, which is what shows the refusal
/// was about the attribution rather than about comments.
#[tokio::test]
async fn a_comment_attributed_to_another_subject_is_refused() {
    let root = TempRoot::new("comment-author");
    let harness = Harness::start(&root).await;
    let (alice, bob, document_uuid) = seeded_document(&harness).await;

    let mut bob_session = bob
        .connect(&document_uuid, "Bob")
        .await
        .expect("bob connects");
    let commits_before = bob_session.commit_seq();

    let forged = bob_session.author(comment_thread("cmt-forged-0001", "alice", "alice said so"));
    bob_session
        .submit("forged", vec![forged.clone()])
        .await
        .expect("the frame goes out");
    match next_submit_reply(&mut bob_session).await {
        ServerMessage::Rejected { code, message, .. } => {
            assert_eq!(code, "forbidden");
            assert!(
                message.contains("alice") && message.contains("bob"),
                "the refusal must name the claimed author and the real subject: {message}"
            );
        }
        other => panic!("a forged comment author must be refused, got {other:?}"),
    }

    // Nothing was written: no commit, and no comment in the server's document.
    let server_document = harness
        .service
        .document(&document_uuid)
        .expect("the document")
        .snapshot()
        .await
        .expect("its snapshot");
    assert!(
        server_document.comments.is_empty(),
        "a refused comment must not reach the document"
    );

    // The same thread under bob's own name is accepted, at the same sequence
    // the forged one tried to use.
    let honest = bob_session.author(comment_thread("cmt-forged-0001", "bob", "alice said so"));
    assert_eq!(
        honest.id, forged.id,
        "the retry must reuse the sequence the refusal did not consume"
    );
    bob_session
        .submit("honest", vec![honest])
        .await
        .expect("the frame goes out");
    let durable_through = match next_submit_reply(&mut bob_session).await {
        ServerMessage::Accepted { commit_seq, .. } => commit_seq,
        other => panic!("a comment under the session's own subject must be taken, got {other:?}"),
    };
    assert!(durable_through > commits_before);
    let server_document = harness
        .service
        .document(&document_uuid)
        .expect("the document")
        .snapshot()
        .await
        .expect("its snapshot");
    assert_eq!(server_document.comments.len(), 1);
    assert_eq!(server_document.comments[0].comments[0].author, "bob");

    bob_session.close().await;
    drop(alice);
    harness.stop().await;
}

// ---------------------------------------------------------------------------
// Revocation, the quota, the registry and the anchor.
// ---------------------------------------------------------------------------

/// Revoking a grant drops that subject's sessions, which is the half of
/// ADR 0015's revocation claim that was false: `close_sessions_for_subject`
/// had no caller at all, so a revoked subject's bearer token went on working
/// until it expired.
#[tokio::test]
async fn revoking_a_grant_drops_the_subjects_sessions() {
    let root = TempRoot::new("revoke-sessions");
    let harness = Harness::start(&root).await;
    let (alice, bob, document_uuid) = seeded_document(&harness).await;
    assert!(
        bob.describe_document(&document_uuid).await.is_ok(),
        "bob is an editor and his token works"
    );

    alice
        .set_grant(&document_uuid, "bob", None)
        .await
        .expect("revoking bob");

    match bob.describe_document(&document_uuid).await {
        Err(crate::ServiceError::Unauthenticated(_)) => {}
        other => panic!("a revoked subject's token must stop working, got {other:?}"),
    }
    // And alice, who was not revoked, still has hers.
    assert!(alice.describe_document(&document_uuid).await.is_ok());
    harness.stop().await;
}

/// A subject cannot create more documents than the service allows, and the
/// count is durable rather than a number a restart forgets.
#[tokio::test]
async fn a_subject_cannot_create_more_documents_than_its_quota() {
    let root = TempRoot::new("quota");
    let harness =
        Harness::start_with(&root, |service| service.with_max_documents_per_subject(2)).await;
    let alice = ServiceClient::open_session(harness.address(), "alice", ALICE_KEY)
        .await
        .expect("alice signs in");

    assert!(alice.create_document("one").await.is_ok());
    assert!(alice.create_document("two").await.is_ok());
    match alice.create_document("three").await {
        Err(crate::ServiceError::Forbidden(message)) => {
            assert!(message.contains("limit"), "{message}");
        }
        other => panic!("the third must be refused, got {other:?}"),
    }
    assert_eq!(
        harness
            .service
            .documents_created_by("alice")
            .expect("the count"),
        2,
        "a refused creation must not be charged"
    );
    // Another subject has its own allowance: the quota is per subject, not a
    // global one the first caller can exhaust for everyone.
    let bob = ServiceClient::open_session(harness.address(), "bob", BOB_KEY)
        .await
        .expect("bob signs in");
    assert!(bob.create_document("bob's own").await.is_ok());
    harness.stop().await;
}

/// A service at its open-document limit refuses, and everything already open
/// keeps working.
///
/// This is the chain the security audit called the strongest: every document
/// opened an OS thread that is never stopped, and the spawn ended in
/// `.expect()` *inside* the registry mutex — so a process out of threads
/// poisoned the registry and answered 500 for every document, for ever. The
/// limit is what stops the process reaching that state; not panicking under
/// the lock is what stops it being permanent.
#[tokio::test]
async fn a_service_at_its_open_document_limit_refuses_and_keeps_working() {
    let root = TempRoot::new("document-limit");
    let harness = Harness::start_with(&root, |service| service.with_max_open_documents(1)).await;
    let alice = ServiceClient::open_session(harness.address(), "alice", ALICE_KEY)
        .await
        .expect("alice signs in");

    let first = alice.create_document("first").await.expect("the first");
    assert_eq!(harness.service.open_document_count().expect("count"), 1);
    assert!(
        alice.create_document("second").await.is_err(),
        "the second must be refused rather than taking a thread"
    );
    // The refusal has no side effects. A creation that fails for want of a
    // thread must not have charged the quota or written a genesis commit
    // first, or the caller is told it did not happen while an owned, durable
    // document says otherwise.
    assert_eq!(
        harness
            .service
            .documents_created_by("alice")
            .expect("the count"),
        1,
        "a creation refused for want of a thread must not be charged"
    );

    // The registry is not poisoned: the document that was already open still
    // answers, and so does a fresh session on it.
    assert!(harness.service.document(&first).is_ok());
    let session = alice
        .connect(&first, "Alice")
        .await
        .expect("alice connects");
    assert_eq!(session.document_uuid(), first);
    session.close().await;
    assert_eq!(harness.service.open_document_count().expect("count"), 1);
    harness.stop().await;
}

/// A service at its open-document limit accepts a new document once one of
/// the documents it is holding is released — over the real transport.
///
/// The limit above is a bound on how many documents this process serves at
/// once. It used to be a fuse: every document opened a thread that was never
/// stopped, so a process that reached the limit refused every new document
/// for the rest of its life, even with 1,024 documents nobody had touched
/// since last week. This is the whole of the difference, driven the way a
/// deployment reaches it — documents opened to the cap through sockets, then
/// released by closing them.
#[tokio::test]
async fn a_service_at_its_limit_accepts_a_new_document_once_a_session_closes() {
    let root = TempRoot::new("document-limit-lifts");
    // One document at a time, and a warm window short enough for a test to
    // outwait. Both are this deployment's numbers; neither is known to a
    // client.
    let harness = Harness::start_with(&root, |service| {
        service
            .with_max_open_documents(1)
            .with_document_idle_timeout_ms(50)
    })
    .await;
    let alice = ServiceClient::open_session(harness.address(), "alice", ALICE_KEY)
        .await
        .expect("alice signs in");

    let first = alice.create_document("first").await.expect("the first");
    let session = alice
        .connect(&first, "Alice")
        .await
        .expect("alice connects");

    // Well past the idle window, but a live session is holding the document.
    // Reclaiming it here would stop the thread that owns the branch head this
    // session is writing to, and the next open would start a second one.
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        alice.create_document("second").await.is_err(),
        "a document a live session is holding must not be reclaimed to make room"
    );

    session.close().await;

    // Nothing holds the first document now. The retry is for the socket
    // teardown, not for the reclamation: a service that never reclaims fails
    // this in the same place, having refused every attempt.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let second = loop {
        let refusal = match alice.create_document("second").await {
            Ok(document_uuid) => break document_uuid,
            Err(error) => error,
        };
        assert!(
            std::time::Instant::now() < deadline,
            "a document nothing holds must eventually be reclaimed so a new one can open; the service kept refusing: {refusal}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    };
    assert_ne!(second, first);
    assert_eq!(
        harness.service.open_document_count().expect("the count"),
        1,
        "the process still owns exactly one document thread"
    );

    // And the document that lost its thread is not lost: it opens again, from
    // storage, once the new one falls idle in its turn.
    let session = alice
        .connect(&second, "Alice")
        .await
        .expect("alice connects to the second");
    assert_eq!(session.document_uuid(), second);
    session.close().await;
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(session) = alice.connect(&first, "Alice").await {
            assert_eq!(session.document_uuid(), first);
            session.close().await;
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the first document must open again once the second falls idle"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    harness.stop().await;
}

/// Both remote selection endpoints are cut to a bounded length before relay.
///
/// An anchor is an opaque string the service never resolves, and it is cloned
/// into a presence frame per connection on every presence change, into
/// outbound queues this service does not bound. Unbounded, a read-only Viewer
/// could amplify one large string into one per peer per keystroke.
#[tokio::test]
async fn presence_selection_endpoints_are_capped_before_the_service_relays_them() {
    let root = TempRoot::new("anchor-cap");
    let harness = Harness::start(&root).await;
    let (alice, bob, document_uuid) = seeded_document(&harness).await;
    alice
        .set_grant(&document_uuid, "bob", Some(Role::Viewer))
        .await
        .expect("bob becomes a viewer");

    let mut alice_session = alice
        .connect(&document_uuid, "Alice")
        .await
        .expect("alice connects");
    let mut bob_session = bob
        .connect(&document_uuid, "Bob")
        .await
        .expect("bob connects");

    let huge = "z".repeat(100_000);
    bob_session
        .announce_presence(Some("Bob"), Some(&huge), Some(&huge))
        .await
        .expect("the frame goes out");

    // Alice is told where bob's caret is, and what she is told is bounded.
    let (cursor, selection) = loop {
        if let ServerMessage::Presence { peers } = next_event(&mut alice_session).await {
            if let Some(peer) = peers.iter().find(|peer| peer.subject == "bob") {
                if let (Some(cursor), Some(selection)) =
                    (peer.cursor_anchor.clone(), peer.selection_anchor.clone())
                {
                    break (cursor, selection);
                }
            }
        }
    };
    assert_eq!(
        cursor.chars().count(),
        crate::document::MAX_CURSOR_ANCHOR_CHARS,
        "the relayed cursor must be cut to the cap, not relayed whole"
    );
    assert_eq!(
        selection.chars().count(),
        crate::document::MAX_CURSOR_ANCHOR_CHARS,
        "the relayed selection endpoint must be cut to the cap, not relayed whole"
    );
    alice_session.close().await;
    bob_session.close().await;
    harness.stop().await;
}

/// Re-announcing an unchanged presence fans nothing out.
///
/// The browser pump sends presence only when it changes, but the service must
/// not depend on a client being polite: a presence frame is cloned once per
/// connection into unbounded queues, so an unchanged one is pure
/// amplification.
#[tokio::test]
async fn an_unchanged_presence_announcement_is_not_fanned_out() {
    let root = TempRoot::new("presence-noop");
    let harness = Harness::start(&root).await;
    let (alice, bob, document_uuid) = seeded_document(&harness).await;

    let mut alice_session = alice
        .connect(&document_uuid, "Alice")
        .await
        .expect("alice connects");
    let mut bob_session = bob
        .connect(&document_uuid, "Bob")
        .await
        .expect("bob connects");
    // Bob joining is itself a presence change alice is told about.
    loop {
        if let ServerMessage::Presence { .. } = next_event(&mut alice_session).await {
            break;
        }
    }

    bob_session
        .announce_presence(Some("Bob"), Some("blk-1:inl-1:3"), None)
        .await
        .expect("a real change");
    loop {
        if let ServerMessage::Presence { peers } = next_event(&mut alice_session).await {
            if peers
                .iter()
                .any(|peer| peer.cursor_anchor.as_deref() == Some("blk-1:inl-1:3"))
            {
                break;
            }
        }
    }

    // The same announcement again, then a submit. If the repeat had been fanned
    // out, alice would read a presence frame before the commit.
    bob_session
        .announce_presence(Some("Bob"), Some("blk-1:inl-1:3"), None)
        .await
        .expect("the same values again");
    let typed = bob_session.author(OperationKind::InsertText {
        inline_id: run_id(),
        offset: 0,
        text: "Z".to_string(),
    });
    bob_session
        .submit("after-the-repeat", vec![typed])
        .await
        .expect("the frame goes out");
    match next_event(&mut alice_session).await {
        ServerMessage::Committed { .. } => {}
        other => panic!("an unchanged presence must not be fanned out, got {other:?}"),
    }
    alice_session.close().await;
    bob_session.close().await;
    harness.stop().await;
}

/// A comment a real `OpenDocApp` authored, over a real socket.
///
/// The author-binding check above is enforced against `Comment::author`, which
/// is *not* the actor and not the subject: it is whatever the `add_comment`
/// command's `author` argument carried. Every other test in this file composes
/// that payload by hand, so none of them can say whether the app a user
/// actually drives produces a comment this service will take. That is the one
/// question the check's value depends on, and this is the test that asks it.
#[tokio::test]
async fn a_comment_a_real_app_authored_reaches_the_service() {
    let root = TempRoot::new("app-comment");
    let harness = Harness::start(&root).await;
    let (_alice, bob, document_uuid) = seeded_document(&harness).await;

    let mut session = bob
        .connect(&document_uuid, "Bob Brown")
        .await
        .expect("bob connects");
    let mut app = OpenDocApp::new_empty_document();
    app.join_collaboration_session(
        service_session_of(&session),
        session.base().clone(),
        session.operations().to_vec(),
    )
    .expect("the app adopts the service's document");

    // The author the app is given is the subject the service authenticated,
    // which is what the binding requires. `opendoc-app` now enforces that
    // itself — `annotation_author` ignores the command argument whenever a
    // session is joined — so this case and the one below differ only in what
    // the caller *asked* for, never in what is written.
    app.dispatch_command(
        "add_comment",
        serde_json::json!({ "author": session.subject(), "body": "a note from the app" }),
    )
    .expect("the app authors a comment");

    let batch = app.local_operations_after(0);
    assert_eq!(batch.len(), 1, "one comment is one operation");
    session
        .submit("app-comment", batch)
        .await
        .expect("the frame goes out");
    match next_submit_reply(&mut session).await {
        ServerMessage::Accepted { .. } => {}
        other => panic!(
            "a comment a real app authored under its own subject must be taken, got {other:?}"
        ),
    }

    // Now the case that used to be refused. `state.authorName` is
    // `runtime.subject ?? "Local user"` — `"Local user"` in a browser whose
    // host injected no runtime config — so before the binding existed, every
    // comment made inside a session arrived under a display name and was
    // thrown out, at a cost of three resynchronisation attempts each.
    //
    // The app can no longer produce a refusable comment **by construction**:
    // hand it the wrong name and it writes the right one. So this asserts the
    // inverse of what it once did, which is the stronger claim — not "the
    // service catches the app", but "the app cannot get it wrong".
    //
    // The refusal path still has to be proven, and it is, against a client
    // that is *not* an `OpenDocApp`:
    // `a_comment_attributed_to_another_subject_is_refused` builds the forged
    // frame by hand. The two are deliberately paired — if this assertion is
    // ever "restored" to expect a rejection, the binding has been weakened to
    // make it pass, and that test is the one that still holds the line.
    app.dispatch_command(
        "add_comment",
        serde_json::json!({ "author": "Bob Brown", "body": "a note under a display name" }),
    )
    .expect("the app authors a second comment");
    let next = app.local_operations_after(1);
    assert_eq!(next.len(), 1);
    session
        .submit("display-name-comment", next)
        .await
        .expect("the frame goes out");
    match next_submit_reply(&mut session).await {
        ServerMessage::Accepted { .. } => {}
        other => {
            panic!("the app bound the author to its subject, so this must be taken, got {other:?}")
        }
    }

    // And what landed says so. Read from the service's own document, not from
    // the app's: the binding is only worth anything if it survives the wire.
    let server_document = harness
        .service
        .document(&document_uuid)
        .expect("the document")
        .snapshot()
        .await
        .expect("its snapshot");
    // One comment per thread here, but flatten rather than assume it: the
    // claim is about every author that reached the document, not the first.
    let authors: Vec<&str> = server_document
        .comments
        .iter()
        .flat_map(|thread| thread.comments.iter())
        .map(|comment| comment.author.as_str())
        .collect();
    assert_eq!(
        authors.len(),
        2,
        "both comments must have reached the document: {authors:?}"
    );
    assert!(
        authors.iter().all(|author| *author == session.subject()),
        "a comment reached the document under a name the caller chose: {authors:?}"
    );
    assert!(
        !authors.contains(&"Bob Brown"),
        "the display name was written through instead of the subject: {authors:?}"
    );
    session.close().await;
    harness.stop().await;
}
