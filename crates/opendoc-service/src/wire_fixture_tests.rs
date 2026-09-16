//! The wire format, written down once, in bytes.
//!
//! `crates/opendoc-wasm` implements this protocol's client half without
//! depending on this crate, because this crate must stay out of the
//! WebAssembly dependency graph (ADR 0015, ADR 0018): a browser cannot link
//! `tokio` and a network daemon's tree has no business in the browser core. So
//! the frame types are restated there — the same trade `opendoc-api` makes for
//! `OpenDocServiceRole` and `opendoc-app` for `SERVICE_DOCUMENT_FORMAT`.
//!
//! A restated definition is only worth the test that compares it. This test
//! owns one direction of that comparison: every frame of the protocol,
//! serialized from *these* types, must equal
//! `crates/opendoc-service/wire/protocol-frames.json`. `opendoc-wasm`'s
//! `collab_tests` owns the other: it parses that same file with its own types
//! and checks the values it gets. A change on either side that the other did
//! not follow fails one of the two.
//!
//! The file is generated, never hand-edited. `OPENDOC_WRITE_WIRE_FIXTURE=1
//! cargo test --release -p opendoc-service wire_fixture` rewrites it, which is
//! the same shape as the command contract: Rust metadata is the source and the
//! checked-in artefact is the diff you review.

use crate::document::DEFAULT_MAX_OPERATIONS_PER_SUBMIT;
use crate::permission::Role;
use crate::protocol::{
    encode_document, ClientMessage, PeerView, ServerMessage, WireOperationId, PROTOCOL_VERSION,
};
use opendoc_core::{Block, BlockKind, Document, DocumentUuid, Inline, StableId};
use opendoc_merge::{ActorId, CausalContext, Operation, OperationId, OperationKind, VectorClock};

const FIXTURE: &str = include_str!("../wire/protocol-frames.json");
const FIXTURE_PATH: &str = "crates/opendoc-service/wire/protocol-frames.json";

/// A document with one run, so the base in the welcome is not empty. Fixed
/// ids and a fixed uuid: the fixture must be byte-stable.
fn fixture_base() -> Document {
    let mut document = Document::new("Wire fixture");
    document.uuid =
        DocumentUuid::parse("11111111-2222-3333-4444-555555555555").expect("a fixed uuid");
    document.blocks.push(Block {
        id: StableId::parse("blk-wire-0001").expect("block id"),
        kind: BlockKind::Paragraph,
        properties: Default::default(),
        content: vec![Inline::Text {
            id: StableId::parse("inl-wire-0001").expect("inline id"),
            text: "abcdefgh".to_string(),
            marks: Vec::new(),
        }],
    });
    document
}

fn fixture_operation(seq: u64, offset: usize, text: &str) -> Operation {
    let mut observed = VectorClock::default();
    if seq > 1 {
        observed.observe(&OperationId {
            actor: ActorId("actor-alice".to_string()),
            seq: seq - 1,
        });
    }
    Operation {
        id: OperationId {
            actor: ActorId("actor-alice".to_string()),
            seq,
        },
        kind: OperationKind::InsertText {
            inline_id: StableId::parse("inl-wire-0001").expect("inline id"),
            offset,
            text: text.to_string(),
        },
        context: Some(CausalContext {
            lamport: seq,
            observed,
        }),
    }
}

fn fixture_peer(subject: &str, actor: &str, role: Role, cursor: Option<&str>) -> PeerView {
    PeerView {
        subject: subject.to_string(),
        actor: ActorId(actor.to_string()),
        display_name: subject.to_string(),
        role,
        cursor_anchor: cursor.map(ToString::to_string),
        selection_anchor: Some("blk-wire-0001:1".to_string()),
        last_seen_ms: 1_700_000_000_000,
        connections: 1,
    }
}

/// Every variant of both enums, with every field populated.
///
/// Populated deliberately: a fixture with `None` in an optional field proves
/// nothing about the name that field serializes under.
fn frames() -> serde_json::Value {
    let server = vec![
        ServerMessage::Welcome {
            protocol_version: PROTOCOL_VERSION,
            document_uuid: "11111111-2222-3333-4444-555555555555".to_string(),
            subject: "alice".to_string(),
            actor: ActorId("actor-alice".to_string()),
            role: Role::Editor,
            commit_seq: 2,
            base_document: encode_document(&fixture_base()).expect("encoding the base"),
            operations: vec![fixture_operation(1, 0, "X"), fixture_operation(2, 1, "Y")],
            peers: vec![
                fixture_peer(
                    "alice",
                    "actor-alice",
                    Role::Editor,
                    Some("blk-wire-0001:3"),
                ),
                fixture_peer("bob", "actor-bob", Role::Viewer, None),
            ],
            max_operations_per_submit: DEFAULT_MAX_OPERATIONS_PER_SUBMIT,
        },
        ServerMessage::Accepted {
            batch_id: "batch-7".to_string(),
            commit_seq: 3,
            operation_ids: vec![
                WireOperationId {
                    actor: "actor-alice".to_string(),
                    seq: 3,
                },
                WireOperationId {
                    actor: "actor-alice".to_string(),
                    seq: 4,
                },
            ],
        },
        ServerMessage::Rejected {
            batch_id: "batch-8".to_string(),
            code: "forbidden".to_string(),
            message: "viewer may not write".to_string(),
        },
        ServerMessage::Committed {
            commit_seq: 4,
            subject: "bob".to_string(),
            actor: ActorId("actor-bob".to_string()),
            operations: vec![Operation {
                id: OperationId {
                    actor: ActorId("actor-bob".to_string()),
                    seq: 1,
                },
                kind: OperationKind::InsertText {
                    inline_id: StableId::parse("inl-wire-0001").expect("inline id"),
                    offset: 2,
                    text: "B".to_string(),
                },
                context: Some(CausalContext {
                    lamport: 3,
                    observed: VectorClock::default(),
                }),
            }],
        },
        ServerMessage::Presence {
            peers: vec![fixture_peer(
                "bob",
                "actor-bob",
                Role::Commenter,
                Some("blk-wire-0001:1"),
            )],
        },
        ServerMessage::Closed {
            code: "forbidden".to_string(),
            message: "read access was revoked".to_string(),
        },
        ServerMessage::Pong,
    ];
    let client = vec![
        ClientMessage::Submit {
            batch_id: "batch-9".to_string(),
            operations: vec![fixture_operation(5, 4, "Z")],
        },
        ClientMessage::Presence {
            display_name: Some("Alice".to_string()),
            cursor_anchor: Some("blk-wire-0001:5".to_string()),
            selection_anchor: Some("blk-wire-0001:2".to_string()),
        },
        ClientMessage::Ping,
    ];
    serde_json::json!({
        "protocol_version": PROTOCOL_VERSION,
        "default_max_operations_per_submit": DEFAULT_MAX_OPERATIONS_PER_SUBMIT,
        "server": server,
        "client": client,
    })
}

#[test]
fn the_checked_in_wire_fixture_is_what_these_types_serialize_to() {
    let expected = serde_json::to_string_pretty(&frames()).expect("serializing the frames") + "\n";
    if std::env::var("OPENDOC_WRITE_WIRE_FIXTURE").is_ok() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("wire")
            .join("protocol-frames.json");
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("creating wire/");
        std::fs::write(&path, &expected).expect("writing the fixture");
        return;
    }
    assert_eq!(
        FIXTURE, expected,
        "{FIXTURE_PATH} is stale. The wire format changed, and `opendoc-wasm`'s client \
         restates it (ADR 0018). Regenerate with \
         OPENDOC_WRITE_WIRE_FIXTURE=1 cargo test --release -p opendoc-service wire_fixture, \
         then make `opendoc-wasm`'s collab_tests pass against the new file."
    );
}

/// The version is in the fixture as a field of its own, not only inside the
/// welcome, so a client can check the number it restated without parsing a
/// frame.
#[test]
fn the_fixture_names_the_protocol_version_this_crate_speaks() {
    let value: serde_json::Value = serde_json::from_str(FIXTURE).expect("the fixture is JSON");
    assert_eq!(
        value["protocol_version"].as_u64(),
        Some(u64::from(PROTOCOL_VERSION))
    );
}

/// The submit cap is in the fixture twice on purpose: once as this crate's
/// default, and once inside the welcome, which is the field a client actually
/// reads. `opendoc-wasm` checks the second against the first, which is what
/// makes "the client chunks to the server's number" a checked statement rather
/// than a comment.
#[test]
fn the_fixture_names_the_submit_cap_the_welcome_carries() {
    let value: serde_json::Value = serde_json::from_str(FIXTURE).expect("the fixture is JSON");
    assert_eq!(
        value["default_max_operations_per_submit"].as_u64(),
        Some(DEFAULT_MAX_OPERATIONS_PER_SUBMIT as u64)
    );
    let welcome = value["server"]
        .as_array()
        .expect("the server frames")
        .iter()
        .find(|frame| frame["type"] == "welcome")
        .expect("a welcome frame");
    assert_eq!(
        welcome["max_operations_per_submit"].as_u64(),
        Some(DEFAULT_MAX_OPERATIONS_PER_SUBMIT as u64),
        "a client that cannot read the cap out of the welcome cannot chunk to it"
    );
}
