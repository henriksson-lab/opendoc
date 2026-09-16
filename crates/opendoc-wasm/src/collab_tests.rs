//! The browser collaboration driver, on the host.
//!
//! Everything the driver does is a function of the frames it was handed and
//! the `OpenDocApp` it was handed them for, so all of it is testable with
//! `cargo test` and no browser at all. What a browser adds — a socket, a
//! retry timer, DOM — is `apps/desktop/src/collab.ts`, and `npm run e2e`
//! drives that against a real service in real Chrome.
//!
//! The first test here is the other half of the wire tripwire described in
//! `crates/opendoc-service/src/wire_fixture_tests.rs`: these restated types
//! must read the bytes that crate's real types write.

use crate::collab::{
    ClientFrame, CollabDriver, Notice, Phase, ServerFrame, WirePeer, PROTOCOL_VERSION,
};
use opendoc_app::{
    base64_encode, Document, OpenDocApp, OpenDocServiceRole, Operation, OperationKind,
};
use opendoc_core::{Block, BlockKind, Inline, StableId};

const FIXTURE: &str = include_str!("../../opendoc-service/wire/protocol-frames.json");

const BLOCK_ID: &str = "blk-collab-0001";
const RUN_ID: &str = "inl-collab-0001";
const BASE_TEXT: &str = "abcdefgh";
const DOCUMENT_UUID: &str = "doc-collab-0001";

// ---- the tripwire --------------------------------------------------------

#[test]
fn the_restated_frames_read_what_the_service_crate_writes() {
    let fixture: serde_json::Value = serde_json::from_str(FIXTURE).expect("the fixture is JSON");
    assert_eq!(
        fixture["protocol_version"].as_u64(),
        Some(u64::from(PROTOCOL_VERSION)),
        "this client restates the protocol version; the service's fixture says otherwise"
    );

    let server = fixture["server"].as_array().expect("server frames");
    let parsed: Vec<ServerFrame> = server
        .iter()
        .map(|frame| {
            serde_json::from_value(frame.clone()).unwrap_or_else(|error| {
                panic!("this client cannot read a frame the service sends: {error}\n{frame:#}")
            })
        })
        .collect();
    assert!(
        !parsed.contains(&ServerFrame::Unrecognised),
        "every frame in the fixture is a frame this client must recognise, not fall through on"
    );

    // Field by field on the welcome, because a tagged enum will happily parse
    // a frame whose *fields* all defaulted away.
    match &parsed[0] {
        ServerFrame::Welcome {
            protocol_version,
            document_uuid,
            subject,
            actor,
            role,
            commit_seq,
            base_document,
            operations,
            peers,
            max_operations_per_submit,
        } => {
            assert_eq!(*protocol_version, PROTOCOL_VERSION);
            // The cap this client chunks its outbox to is read out of the
            // welcome, never restated here, so the fixture is where the two
            // sides are compared. `default_max_operations_per_submit` is the
            // service crate's own constant, written into the fixture beside
            // the frames.
            assert_eq!(
                *max_operations_per_submit as u64,
                fixture["default_max_operations_per_submit"]
                    .as_u64()
                    .expect("the fixture names the service's default submit cap"),
                "this client would chunk to a different number than the service accepts"
            );
            assert_eq!(document_uuid, "11111111-2222-3333-4444-555555555555");
            assert_eq!(subject, "alice");
            assert_eq!(actor, "actor-alice");
            assert_eq!(*role, OpenDocServiceRole::Editor);
            assert_eq!(*commit_seq, 2);
            assert_eq!(operations.len(), 2);
            assert_eq!(operations[1].id.seq, 2);
            assert_eq!(peers.len(), 2);
            assert_eq!(peers[0].subject, "alice");
            assert_eq!(peers[0].actor, "actor-alice");
            assert_eq!(peers[0].display_name, "alice");
            assert_eq!(peers[0].role, OpenDocServiceRole::Editor);
            assert_eq!(peers[0].cursor_anchor.as_deref(), Some("blk-wire-0001:3"));
            assert_eq!(peers[0].last_seen_ms, 1_700_000_000_000);
            assert_eq!(peers[0].connections, 1);
            assert_eq!(peers[1].cursor_anchor, None);
            // The base is base64 of canonical CBOR, and it has to decode to a
            // document — the whole reconnect story rests on it.
            let base = crate::collab::decode_base_document_for_test(base_document)
                .expect("the fixture's base document decodes");
            assert_eq!(base.title, "Wire fixture");
        }
        other => panic!("the first server frame must be a welcome, got {other:?}"),
    }
    match &parsed[1] {
        ServerFrame::Accepted {
            batch_id,
            commit_seq,
            operation_ids,
        } => {
            assert_eq!(batch_id, "batch-7");
            assert_eq!(*commit_seq, 3);
            assert_eq!(operation_ids.len(), 2);
            assert_eq!(operation_ids[1].actor, "actor-alice");
            assert_eq!(operation_ids[1].seq, 4);
        }
        other => panic!("expected accepted, got {other:?}"),
    }
    match &parsed[2] {
        ServerFrame::Rejected {
            batch_id,
            code,
            message,
        } => {
            assert_eq!(batch_id, "batch-8");
            assert_eq!(code, "forbidden");
            assert_eq!(message, "viewer may not write");
        }
        other => panic!("expected rejected, got {other:?}"),
    }
    match &parsed[3] {
        ServerFrame::Committed {
            commit_seq,
            subject,
            actor,
            operations,
        } => {
            assert_eq!(*commit_seq, 4);
            assert_eq!(subject, "bob");
            assert_eq!(actor, "actor-bob");
            assert_eq!(operations.len(), 1);
        }
        other => panic!("expected committed, got {other:?}"),
    }
    match &parsed[4] {
        ServerFrame::Presence { peers } => {
            assert_eq!(peers.len(), 1);
            assert_eq!(peers[0].role, OpenDocServiceRole::Commenter);
        }
        other => panic!("expected presence, got {other:?}"),
    }
    match &parsed[5] {
        ServerFrame::Closed { code, message } => {
            assert_eq!(code, "forbidden");
            assert_eq!(message, "read access was revoked");
        }
        other => panic!("expected closed, got {other:?}"),
    }
    assert_eq!(parsed[6], ServerFrame::Pong);

    // And the other direction: what this client *sends* must be what the
    // service parses. Compared as JSON values, so key order is not the claim.
    let client = fixture["client"].as_array().expect("client frames");
    let submit = ClientFrame::Submit {
        batch_id: "batch-9".to_string(),
        operations: vec![
            serde_json::from_value(client[0]["operations"][0].clone()).expect("an operation")
        ],
    };
    assert_eq!(
        serde_json::to_value(&submit).expect("serializing a submit"),
        client[0],
        "a submit frame from this client is not the one the service reads"
    );
    let presence = ClientFrame::Presence {
        display_name: Some("Alice".to_string()),
        cursor_anchor: Some("blk-wire-0001:5".to_string()),
        selection_anchor: Some("blk-wire-0001:2".to_string()),
    };
    assert_eq!(
        serde_json::to_value(&presence).expect("serializing presence"),
        client[1],
        "a presence frame from this client is not the one the service reads"
    );
    assert_eq!(
        serde_json::to_value(ClientFrame::Ping).expect("serializing ping"),
        client[2]
    );
}

#[test]
fn a_frame_type_this_client_does_not_know_is_ignored_rather_than_fatal() {
    let frame: ServerFrame =
        serde_json::from_str(r#"{"type":"something-new","detail":7}"#).expect("parses");
    assert_eq!(frame, ServerFrame::Unrecognised);
}

// ---- driving the session -------------------------------------------------

fn base_document() -> Document {
    let mut document = Document::new("Shared");
    document.blocks.push(Block {
        id: StableId::parse(BLOCK_ID).expect("block id"),
        kind: BlockKind::Paragraph,
        properties: Default::default(),
        content: vec![Inline::Text {
            id: StableId::parse(RUN_ID).expect("run id"),
            text: BASE_TEXT.to_string(),
            marks: Vec::new(),
        }],
    });
    document
}

fn welcome_frame(commit_seq: u64, operations: &[Operation], role: &str) -> String {
    welcome_frame_as(commit_seq, operations, role, "actor-alice", 512)
}

/// A welcome with the two things the tests below vary: which actor the service
/// bound this subject to, and the submit cap it announces.
fn welcome_frame_as(
    commit_seq: u64,
    operations: &[Operation],
    role: &str,
    actor: &str,
    max_operations_per_submit: usize,
) -> String {
    let encoded = base64_encode(
        &opendoc_format::encode_canonical_cbor(&base_document()).expect("canonical CBOR"),
    );
    serde_json::json!({
        "type": "welcome",
        "protocol_version": PROTOCOL_VERSION,
        "document_uuid": DOCUMENT_UUID,
        "subject": "alice",
        "actor": actor,
        "role": role,
        "commit_seq": commit_seq,
        "base_document": encoded,
        "operations": operations,
        "peers": [{
            "subject": "alice",
            "actor": actor,
            "display_name": "Alice",
            "role": role,
            "cursor_anchor": serde_json::Value::Null,
            "last_seen_ms": 1_700_000_000_000u64,
            "connections": 1,
        }],
        "max_operations_per_submit": max_operations_per_submit,
    })
    .to_string()
}

fn rejected_frame(code: &str, message: &str) -> String {
    serde_json::json!({
        "type": "rejected",
        "batch_id": "batch-1",
        "code": code,
        "message": message,
    })
    .to_string()
}

/// Every `Submit` frame in an outbox, as (batch id, the sequence numbers it
/// carries).
fn submits_of(outbox: &[String]) -> Vec<(String, Vec<u64>)> {
    frames_of(outbox)
        .into_iter()
        .filter(|frame| frame["type"] == "submit")
        .map(|frame| {
            (
                frame["batch_id"].as_str().unwrap_or_default().to_string(),
                frame["operations"]
                    .as_array()
                    .expect("a submit carries operations")
                    .iter()
                    .map(|operation| operation["id"]["seq"].as_u64().expect("a sequence"))
                    .collect(),
            )
        })
        .collect()
}

/// A commit as the service fans it out, authored by someone else.
fn committed_frame(commit_seq: u64, operations: &[Operation]) -> String {
    serde_json::json!({
        "type": "committed",
        "commit_seq": commit_seq,
        "subject": "bob",
        "actor": "actor-bob",
        "operations": operations,
    })
    .to_string()
}

fn accepted_frame(commit_seq: u64, seqs: &[u64]) -> String {
    serde_json::json!({
        "type": "accepted",
        "batch_id": "whatever",
        "commit_seq": commit_seq,
        "operation_ids": seqs
            .iter()
            .map(|seq| serde_json::json!({ "actor": "actor-alice", "seq": seq }))
            .collect::<Vec<_>>(),
    })
    .to_string()
}

fn presence_frame(cursor: &str) -> String {
    serde_json::json!({
        "type": "presence",
        "peers": [
            {
                "subject": "alice",
                "actor": "actor-alice",
                "display_name": "Alice",
                "role": "editor",
                "cursor_anchor": serde_json::Value::Null,
                "last_seen_ms": 1_700_000_000_001u64,
                "connections": 2,
            },
            {
                "subject": "bob",
                "actor": "actor-bob",
                "display_name": "Bob",
                "role": "commenter",
                "cursor_anchor": cursor,
                "last_seen_ms": 1_700_000_000_002u64,
                "connections": 1,
            },
        ],
    })
    .to_string()
}

/// An operation authored the way another replica would author it.
fn remote_insert(seq: u64, offset: usize, text: &str) -> Operation {
    Operation {
        id: opendoc_app::OperationId {
            actor: opendoc_app::ActorId("actor-bob".to_string()),
            seq,
        },
        kind: OperationKind::InsertText {
            inline_id: StableId::parse(RUN_ID).expect("run id"),
            offset,
            text: text.to_string(),
        },
        context: None,
    }
}

/// A keystroke, through the ordinary command surface — the only way a gesture
/// becomes an operation.
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
    .expect("typing into the shared run");
}

fn run_text(app: &OpenDocApp) -> String {
    for block in &app.source_document().blocks {
        for inline in &block.content {
            if let Inline::Text { id, text, .. } = inline {
                if id.as_str() == RUN_ID {
                    return text.clone();
                }
            }
        }
    }
    String::new()
}

fn frames_of(outbox: &[String]) -> Vec<serde_json::Value> {
    outbox
        .iter()
        .map(|frame| serde_json::from_str(frame).expect("an outbound frame is JSON"))
        .collect()
}

fn live_session() -> (CollabDriver, OpenDocApp) {
    let mut driver = CollabDriver::default();
    let mut app = OpenDocApp::new_empty_document();
    driver.begin(DOCUMENT_UUID, "Alice");
    driver
        .ingest(&mut app, &welcome_frame(0, &[], "editor"))
        .expect("the welcome is adopted");
    (driver, app)
}

#[test]
fn the_welcome_makes_this_replica_a_client_of_the_service() {
    let (mut driver, app) = live_session();
    let status = driver.status(&app);
    assert_eq!(status.phase, Phase::Live);
    assert_eq!(status.document_uuid, DOCUMENT_UUID);
    assert!(
        status.document_changed,
        "the document was replaced by the service's; the caller must re-render"
    );
    let session = status.session.expect("the app holds the service's answers");
    assert_eq!(session.subject, "alice");
    assert_eq!(session.actor, "actor-alice");
    assert_eq!(session.role, OpenDocServiceRole::Editor);
    assert_eq!(app.actor_id(), "actor-alice", "the actor is the service's");
    assert_eq!(run_text(&app), BASE_TEXT);
}

#[test]
fn a_local_keystroke_becomes_one_submit_frame_and_is_not_sent_twice() {
    let (mut driver, mut app) = live_session();
    // The first outbox announces this connection's presence.
    let first = frames_of(&driver.outbox(&mut app));
    assert_eq!(first.len(), 1);
    assert_eq!(first[0]["type"], "presence");

    type_into_run(&mut app, 0, "Z");
    let frames = frames_of(&driver.outbox(&mut app));
    assert_eq!(frames.len(), 1, "one batch, and no second presence frame");
    assert_eq!(frames[0]["type"], "submit");
    let operations = frames[0]["operations"].as_array().expect("operations");
    assert_eq!(operations.len(), 1);
    assert_eq!(operations[0]["id"]["actor"], "actor-alice");
    assert_eq!(operations[0]["id"]["seq"], 1);

    assert!(
        driver.outbox(&mut app).is_empty(),
        "a batch already on the wire must not be resubmitted on every tick"
    );
    assert_eq!(driver.status(&app).pending_operations, 1);

    driver
        .ingest(&mut app, &accepted_frame(1, &[1]))
        .expect("the acknowledgement");
    let status = driver.status(&app);
    assert_eq!(status.acknowledged_seq, 1);
    assert_eq!(status.pending_operations, 0);
    assert_eq!(status.commit_seq, 1);
    // The *app* must be told too, not only this driver: the watermark in the
    // session is what `get_runtime_session` reports, and a transport that kept
    // it to itself would leave the rest of the app believing nothing is
    // durable.
    assert_eq!(
        status.session.expect("a session").acknowledged_seq,
        1,
        "the acknowledgement must reach the app's own session"
    );
}

#[test]
fn a_commit_from_another_replica_changes_the_document_and_says_so_once() {
    let (mut driver, mut app) = live_session();
    driver
        .ingest(&mut app, &committed_frame(1, &[remote_insert(1, 0, "R")]))
        .expect("the commit is applied");
    let status = driver.status(&app);
    assert!(status.document_changed);
    assert_eq!(run_text(&app), format!("R{BASE_TEXT}"));
    assert!(
        !driver.status(&app).document_changed,
        "the flag is consumed by the reader, so one commit does not cause two re-renders"
    );

    // The service relays a commit to its submitter too. Applying one's own
    // work twice must be a no-op, not a second insertion.
    driver
        .ingest(&mut app, &committed_frame(2, &[remote_insert(1, 0, "R")]))
        .expect("the redelivery");
    assert_eq!(run_text(&app), format!("R{BASE_TEXT}"));
    assert!(!driver.status(&app).document_changed);
}

#[test]
fn presence_frames_become_the_peers_the_ui_renders() {
    let (mut driver, mut app) = live_session();
    driver
        .ingest(&mut app, &presence_frame("blk-collab-0001:4"))
        .expect("the presence frame");
    let session = driver.status(&app).session.expect("a session");
    assert_eq!(session.peers.len(), 2);
    let bob = session
        .peers
        .iter()
        .find(|peer| peer.subject == "bob")
        .expect("bob is present");
    assert_eq!(bob.actor, "actor-bob");
    assert_eq!(bob.display_name, "Bob");
    assert_eq!(bob.role, OpenDocServiceRole::Commenter);
    assert_eq!(bob.cursor_anchor.as_deref(), Some("blk-collab-0001:4"));
    assert_eq!(bob.connections, 1);
    let alice = session
        .peers
        .iter()
        .find(|peer| peer.subject == "alice")
        .expect("alice is present");
    assert_eq!(
        alice.connections, 2,
        "one person in two tabs is one peer with two connections"
    );
}

#[test]
fn a_presence_re_attestation_updates_our_own_role() {
    let (mut driver, mut app) = live_session();
    let downgraded = presence_frame("blk-collab-0001:4").replacen(
        "\"role\":\"editor\"",
        "\"role\":\"viewer\"",
        1,
    );
    driver
        .ingest(&mut app, &downgraded)
        .expect("the service re-attestation");

    let session = driver.status(&app).session.expect("a session");
    assert_eq!(
        session.role,
        OpenDocServiceRole::Viewer,
        "the local role must not remain the role from the welcome after a grant change"
    );
    assert_eq!(
        session
            .peers
            .iter()
            .find(|peer| peer.subject == "alice" && peer.actor == "actor-alice")
            .expect("the service includes this connection in presence")
            .role,
        OpenDocServiceRole::Viewer
    );
}

#[test]
fn a_cursor_is_announced_when_it_moves_and_not_before() {
    let (mut driver, mut app) = live_session();
    let _ = driver.outbox(&mut app);
    assert!(driver.outbox(&mut app).is_empty());

    driver.set_cursor(Some("blk-collab-0001:3".to_string()));
    let frames = frames_of(&driver.outbox(&mut app));
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0]["type"], "presence");
    assert_eq!(frames[0]["cursor_anchor"], "blk-collab-0001:3");
    assert_eq!(frames[0]["display_name"], "Alice");

    driver.set_cursor(Some("blk-collab-0001:3".to_string()));
    assert!(
        driver.outbox(&mut app).is_empty(),
        "an unchanged cursor must not put a frame on the wire every tick"
    );
}

#[test]
fn a_selection_endpoint_is_announced_once_and_is_independent_of_the_focus() {
    let (mut driver, mut app) = live_session();
    let _ = driver.outbox(&mut app);
    driver.set_cursor(Some("blk-collab-0001:5".to_string()));
    driver.set_selection_anchor(Some("blk-collab-0001:1".to_string()));
    let frames = frames_of(&driver.outbox(&mut app));
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0]["cursor_anchor"], "blk-collab-0001:5");
    assert_eq!(frames[0]["selection_anchor"], "blk-collab-0001:1");
    assert!(driver.outbox(&mut app).is_empty());
}

#[test]
fn a_dropped_socket_resynchronises_from_the_next_welcome_and_replays_unsent_work() {
    let (mut driver, mut app) = live_session();
    let _ = driver.outbox(&mut app);

    // Two keystrokes: the first is acknowledged, the second is on the wire
    // when the socket dies, so the service may or may not have it.
    type_into_run(&mut app, 0, "A");
    let _ = driver.outbox(&mut app);
    driver
        .ingest(&mut app, &accepted_frame(1, &[1]))
        .expect("the first is durable");
    type_into_run(&mut app, 1, "B");
    let _ = driver.outbox(&mut app);
    assert_eq!(run_text(&app), format!("AB{BASE_TEXT}"));

    driver.socket_closed("1006", "abnormal closure");
    let dropped = driver.status(&app);
    assert_eq!(dropped.phase, Phase::Reconnecting);
    let notice = dropped.notice.expect("a dropped socket is reported");
    assert_eq!(notice.kind, "socket-closed");
    assert!(notice.resumable);

    // The service never got the second operation. Its log has the first.
    let acknowledged = app
        .local_operations_after(0)
        .into_iter()
        .filter(|operation| operation.id.seq == 1)
        .collect::<Vec<_>>();
    assert_eq!(acknowledged.len(), 1);

    driver.begin(DOCUMENT_UUID, "Alice");
    driver
        .ingest(&mut app, &welcome_frame(1, &acknowledged, "editor"))
        .expect("the second welcome");

    let status = driver.status(&app);
    assert_eq!(status.phase, Phase::Live);
    assert_eq!(
        status.acknowledged_seq, 1,
        "the watermark is read back out of the log the welcome carried"
    );
    assert_eq!(
        run_text(&app),
        format!("AB{BASE_TEXT}"),
        "work the service never acknowledged must survive the reconnect"
    );
    let notice = status.notice.expect("a resynchronisation is reported");
    assert_eq!(notice.kind, "resynchronised");
    assert!(notice.message.contains("1 change(s)"), "{}", notice.message);

    // And it is put back on the wire.
    let frames = frames_of(&driver.outbox(&mut app));
    let submits: Vec<&serde_json::Value> = frames
        .iter()
        .filter(|frame| frame["type"] == "submit")
        .collect();
    assert_eq!(submits.len(), 1, "the unsent tail is resubmitted");
    let operations = submits[0]["operations"].as_array().expect("operations");
    assert_eq!(operations.len(), 1);
    assert_eq!(operations[0]["id"]["seq"], 2);
}

#[test]
fn a_reconnect_that_finds_its_work_already_committed_resubmits_nothing() {
    let (mut driver, mut app) = live_session();
    let _ = driver.outbox(&mut app);
    type_into_run(&mut app, 0, "A");
    let _ = driver.outbox(&mut app);
    // The socket died before the acknowledgement arrived, but the service had
    // already committed it.
    let committed = app.local_operations_after(0);
    assert_eq!(committed.len(), 1);
    driver.socket_closed("1006", "");

    driver.begin(DOCUMENT_UUID, "Alice");
    driver
        .ingest(&mut app, &welcome_frame(1, &committed, "editor"))
        .expect("the second welcome");
    let status = driver.status(&app);
    assert_eq!(status.acknowledged_seq, 1);
    assert_eq!(status.pending_operations, 0);
    assert_eq!(run_text(&app), format!("A{BASE_TEXT}"));
    let frames = frames_of(&driver.outbox(&mut app));
    assert!(
        frames.iter().all(|frame| frame["type"] != "submit"),
        "work the service already holds must not be sent again"
    );
}

#[test]
fn a_refusal_stops_the_session_submitting_and_is_reported_in_the_services_own_words() {
    let (mut driver, mut app) = live_session();
    let _ = driver.outbox(&mut app);
    type_into_run(&mut app, 0, "A");
    let _ = driver.outbox(&mut app);
    driver
        .ingest(
            &mut app,
            &serde_json::json!({
                "type": "rejected",
                "batch_id": "batch-1",
                "code": "bad-request",
                "message": "operation 2 is not dense after 0",
            })
            .to_string(),
        )
        .expect("a rejection is a frame, not an error");
    let status = driver.status(&app);
    let notice = status.notice.expect("the refusal reaches the user");
    assert_eq!(notice.kind, "refused-bad-request");
    assert!(
        notice.message.contains("operation 2 is not dense after 0"),
        "the service's own message must be shown: {}",
        notice.message
    );
    assert!(!status.can_submit);

    type_into_run(&mut app, 0, "B");
    let frames = frames_of(&driver.outbox(&mut app));
    assert!(
        frames.iter().all(|frame| frame["type"] != "submit"),
        "a refused session must not keep pushing batches the service will refuse"
    );
}

/// Undo inside a live session, after ADR 0017.
///
/// This was the known hazard: undo rewound the operation counter below the
/// acknowledged watermark, and the service refuses to rewrite history. It is
/// now the *inverse* of this actor's own operations, authored like any other
/// edit — so from a transport's side an undo is simply more work to submit,
/// and this test is what says so from out here rather than taking the app's
/// word for it.
#[test]
fn an_undo_inside_a_live_session_is_submitted_as_new_work() {
    let (mut driver, mut app) = live_session();
    let _ = driver.outbox(&mut app);
    type_into_run(&mut app, 0, "A");
    type_into_run(&mut app, 1, "B");
    let first = frames_of(&driver.outbox(&mut app));
    let submitted: Vec<u64> = first
        .iter()
        .filter(|frame| frame["type"] == "submit")
        .flat_map(|frame| frame["operations"].as_array().cloned().unwrap_or_default())
        .map(|operation| operation["id"]["seq"].as_u64().unwrap_or(0))
        .collect();
    let highest_before = submitted.iter().copied().max().expect("a batch went out");
    driver
        .ingest(&mut app, &accepted_frame(1, &submitted))
        .expect("the batch is durable");
    let text_before = run_text(&app);

    app.dispatch_command("undo_current_edit", serde_json::json!({}))
        .expect("undo is a command like any other");
    assert_ne!(run_text(&app), text_before, "the undo changed the document");

    let frames = frames_of(&driver.outbox(&mut app));
    let batch: Vec<&serde_json::Value> = frames
        .iter()
        .filter(|frame| frame["type"] == "submit")
        .collect();
    assert_eq!(batch.len(), 1, "the undo is one more batch, not a refusal");
    let operations = batch[0]["operations"].as_array().expect("operations");
    assert!(!operations.is_empty());
    for operation in operations {
        assert!(
            operation["id"]["seq"].as_u64().unwrap_or(0) > highest_before,
            "an undo must be *new* operations, above everything the service holds: {operation}"
        );
        assert_eq!(operation["id"]["actor"], "actor-alice");
    }
    let status = driver.status(&app);
    assert!(status.can_submit, "nothing here is a refusal");
    assert!(
        status
            .notice
            .as_ref()
            .is_none_or(|notice| notice.kind != "history-rewritten"),
        "{:?}",
        status.notice
    );
}

/// The cross-check behind that: a replica holding fewer of its own operations
/// than the service has made durable stops submitting and says why.
///
/// Undo no longer produces this state (see above), which is exactly why the
/// check is worth keeping and worth testing — the failure it catches is silent
/// divergence, and nothing else would notice it.
#[test]
fn a_replica_behind_the_services_watermark_stops_submitting_and_says_why() {
    let (mut driver, mut app) = live_session();
    let _ = driver.outbox(&mut app);
    type_into_run(&mut app, 0, "A");
    let _ = driver.outbox(&mut app);
    // The service claims operation 4 of this actor is durable. This replica
    // has authored one. Its next id would be one the log already holds.
    driver
        .ingest(&mut app, &accepted_frame(2, &[4]))
        .expect("the acknowledgement");

    type_into_run(&mut app, 1, "B");
    let frames = frames_of(&driver.outbox(&mut app));
    assert!(
        frames.iter().all(|frame| frame["type"] != "submit"),
        "nothing may go on the wire once this replica is behind the service"
    );
    let notice = driver
        .status(&app)
        .notice
        .expect("the refusal must reach the user");
    assert_eq!(notice.kind, "history-rewritten");
    assert!(
        notice.message.contains("refuses to rewrite history"),
        "{}",
        notice.message
    );
    assert!(!driver.status(&app).can_submit);
}

#[test]
fn a_viewer_is_told_why_its_typing_is_not_being_sent() {
    let mut driver = CollabDriver::default();
    let mut app = OpenDocApp::new_empty_document();
    driver.begin(DOCUMENT_UUID, "Alice");
    driver
        .ingest(&mut app, &welcome_frame(0, &[], "viewer"))
        .expect("a viewer may still join");
    let _ = driver.outbox(&mut app);
    type_into_run(&mut app, 0, "A");
    let frames = frames_of(&driver.outbox(&mut app));
    assert!(frames.iter().all(|frame| frame["type"] != "submit"));
    let notice = driver.status(&app).notice.expect("a notice");
    assert_eq!(notice.kind, "not-an-editor");
    assert!(notice.message.contains("viewer"), "{}", notice.message);
    assert!(!driver.status(&app).can_submit);
}

#[test]
fn a_close_the_service_will_repeat_is_not_reported_as_reconnecting() {
    let (mut driver, mut app) = live_session();
    driver
        .ingest(
            &mut app,
            &serde_json::json!({
                "type": "closed",
                "code": "forbidden",
                "message": "read access was revoked",
            })
            .to_string(),
        )
        .expect("a close is a frame");
    let status = driver.status(&app);
    assert_eq!(status.phase, Phase::Closed);
    let notice = status.notice.expect("a notice");
    assert_eq!(notice.kind, "closed-forbidden");
    assert!(
        !notice.resumable,
        "a revoked grant refuses the next handshake too; saying 'reconnecting' would be a lie"
    );
    // And the socket dropping afterwards must not overwrite that with hope.
    driver.socket_closed("1006", "");
    let status = driver.status(&app);
    assert_eq!(status.phase, Phase::Closed);
    assert_eq!(status.notice.expect("a notice").kind, "closed-forbidden");
}

#[test]
fn a_welcome_from_a_protocol_this_client_does_not_speak_is_refused_not_guessed_at() {
    let mut driver = CollabDriver::default();
    let mut app = OpenDocApp::new_empty_document();
    driver.begin(DOCUMENT_UUID, "Alice");
    let frame = welcome_frame(0, &[], "editor").replace(
        &format!("\"protocol_version\":{PROTOCOL_VERSION}"),
        "\"protocol_version\":999",
    );
    let error = driver
        .ingest(&mut app, &frame)
        .expect_err("a version mismatch must not be adopted");
    assert!(error.contains("999"), "{error}");
    let status = driver.status(&app);
    assert_eq!(status.phase, Phase::Closed);
    assert_eq!(
        status.notice.expect("a notice").kind,
        "protocol-version".to_string()
    );
    assert!(status.session.is_none(), "nothing was joined");
}

#[test]
fn leaving_drops_the_services_answers_and_keeps_the_document() {
    let (mut driver, mut app) = live_session();
    let _ = driver.outbox(&mut app);
    type_into_run(&mut app, 0, "A");
    driver.leave(&mut app);
    let status = driver.status(&app);
    assert_eq!(status.phase, Phase::Idle);
    assert!(
        status.session.is_none(),
        "keeping a role after the socket closed would be the client deciding its own permissions"
    );
    assert_eq!(
        run_text(&app),
        format!("A{BASE_TEXT}"),
        "the document and its history stay; only the anchor to the service goes"
    );
    assert!(driver.outbox(&mut app).is_empty());
}

/// A notice is a DTO the UI reads; this pins the shape so a rename cannot
/// silently stop the banner from rendering.
#[test]
fn a_notice_serializes_with_the_fields_the_frontend_reads() {
    let notice = Notice {
        kind: "socket-closed".to_string(),
        message: "gone".to_string(),
        resumable: true,
    };
    assert_eq!(
        serde_json::to_value(&notice).expect("serializing"),
        serde_json::json!({ "kind": "socket-closed", "message": "gone", "resumable": true })
    );
}

/// The status DTO the frontend renders from, pinned the same way.
#[test]
fn the_status_serializes_with_the_fields_the_frontend_reads() {
    let (mut driver, mut app) = live_session();
    driver
        .ingest(&mut app, &presence_frame("blk-collab-0001:1"))
        .expect("presence");
    let value = serde_json::to_value(driver.status(&app)).expect("serializing the status");
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
    assert_eq!(value["phase"], "live");
    assert_eq!(value["session"]["peers"][1]["display_name"], "Bob");
    let _ = WirePeer {
        subject: String::new(),
        actor: String::new(),
        display_name: String::new(),
        role: OpenDocServiceRole::Viewer,
        cursor_anchor: None,
        selection_anchor: None,
        last_seen_ms: 0,
        connections: 0,
    };
}

// ---- The caret moves with the text -----------------------------------------
//
// `OpenDocApp::apply_remote_operations` has always computed a rebased caret
// and returned it in `AppRemoteIntake::selection`. Every caller here passed
// `None`, and `CollabStatus` had nowhere to put the answer, so a collaborator
// typing in front of your caret left it at the same character index — three
// characters earlier in the text than where it had been.

fn caret(offset: usize) -> opendoc_app::EditorSelection {
    opendoc_app::EditorSelection::collapsed(opendoc_app::EditorPosition {
        block_id: BLOCK_ID.to_string(),
        inline_id: Some(RUN_ID.to_string()),
        offset,
    })
}

#[test]
fn a_commit_in_front_of_the_caret_moves_it() {
    let (mut driver, mut app) = live_session();
    driver.set_selection(Some(caret(5)));
    let _ = driver.status(&app);

    driver
        .ingest(&mut app, &committed_frame(1, &[remote_insert(1, 0, "XY")]))
        .expect("the commit is applied");

    assert_eq!(run_text(&app), "XYabcdefgh");
    let status = driver.status(&app);
    let moved = status
        .selection
        .expect("the caret the caller is holding was rebased");
    assert_eq!(
        moved.focus.offset, 7,
        "two characters landed before the caret, so it moved by two"
    );
    assert_eq!(moved.focus.block_id, BLOCK_ID);
    assert_eq!(moved.focus.inline_id.as_deref(), Some(RUN_ID));
}

#[test]
fn a_commit_behind_the_caret_leaves_it_where_it_is() {
    let (mut driver, mut app) = live_session();
    driver.set_selection(Some(caret(2)));
    let _ = driver.status(&app);

    driver
        .ingest(&mut app, &committed_frame(1, &[remote_insert(1, 6, "XY")]))
        .expect("the commit is applied");

    let status = driver.status(&app);
    let moved = status.selection.expect("a rebased caret is still reported");
    assert_eq!(
        moved.focus.offset, 2,
        "text typed after the caret must not move it"
    );
}

/// The answer describes the frames just ingested, so it is read once — like
/// `document_changed`. Re-applying it on a later tick would drag the caret
/// back from wherever the user has since put it.
#[test]
fn the_rebased_caret_is_reported_once() {
    let (mut driver, mut app) = live_session();
    driver.set_selection(Some(caret(5)));
    let _ = driver.status(&app);
    driver
        .ingest(&mut app, &committed_frame(1, &[remote_insert(1, 0, "XY")]))
        .expect("the commit is applied");
    assert!(driver.status(&app).selection.is_some());
    assert!(
        driver.status(&app).selection.is_none(),
        "the caret was offered a second time"
    );
}

/// Successive commits compose. The second rebase has to start from where the
/// first one left the caret, not from where the caller last said it was —
/// otherwise the caret is right after one collaborator's keystroke and wrong
/// after two.
#[test]
fn successive_commits_each_move_the_caret_again() {
    let (mut driver, mut app) = live_session();
    driver.set_selection(Some(caret(5)));
    let _ = driver.status(&app);
    driver
        .ingest(&mut app, &committed_frame(1, &[remote_insert(1, 0, "XY")]))
        .expect("the first commit");
    let _ = driver.status(&app);
    driver
        .ingest(&mut app, &committed_frame(2, &[remote_insert(2, 0, "Z")]))
        .expect("the second commit");
    let status = driver.status(&app);
    assert_eq!(
        status
            .selection
            .expect("a rebased caret after the second commit")
            .focus
            .offset,
        8,
        "three characters have landed in front of a caret that started at 5"
    );
}

/// A caller that shows no caret is not made to invent one.
#[test]
fn without_a_caret_there_is_nothing_to_report() {
    let (mut driver, mut app) = live_session();
    let _ = driver.status(&app);
    driver
        .ingest(&mut app, &committed_frame(1, &[remote_insert(1, 0, "XY")]))
        .expect("the commit is applied");
    let status = driver.status(&app);
    assert!(status.document_changed);
    assert!(status.selection.is_none());
}

// ---- P1-8: the outbox is chunked, and a refusal is not the end -----------

/// A tail longer than the service's cap goes out as several `Submit` frames,
/// each within the cap, together carrying every operation exactly once.
///
/// The answer is pinned, not merely "more than one frame": a replayed
/// disconnection used to build one oversized batch, the service refused it,
/// and nothing ever cleared the refusal.
#[test]
fn a_tail_longer_than_the_services_cap_goes_out_as_several_submits() {
    let mut driver = CollabDriver::default();
    let mut app = OpenDocApp::new_empty_document();
    driver.begin(DOCUMENT_UUID, "Alice");
    driver
        .ingest(
            &mut app,
            &welcome_frame_as(0, &[], "editor", "actor-alice", 4),
        )
        .expect("the welcome is adopted");

    for index in 0..10 {
        type_into_run(&mut app, BASE_TEXT.len() + index, "x");
    }

    let submits = submits_of(&driver.outbox(&mut app));
    assert_eq!(
        submits
            .iter()
            .map(|(_, seqs)| seqs.clone())
            .collect::<Vec<_>>(),
        vec![vec![1, 2, 3, 4], vec![5, 6, 7, 8], vec![9, 10]],
        "ten operations against a cap of four are three submits, in order, with nothing repeated"
    );
    let batch_ids: Vec<&str> = submits.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(
        batch_ids,
        vec![
            "doc-collab-0001-1",
            "doc-collab-0001-2",
            "doc-collab-0001-3"
        ],
        "each chunk is its own batch, so the service can accept or refuse them one at a time"
    );

    // And the watermark moved past the whole tail, so a second tick with no
    // new typing sends nothing.
    assert!(
        submits_of(&driver.outbox(&mut app)).is_empty(),
        "a chunked tail must not be sent twice"
    );
}

/// A refusal stops this session submitting *and* asks for a fresh welcome, and
/// the welcome unblocks it.
///
/// `blocked` used to be cleared only by a welcome, and nothing asked for one —
/// so a refused session sat in `live` with `can_submit: false` while every
/// keystroke piled up locally, for ever (P1-8).
#[test]
fn a_refusal_asks_for_a_fresh_welcome_and_the_next_one_unblocks_the_session() {
    let (mut driver, mut app) = live_session();
    type_into_run(&mut app, BASE_TEXT.len(), "a");
    assert_eq!(submits_of(&driver.outbox(&mut app)).len(), 1);

    driver
        .ingest(&mut app, &rejected_frame("conflict", "out of sequence"))
        .expect("a rejection is a frame, not an error");
    let refused = driver.status(&app);
    assert_eq!(refused.phase, Phase::Live);
    assert!(!refused.can_submit, "a refused session stops submitting");
    assert!(
        refused.reconnect_requested,
        "a refusal must ask for the service's log rather than sit in `live` for ever"
    );
    assert_eq!(
        refused.notice.as_ref().map(|n| n.kind.clone()),
        Some("refused-conflict".to_string())
    );
    assert!(
        refused.notice.as_ref().is_some_and(|n| n.resumable),
        "the first refusal is recoverable"
    );
    assert!(
        !driver.status(&app).reconnect_requested,
        "the request is read once, or the caller would drop the socket it just opened"
    );
    assert!(
        submits_of(&driver.outbox(&mut app)).is_empty(),
        "nothing goes out while the session is blocked"
    );

    // The socket goes and comes back; the welcome is the resynchronisation.
    driver.socket_closed("1006", "resynchronising");
    driver.begin(DOCUMENT_UUID, "Alice");
    driver
        .ingest(&mut app, &welcome_frame(0, &[], "editor"))
        .expect("the second welcome is adopted");
    let resumed = driver.status(&app);
    assert_eq!(resumed.phase, Phase::Live);
    assert!(resumed.can_submit, "the welcome is what clears the block");
    assert_eq!(
        submits_of(&driver.outbox(&mut app))
            .into_iter()
            .map(|(_, seqs)| seqs)
            .collect::<Vec<_>>(),
        vec![vec![1]],
        "the tail the service never took is replayed"
    );
}

/// A refusal a welcome cannot fix ends the session instead of looping.
#[test]
fn a_refusal_that_a_welcome_cannot_fix_ends_the_session_rather_than_looping() {
    let (mut driver, mut app) = live_session();
    for attempt in 1..=3 {
        type_into_run(&mut app, BASE_TEXT.len(), "a");
        let _ = driver.outbox(&mut app);
        driver
            .ingest(&mut app, &rejected_frame("conflict", "out of sequence"))
            .expect("a rejection is a frame");
        if attempt < 3 {
            driver.socket_closed("1006", "resynchronising");
            driver.begin(DOCUMENT_UUID, "Alice");
            driver
                .ingest(&mut app, &welcome_frame(0, &[], "editor"))
                .expect("the welcome is adopted");
        }
    }
    let over = driver.status(&app);
    assert_eq!(
        over.phase,
        Phase::Closed,
        "three refusals a welcome could not fix is a session that is over"
    );
    assert!(!over.reconnect_requested, "nothing more is being asked for");
    let notice = over.notice.expect("an explanation");
    assert!(!notice.resumable, "{}", notice.message);
    assert!(
        notice.message.contains("3 refusals"),
        "the message must say why it stopped: {}",
        notice.message
    );
}

/// A batch the service accepts forgives the refusals before it, so an
/// occasional refusal over a long session never adds up to the cap.
#[test]
fn an_accepted_batch_forgives_the_refusals_before_it() {
    let (mut driver, mut app) = live_session();
    for _ in 0..2 {
        type_into_run(&mut app, BASE_TEXT.len(), "a");
        let _ = driver.outbox(&mut app);
        driver
            .ingest(&mut app, &rejected_frame("conflict", "out of sequence"))
            .expect("a rejection is a frame");
        driver.socket_closed("1006", "resynchronising");
        driver.begin(DOCUMENT_UUID, "Alice");
        driver
            .ingest(&mut app, &welcome_frame(0, &[], "editor"))
            .expect("the welcome is adopted");
    }
    // Two refusals in; one acceptance, then a third refusal.
    let _ = driver.outbox(&mut app);
    driver
        .ingest(&mut app, &accepted_frame(1, &[1, 2]))
        .expect("an acceptance");
    driver
        .ingest(&mut app, &rejected_frame("conflict", "out of sequence"))
        .expect("a rejection is a frame");
    let status = driver.status(&app);
    assert_eq!(
        status.phase,
        Phase::Live,
        "the count restarted at the acceptance, so this is the first refusal again"
    );
    assert!(status.reconnect_requested);
    assert!(status.notice.expect("a notice").resumable);
}

// ---- P1-9: a commit that cannot be applied ------------------------------

/// A `Committed` frame this replica cannot apply stops the session, leaves the
/// commit sequence where it was, and asks for the log again.
///
/// `apply_remote_operations` is all or nothing, so a failure loses the *whole*
/// commit. The old code recorded a notice and carried on — still `Live`, still
/// submitting, silently diverged from everyone else (P1-9).
#[test]
fn a_commit_that_cannot_be_applied_stops_the_session_and_does_not_advance_the_sequence() {
    let (mut driver, mut app) = live_session();
    driver
        .ingest(&mut app, &committed_frame(4, &[remote_insert(1, 1, "B")]))
        .expect("the first commit applies");
    assert_eq!(driver.status(&app).commit_seq, 4);
    let applied_text = run_text(&app);

    // The same operation id under a different payload: the replica refuses to
    // rewrite its own history, exactly as the service does, so the commit
    // cannot be applied at all.
    let conflicting = committed_frame(5, &[remote_insert(1, 1, "DIFFERENT")]);
    driver
        .ingest(&mut app, &conflicting)
        .expect_err("a commit that cannot be applied is an error to the caller too");

    let status = driver.status(&app);
    assert_eq!(
        status.commit_seq, 4,
        "the sequence must not move past a commit this replica does not have"
    );
    assert_eq!(run_text(&app), applied_text, "nothing of it landed");
    assert!(
        !status.can_submit,
        "a replica behind the service must stop authoring against a document the service does not have"
    );
    assert!(
        status.reconnect_requested,
        "the welcome carries the whole log, which is this client's only way to re-request a commit"
    );
    assert_eq!(
        status.notice.as_ref().map(|notice| notice.kind.clone()),
        Some("commit-not-applied".to_string())
    );
    assert!(
        submits_of(&driver.outbox(&mut app)).is_empty(),
        "nothing goes out until the replica has caught up"
    );
}

// ---- the acknowledgement that never came --------------------------------

/// A submit the service never acknowledges is sent again rather than stranded.
///
/// There was no acknowledgement timeout anywhere: `submitted_through` moves as
/// a batch goes out and comes back only when the socket closes, so an
/// `Accepted` dropped on a socket that stayed open stranded that batch and
/// everything after it, with the pill still reading "Live".
#[test]
fn an_unacknowledged_submit_is_sent_again_rather_than_stranded() {
    let (mut driver, mut app) = live_session();
    type_into_run(&mut app, BASE_TEXT.len(), "a");
    assert_eq!(
        submits_of(&driver.outbox(&mut app))
            .into_iter()
            .map(|(_, seqs)| seqs)
            .collect::<Vec<_>>(),
        vec![vec![1]]
    );

    // Thirty-nine more ticks with no acknowledgement: still waiting, nothing
    // resent, because a resend on every tick would be a flood. The batch went
    // out on tick 1, so the fortieth tick *after* it is tick 41.
    for tick in 2..=40 {
        assert!(
            submits_of(&driver.outbox(&mut app)).is_empty(),
            "tick {tick} must not resend yet"
        );
    }
    let resent = submits_of(&driver.outbox(&mut app));
    assert_eq!(
        resent.iter().map(|(_, seqs)| seqs.clone()).collect::<Vec<_>>(),
        vec![vec![1]],
        "ten seconds of silence sends the same operation again — byte-identical, which the service takes as a retry"
    );
    assert_ne!(
        resent[0].0, "doc-collab-0001-1",
        "a resend is a new batch id, so an acknowledgement cannot be matched to the wrong one"
    );
    assert_eq!(
        driver.status(&app).notice.map(|notice| notice.kind),
        Some("acknowledgement-timed-out".to_string())
    );

    // And once it is acknowledged, nothing is resent again.
    driver
        .ingest(&mut app, &accepted_frame(1, &[1]))
        .expect("the acknowledgement");
    for _ in 0..45 {
        assert!(submits_of(&driver.outbox(&mut app)).is_empty());
    }
}

/// The timeout is measured from the batch that is actually outstanding, not
/// from some earlier one the service already acknowledged.
///
/// The exact tick matters and is the whole of the assertion: a batch sent on
/// tick 2 must get its full ten seconds, so it is resent on tick 42 and not on
/// tick 41. An acknowledgement that does not clear the wait leaves the clock
/// running from the *previous* batch, which cuts the window short by however
/// long the first batch was outstanding — and in a busy session that converges
/// on resending everything immediately.
#[test]
fn the_acknowledgement_timeout_is_measured_from_the_batch_that_is_outstanding() {
    let (mut driver, mut app) = live_session();
    // Tick 1: the first batch goes out and is acknowledged.
    type_into_run(&mut app, BASE_TEXT.len(), "a");
    assert_eq!(
        submits_of(&driver.outbox(&mut app))
            .into_iter()
            .map(|(_, seqs)| seqs)
            .collect::<Vec<_>>(),
        vec![vec![1]]
    );
    driver
        .ingest(&mut app, &accepted_frame(1, &[1]))
        .expect("the acknowledgement");

    // Tick 2: a second batch goes out and is never acknowledged.
    type_into_run(&mut app, BASE_TEXT.len() + 1, "b");
    assert_eq!(
        submits_of(&driver.outbox(&mut app))
            .into_iter()
            .map(|(_, seqs)| seqs)
            .collect::<Vec<_>>(),
        vec![vec![2]]
    );

    // Ticks 3 to 41. Forty ticks after the *first* batch falls in here, and
    // nothing may be resent on it.
    for tick in 3..=41 {
        assert!(
            submits_of(&driver.outbox(&mut app)).is_empty(),
            "tick {tick} resent a batch that has not been outstanding for ten seconds"
        );
    }
    assert!(
        driver.status(&app).notice.is_none(),
        "nothing has timed out yet, so nothing may say so"
    );
    // Tick 42: forty ticks after the second batch went out.
    assert_eq!(
        submits_of(&driver.outbox(&mut app))
            .into_iter()
            .map(|(_, seqs)| seqs)
            .collect::<Vec<_>>(),
        vec![vec![2]],
        "the unacknowledged batch, and only it, is sent again"
    );
}

// ---- the states that used to read "Live" --------------------------------

/// Work this replica cannot replay ends the session instead of leaving the
/// pill reading "Live" over a session that can never send anything again.
///
/// The trigger is the one the protocol really allows: a subject's actor
/// binding is directory state an operator can change, so a reconnect can hand
/// this replica a different actor — and the work it authored under the old one
/// is then work the service's log already holds, under those ids, with other
/// bytes. That is exactly "cannot be replayed onto the service's log".
#[test]
fn work_that_cannot_be_replayed_ends_the_session_rather_than_reading_live() {
    let (mut driver, mut app) = live_session();
    type_into_run(&mut app, BASE_TEXT.len(), "a");
    type_into_run(&mut app, BASE_TEXT.len() + 1, "b");
    let held = app.local_operations_after(0);
    assert_eq!(held.len(), 2, "two keystrokes, two operations");

    // The log the service sends back holds this actor's first id with other
    // bytes, and the subject is now bound to a different actor — so the tail
    // this replica is holding can neither be acknowledged nor replayed.
    let conflicting = Operation {
        id: held[0].id.clone(),
        kind: OperationKind::InsertText {
            inline_id: StableId::parse(RUN_ID).expect("run id"),
            offset: 0,
            text: "SOMEBODY-ELSE".to_string(),
        },
        context: None,
    };
    driver.socket_closed("1006", "dropped");
    driver.begin(DOCUMENT_UUID, "Alice");
    driver
        .ingest(
            &mut app,
            &welcome_frame_as(1, &[conflicting], "editor", "actor-alice-2", 512),
        )
        .expect("the welcome itself is adopted");

    let status = driver.status(&app);
    assert_eq!(
        status.phase,
        Phase::Closed,
        "a session that can never send again must not read `live`"
    );
    assert!(!status.can_submit);
    let notice = status.notice.expect("an explanation");
    assert_eq!(notice.kind, "unsendable-work");
    assert!(!notice.resumable);
    assert!(
        notice.message.contains("2 change(s)"),
        "the user is told how much: {}",
        notice.message
    );
}

/// A non-resumable notice followed by a dropped socket says the session is
/// over, rather than leaving "Reconnecting…" on screen with nothing retrying.
///
/// Both halves of the bug are here. The *cause* was work this replica could
/// not replay: the notice said `resumable: false` while the phase stayed
/// `Live`, so the socket's own close then set `Reconnecting` — and the page,
/// reading a non-resumable notice, scheduled no retry. "Reconnecting…" was the
/// pill's last word for ever. Every non-resumable end now sets the phase where
/// it happens, and the socket's close leaves an ended session alone.
#[test]
fn a_non_resumable_notice_followed_by_a_dropped_socket_says_the_session_is_over() {
    let (mut driver, mut app) = live_session();
    driver
        .ingest(
            &mut app,
            &serde_json::json!({
                "type": "closed",
                "code": "forbidden",
                "message": "read access to this document was revoked",
            })
            .to_string(),
        )
        .expect("a close is a frame");
    assert_eq!(driver.status(&app).phase, Phase::Closed);

    // The socket the service closed then reports its close, which is the
    // ordinary order of events.
    driver.socket_closed("1006", "");
    let status = driver.status(&app);
    assert_eq!(status.phase, Phase::Closed);
    assert_eq!(
        status.notice.as_ref().map(|notice| notice.kind.clone()),
        Some("closed-forbidden".to_string()),
        "the service's own words survive the socket's close"
    );

    // And the same when the non-resumable notice was this client's own
    // conclusion rather than the service's: a `Closed` phase is not what makes
    // this work, the notice is.
    let mut driver = CollabDriver::default();
    let mut app = OpenDocApp::new_empty_document();
    driver.begin(DOCUMENT_UUID, "Alice");
    driver
        .ingest(
            &mut app,
            &serde_json::json!({
                "type": "welcome",
                "protocol_version": PROTOCOL_VERSION + 1,
                "document_uuid": DOCUMENT_UUID,
                "subject": "alice",
                "actor": "actor-alice",
                "role": "editor",
                "commit_seq": 0,
                "base_document": "",
                "operations": [],
                "peers": [],
                "max_operations_per_submit": 512,
            })
            .to_string(),
        )
        .expect_err("a protocol this client does not speak is refused");
    driver.socket_closed("1006", "");
    assert_eq!(
        driver.status(&app).phase,
        Phase::Closed,
        "nothing retries a protocol mismatch, so the pill must not say Reconnecting"
    );

    // And the shape the audit actually described: work that cannot be
    // replayed, then the socket going away.
    let (mut driver, mut app) = live_session();
    type_into_run(&mut app, BASE_TEXT.len(), "a");
    let held = app.local_operations_after(0);
    let conflicting = Operation {
        id: held[0].id.clone(),
        kind: OperationKind::InsertText {
            inline_id: StableId::parse(RUN_ID).expect("run id"),
            offset: 0,
            text: "SOMEBODY-ELSE".to_string(),
        },
        context: None,
    };
    driver.socket_closed("1006", "dropped");
    driver.begin(DOCUMENT_UUID, "Alice");
    driver
        .ingest(
            &mut app,
            &welcome_frame_as(1, &[conflicting], "editor", "actor-alice-2", 512),
        )
        .expect("the welcome itself is adopted");
    assert_eq!(driver.status(&app).phase, Phase::Closed);
    driver.socket_closed("1006", "and then the socket went");
    let status = driver.status(&app);
    assert_eq!(
        status.phase,
        Phase::Closed,
        "the socket's own close must not turn an ended session back into Reconnecting"
    );
    assert_eq!(
        status.notice.as_ref().map(|notice| notice.kind.clone()),
        Some("unsendable-work".to_string()),
        "and it must not overwrite the reason the session ended"
    );
}
