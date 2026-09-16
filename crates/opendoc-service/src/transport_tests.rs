//! The tests that matter: two clients, one server, one real socket.
//!
//! Everything here drives the service the way a browser would — an HTTP
//! session exchange, a WebSocket upgrade, JSON frames — against a service
//! bound to a real TCP port. Nothing reaches past the transport into the
//! document thread, because a service that is only correct when called
//! directly is not a service.

use crate::client::{DocumentSession, ServiceClient};
use crate::error::ServiceError;
use crate::permission::Role;
use crate::protocol::ServerMessage;
use crate::server::{serve, RunningServer};
use crate::service::OpenDocService;
use crate::test_support::{register_default_subjects, TempRoot, ALICE_KEY, BOB_KEY, CAROL_KEY};
use crate::Clock;
use opendoc_core::{Block, BlockKind, Document, Inline, StableId};
use opendoc_merge::{ActorId, CausalContext, Operation, OperationId, OperationKind, VectorClock};
use opendoc_store::LocalObjectStore;
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

/// Every wait in this file is bounded. A service bug that simply stops sending
/// a message must fail a test, not hang the suite until someone kills it.
const EVENT_TIMEOUT: Duration = Duration::from_secs(10);

const BLOCK_ID: &str = "blk-transport-0001";
const RUN_ID: &str = "inl-transport-0001";
const BASE_TEXT: &str = "abcdefgh";

struct Harness {
    service: Arc<OpenDocService<LocalObjectStore>>,
    server: RunningServer,
}

impl Harness {
    async fn start(root: &TempRoot) -> Self {
        let service = Arc::new(OpenDocService::new(root.store(), Clock::system()));
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

fn block_id() -> StableId {
    StableId::parse(BLOCK_ID).unwrap()
}

fn run_id() -> StableId {
    StableId::parse(RUN_ID).unwrap()
}

/// The one operation that gives the document something to edit.
fn seed_block() -> OperationKind {
    OperationKind::InsertBlock {
        position: opendoc_core::InsertPosition::Last,
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

/// One server event, or a failed test.
async fn next_event(session: &mut DocumentSession) -> ServerMessage {
    tokio::time::timeout(EVENT_TIMEOUT, session.next_event())
        .await
        .expect("a server event within the timeout")
        .expect("reading a server event")
}

/// Reads events until the session reaches `commit_seq`, failing loudly on a
/// rejection rather than waiting for a commit that will never come.
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

/// Reads events until a `Rejected` arrives, returning its code and message.
async fn next_rejection(session: &mut DocumentSession) -> (String, String) {
    loop {
        match next_event(session).await {
            ServerMessage::Rejected { code, message, .. } => return (code, message),
            ServerMessage::Accepted { .. } => panic!("the batch was accepted, not rejected"),
            _ => {}
        }
    }
}

/// Reads presence updates until one satisfies `ready`.
async fn next_presence_where(
    session: &mut DocumentSession,
    ready: impl Fn(&[crate::protocol::PeerView]) -> bool,
) -> Vec<crate::protocol::PeerView> {
    loop {
        if let ServerMessage::Presence { peers } = next_event(session).await {
            if ready(&peers) {
                return peers;
            }
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
    let document_uuid = alice.create_document("Shared").await.expect("create");
    alice
        .set_grant(&document_uuid, "bob", Some(Role::Editor))
        .await
        .expect("granting bob edit access");

    let mut session = alice
        .connect(&document_uuid, "Alice")
        .await
        .expect("connect");
    let seed = session.author(seed_block());
    session.submit("seed", vec![seed]).await.expect("submit");
    drain_to(&mut session, 1).await;
    session.close().await;
    (alice, bob, document_uuid)
}

// ---------------------------------------------------------------------------
// Convergence over the real transport.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn two_clients_editing_concurrently_over_the_real_transport_converge_byte_identically() {
    let root = TempRoot::new("converge");
    let harness = Harness::start(&root).await;
    let (alice, bob, document_uuid) = seeded_document(&harness).await;

    let mut alice_session = alice.connect(&document_uuid, "Alice").await.unwrap();
    let mut bob_session = bob.connect(&document_uuid, "Bob").await.unwrap();
    assert_eq!(alice_session.commit_seq(), 1);
    assert_eq!(bob_session.commit_seq(), 1);

    // A deterministic schedule, so a failure is reproducible. Each round both
    // clients author against the state they can see *before* either round's
    // commits land, which is what makes the two batches genuinely concurrent
    // rather than causally ordered.
    let mut seed = 0x5eed_1234u64;
    let mut next = move || {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (seed >> 33) as usize
    };

    let rounds = 8u64;
    for round in 0..rounds {
        let alice_text = run_text(&alice_session.document().unwrap());
        let bob_text = run_text(&bob_session.document().unwrap());

        let alice_batch = if round % 3 == 2 && alice_text.chars().count() > 3 {
            let length = alice_text.chars().count();
            let start = next() % (length - 2);
            vec![OperationKind::DeleteText {
                inline_id: run_id(),
                start,
                end: start + 1 + next() % 2,
            }]
        } else {
            let offset = next() % (alice_text.chars().count() + 1);
            vec![
                OperationKind::InsertText {
                    inline_id: run_id(),
                    offset,
                    text: format!("A{round}"),
                },
                OperationKind::SetDocumentTitle {
                    title: format!("alice round {round}"),
                },
            ]
        };
        let bob_batch = if round % 3 == 1 && bob_text.chars().count() > 3 {
            let length = bob_text.chars().count();
            let start = next() % (length - 2);
            vec![OperationKind::DeleteText {
                inline_id: run_id(),
                start,
                end: start + 1 + next() % 2,
            }]
        } else {
            let offset = next() % (bob_text.chars().count() + 1);
            vec![
                OperationKind::InsertText {
                    inline_id: run_id(),
                    offset,
                    text: format!("B{round}"),
                },
                OperationKind::SetDocumentTitle {
                    title: format!("bob round {round}"),
                },
            ]
        };

        let alice_ops = alice_session.author_batch(alice_batch);
        let bob_ops = bob_session.author_batch(bob_batch);
        alice_session
            .submit(&format!("alice-{round}"), alice_ops)
            .await
            .unwrap();
        bob_session
            .submit(&format!("bob-{round}"), bob_ops)
            .await
            .unwrap();

        let target = 1 + 2 * (round + 1);
        drain_to(&mut alice_session, target).await;
        drain_to(&mut bob_session, target).await;
    }

    let alice_bytes = alice_session.document_bytes().unwrap();
    let bob_bytes = bob_session.document_bytes().unwrap();
    assert_eq!(
        alice_bytes, bob_bytes,
        "two replicas that received the same commits must encode identically\nalice: {:?}\nbob:   {:?}",
        run_text(&alice_session.document().unwrap()),
        run_text(&bob_session.document().unwrap())
    );

    // And the server's own materialised document is the same document, not a
    // third opinion.
    let handle = harness.service.document(&document_uuid).unwrap();
    let server_document = handle.snapshot().await.unwrap();
    let server_bytes = opendoc_format::encode_canonical_cbor(&server_document).unwrap();
    assert_eq!(
        server_bytes, alice_bytes,
        "the server's document must equal the clients' document"
    );

    // The run must actually have been edited, or the assertion above is
    // comparing two copies of the base.
    let text = run_text(&server_document);
    assert_ne!(text, BASE_TEXT, "no edit survived the round trip");
    // Concurrent deletes legitimately remove some of the markers, so the
    // assertion is that both actors' work is well represented, not that any
    // particular insert survived.
    let alice_marks = text.matches('A').count();
    let bob_marks = text.matches('B').count();
    assert!(
        alice_marks >= 3 && bob_marks >= 3,
        "both actors' work must survive: {alice_marks} from alice and {bob_marks} from bob in {text}"
    );
    assert_eq!(
        alice_session.operations().len(),
        bob_session.operations().len()
    );

    alice_session.close().await;
    bob_session.close().await;
    harness.stop().await;
}

#[tokio::test]
async fn a_late_joiner_reaches_the_same_bytes_from_the_welcome_alone() {
    let root = TempRoot::new("late-join");
    let harness = Harness::start(&root).await;
    let (alice, bob, document_uuid) = seeded_document(&harness).await;

    let mut alice_session = alice.connect(&document_uuid, "Alice").await.unwrap();
    for round in 0..4u64 {
        let ops = alice_session.author_batch(vec![OperationKind::InsertText {
            inline_id: run_id(),
            offset: round as usize,
            text: format!("{round}"),
        }]);
        alice_session.submit("edit", ops).await.unwrap();
        drain_to(&mut alice_session, 2 + round).await;
    }
    let expected = alice_session.document_bytes().unwrap();

    // bob has seen none of this; the welcome has to be enough.
    let bob_session = bob.connect(&document_uuid, "Bob").await.unwrap();
    assert_eq!(bob_session.commit_seq(), alice_session.commit_seq());
    assert_eq!(bob_session.document_bytes().unwrap(), expected);

    alice_session.close().await;
    bob_session.close().await;
    harness.stop().await;
}

// ---------------------------------------------------------------------------
// Durability.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn accepted_operations_survive_a_full_service_restart() {
    let root = TempRoot::new("restart");
    let expected = {
        let harness = Harness::start(&root).await;
        let (alice, _bob, document_uuid) = seeded_document(&harness).await;
        let mut session = alice.connect(&document_uuid, "Alice").await.unwrap();
        let ops = session.author_batch(vec![
            OperationKind::InsertText {
                inline_id: run_id(),
                offset: 3,
                text: "-durable-".to_string(),
            },
            OperationKind::SetDocumentTitle {
                title: "Survivor".to_string(),
            },
        ]);
        session.submit("edit", ops).await.unwrap();
        drain_to(&mut session, 2).await;
        let bytes = session.document_bytes().unwrap();
        session.close().await;
        harness.stop().await;
        (document_uuid, bytes)
    };
    let (document_uuid, expected_bytes) = expected;

    // A brand new service object, a new server, a new session table, a new
    // permission cache. Only the object store crosses the boundary.
    let harness = Harness::start(&root).await;
    let alice = ServiceClient::open_session(harness.address(), "alice", ALICE_KEY)
        .await
        .unwrap();
    let session = alice.connect(&document_uuid, "Alice").await.unwrap();
    assert_eq!(session.commit_seq(), 2);
    assert_eq!(
        session.document_bytes().unwrap(),
        expected_bytes,
        "a restarted service must hand back byte-identical state"
    );
    assert!(run_text(&session.document().unwrap()).contains("-durable-"));

    session.close().await;
    harness.stop().await;
}

#[tokio::test]
async fn a_grant_made_before_a_restart_still_holds_after_it() {
    let root = TempRoot::new("restart-grants");
    let document_uuid = {
        let harness = Harness::start(&root).await;
        let (_alice, _bob, document_uuid) = seeded_document(&harness).await;
        harness.stop().await;
        document_uuid
    };

    let harness = Harness::start(&root).await;
    let bob = ServiceClient::open_session(harness.address(), "bob", BOB_KEY)
        .await
        .unwrap();
    let view = bob.describe_document(&document_uuid).await.unwrap();
    assert_eq!(view.role, Role::Editor);

    let carol = ServiceClient::open_session(harness.address(), "carol", CAROL_KEY)
        .await
        .unwrap();
    assert!(matches!(
        carol.describe_document(&document_uuid).await,
        Err(ServiceError::Forbidden(_))
    ));
    harness.stop().await;
}

// ---------------------------------------------------------------------------
// Authentication and authorization, over the wire.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn an_unauthenticated_caller_is_refused_by_http_and_by_the_socket() {
    let root = TempRoot::new("unauth");
    let harness = Harness::start(&root).await;
    let (alice, _bob, document_uuid) = seeded_document(&harness).await;

    assert!(matches!(
        ServiceClient::open_session(harness.address(), "alice", "wrong-api-key-000000").await,
        Err(ServiceError::Unauthenticated(_))
    ));

    // A token the service has since forgotten must stop working everywhere,
    // not just on the next sign-in.
    harness
        .service
        .identity()
        .close_session(alice.token())
        .unwrap();
    assert!(matches!(
        alice.describe_document(&document_uuid).await,
        Err(ServiceError::Unauthenticated(_))
    ));
    let refusal = alice
        .connect(&document_uuid, "Ghost")
        .await
        .err()
        .expect("a revoked token must not open a socket");
    // Refused before the upgrade, not after: the caller gets a status it can
    // read rather than a socket that closes for reasons it has to guess at.
    assert!(
        matches!(refusal, ServiceError::Unauthenticated(_))
            && refusal.message().contains("upgrade refused"),
        "expected an HTTP refusal of the upgrade, got {refusal}"
    );
    harness.stop().await;
}

#[tokio::test]
async fn a_subject_with_no_grant_cannot_open_the_document_socket() {
    let root = TempRoot::new("no-grant");
    let harness = Harness::start(&root).await;
    let (_alice, _bob, document_uuid) = seeded_document(&harness).await;

    let carol = ServiceClient::open_session(harness.address(), "carol", CAROL_KEY)
        .await
        .unwrap();
    assert!(matches!(
        carol.describe_document(&document_uuid).await,
        Err(ServiceError::Forbidden(_))
    ));
    let refusal = carol
        .connect(&document_uuid, "Carol")
        .await
        .err()
        .expect("a subject with no grant must not get a socket");
    assert!(
        matches!(refusal, ServiceError::Forbidden(_))
            && refusal.message().contains("upgrade refused"),
        "the upgrade itself must be refused, not the session after it: {refusal}"
    );
    harness.stop().await;
}

#[tokio::test]
async fn a_viewer_receives_commits_but_cannot_write() {
    let root = TempRoot::new("viewer");
    let harness = Harness::start(&root).await;
    let (alice, _bob, document_uuid) = seeded_document(&harness).await;
    alice
        .set_grant(&document_uuid, "carol", Some(Role::Viewer))
        .await
        .unwrap();

    let carol = ServiceClient::open_session(harness.address(), "carol", CAROL_KEY)
        .await
        .unwrap();
    let mut carol_session = carol.connect(&document_uuid, "Carol").await.unwrap();
    assert_eq!(carol_session.role(), Role::Viewer);

    let attempt = carol_session.author_batch(vec![OperationKind::SetDocumentTitle {
        title: "carol was here".to_string(),
    }]);
    carol_session.submit("nope", attempt).await.unwrap();
    let (code, _message) = next_rejection(&mut carol_session).await;
    assert_eq!(code, "forbidden");

    // The refusal left the document alone.
    let handle = harness.service.document(&document_uuid).unwrap();
    assert_eq!(handle.status().await.unwrap().commit_seq, 1);

    // And the viewer still receives an editor's work.
    let mut alice_session = alice.connect(&document_uuid, "Alice").await.unwrap();
    let ops = alice_session.author_batch(vec![OperationKind::SetDocumentTitle {
        title: "alice wrote".to_string(),
    }]);
    alice_session.submit("yes", ops).await.unwrap();
    drain_to(&mut carol_session, 2).await;
    assert_eq!(carol_session.document().unwrap().title, "alice wrote");

    alice_session.close().await;
    carol_session.close().await;
    harness.stop().await;
}

#[tokio::test]
async fn revoking_read_closes_a_connection_that_is_already_open() {
    let root = TempRoot::new("revoke-live");
    let harness = Harness::start(&root).await;
    let (alice, bob, document_uuid) = seeded_document(&harness).await;
    let mut bob_session = bob.connect(&document_uuid, "Bob").await.unwrap();

    alice.set_grant(&document_uuid, "bob", None).await.unwrap();

    // Revocation that only bites on reconnect is not revocation.
    let closed = loop {
        if let ServerMessage::Closed { code, .. } = next_event(&mut bob_session).await {
            break code;
        }
    };
    assert_eq!(closed, "forbidden");
    let refusal = bob
        .connect(&document_uuid, "Bob")
        .await
        .err()
        .expect("a revoked subject must not reconnect");
    assert!(refusal.message().contains("upgrade refused"), "{refusal}");
    harness.stop().await;
}

#[tokio::test]
async fn an_editor_cannot_change_grants() {
    let root = TempRoot::new("editor-share");
    let harness = Harness::start(&root).await;
    let (_alice, bob, document_uuid) = seeded_document(&harness).await;

    assert!(matches!(
        bob.set_grant(&document_uuid, "carol", Some(Role::Owner))
            .await,
        Err(ServiceError::Forbidden(_))
    ));
    assert!(matches!(
        bob.list_grants(&document_uuid).await,
        Err(ServiceError::Forbidden(_))
    ));
    harness.stop().await;
}

// ---------------------------------------------------------------------------
// What the server refuses to believe about an operation.
// ---------------------------------------------------------------------------

async fn rejected_submission(
    harness: &Harness,
    build: impl FnOnce(&DocumentSession) -> Vec<Operation>,
) -> (String, String) {
    let (_alice, bob, document_uuid) = seeded_document(harness).await;
    let mut session = bob.connect(&document_uuid, "Bob").await.unwrap();
    let operations = build(&session);
    session.submit("bad", operations).await.unwrap();
    next_rejection(&mut session).await
}

#[tokio::test]
async fn an_operation_authored_as_another_actor_is_refused() {
    let root = TempRoot::new("forged-actor");
    let harness = Harness::start(&root).await;
    let (code, message) = rejected_submission(&harness, |_session| {
        vec![Operation::in_context(
            OperationId {
                // bob's session, alice's actor id.
                actor: ActorId("actor-alice".to_string()),
                seq: 2,
            },
            OperationKind::SetDocumentTitle {
                title: "signed, alice".to_string(),
            },
            CausalContext::default(),
        )]
    })
    .await;
    assert_eq!(code, "forbidden", "{message}");
    assert!(message.contains("actor-alice"), "{message}");
    harness.stop().await;
}

#[tokio::test]
async fn a_forged_lamport_timestamp_is_refused() {
    let root = TempRoot::new("forged-lamport");
    let harness = Harness::start(&root).await;
    let (code, message) = rejected_submission(&harness, |session| {
        // Without this check, one client sets `lamport: u64::MAX` and wins
        // every last-writer-wins contest on the document for ever.
        vec![Operation::in_context(
            OperationId {
                actor: session.actor().clone(),
                seq: 1,
            },
            OperationKind::SetDocumentTitle {
                title: "mine for ever".to_string(),
            },
            CausalContext {
                lamport: u64::MAX,
                observed: VectorClock::new(),
            },
        )]
    })
    .await;
    assert_eq!(code, "bad-request", "{message}");
    assert!(message.contains("Lamport"), "{message}");
    harness.stop().await;
}

#[tokio::test]
async fn an_operation_claiming_a_causal_past_the_log_does_not_have_is_refused() {
    let root = TempRoot::new("forged-clock");
    let harness = Harness::start(&root).await;
    let (code, message) = rejected_submission(&harness, |session| {
        let mut observed = VectorClock::new();
        observed.observe(&OperationId {
            actor: ActorId("actor-alice".to_string()),
            seq: 99,
        });
        vec![Operation::in_context(
            OperationId {
                actor: session.actor().clone(),
                seq: 1,
            },
            OperationKind::SetDocumentTitle {
                title: "from the future".to_string(),
            },
            CausalContext {
                lamport: 100,
                observed,
            },
        )]
    })
    .await;
    assert_eq!(code, "bad-request", "{message}");
    assert!(message.contains("not in the log"), "{message}");
    harness.stop().await;
}

#[tokio::test]
async fn an_out_of_sequence_operation_is_refused() {
    let root = TempRoot::new("sequence-gap");
    let harness = Harness::start(&root).await;
    let (code, message) = rejected_submission(&harness, |session| {
        // A gap would make `VectorClock::observed` — which reads "seq >= n" as
        // "every operation up to n" — false.
        vec![Operation::in_context(
            OperationId {
                actor: session.actor().clone(),
                seq: 7,
            },
            OperationKind::SetDocumentTitle {
                title: "skipping ahead".to_string(),
            },
            CausalContext::default(),
        )]
    })
    .await;
    assert_eq!(code, "conflict", "{message}");
    assert!(message.contains("out of sequence"), "{message}");
    harness.stop().await;
}

#[tokio::test]
async fn one_operation_id_cannot_be_reused_for_a_different_payload() {
    let root = TempRoot::new("rewrite");
    let harness = Harness::start(&root).await;
    let (_alice, bob, document_uuid) = seeded_document(&harness).await;
    let mut session = bob.connect(&document_uuid, "Bob").await.unwrap();

    let first = session.author_batch(vec![OperationKind::SetDocumentTitle {
        title: "first".to_string(),
    }]);
    session.submit("one", first.clone()).await.unwrap();
    drain_to(&mut session, 2).await;

    let rewritten = vec![Operation::in_context(
        first[0].id.clone(),
        OperationKind::SetDocumentTitle {
            title: "rewritten".to_string(),
        },
        first[0].context.clone().unwrap_or_default(),
    )];
    session.submit("two", rewritten).await.unwrap();
    let (code, message) = next_rejection(&mut session).await;
    assert_eq!(code, "conflict", "{message}");
    assert_eq!(session.document().unwrap().title, "first");

    session.close().await;
    harness.stop().await;
}

#[tokio::test]
async fn an_exact_replay_is_acknowledged_without_committing_again() {
    let root = TempRoot::new("replay");
    let harness = Harness::start(&root).await;
    let (_alice, bob, document_uuid) = seeded_document(&harness).await;
    let mut session = bob.connect(&document_uuid, "Bob").await.unwrap();

    let batch = session.author_batch(vec![OperationKind::SetDocumentTitle {
        title: "once".to_string(),
    }]);
    session.submit("one", batch.clone()).await.unwrap();
    drain_to(&mut session, 2).await;

    // A retry after a lost acknowledgement must be a no-op, not a duplicate.
    session.submit("one-again", batch).await.unwrap();
    let accepted = loop {
        match next_event(&mut session).await {
            ServerMessage::Accepted {
                batch_id,
                commit_seq,
                operation_ids,
            } if batch_id == "one-again" => break (commit_seq, operation_ids),
            ServerMessage::Rejected { code, message, .. } => {
                panic!("a replay must not be rejected: {code}: {message}")
            }
            _ => {}
        }
    };
    assert_eq!(accepted.0, 2, "the replay must not advance the commit");
    assert!(accepted.1.is_empty(), "the replay must store nothing");

    let handle = harness.service.document(&document_uuid).unwrap();
    assert_eq!(handle.status().await.unwrap().commit_seq, 2);
    assert_eq!(session.operations().len(), 2);

    session.close().await;
    harness.stop().await;
}

#[tokio::test]
async fn one_submit_cannot_carry_the_same_operation_id_twice() {
    let root = TempRoot::new("dup-in-batch");
    let harness = Harness::start(&root).await;
    let (code, message) = rejected_submission(&harness, |session| {
        let one = Operation::in_context(
            OperationId {
                actor: session.actor().clone(),
                seq: 1,
            },
            OperationKind::SetDocumentTitle {
                title: "twice".to_string(),
            },
            CausalContext::default(),
        );
        vec![one.clone(), one]
    })
    .await;
    assert_eq!(code, "bad-request", "{message}");
    assert!(message.contains("twice in one submit"), "{message}");
    harness.stop().await;
}

// ---------------------------------------------------------------------------
// Presence (CO-18).
// ---------------------------------------------------------------------------

#[tokio::test]
async fn presence_reports_every_peer_with_the_role_the_server_holds() {
    let root = TempRoot::new("presence");
    let harness = Harness::start(&root).await;
    let (alice, bob, document_uuid) = seeded_document(&harness).await;
    alice
        .set_grant(&document_uuid, "carol", Some(Role::Viewer))
        .await
        .unwrap();
    let carol = ServiceClient::open_session(harness.address(), "carol", CAROL_KEY)
        .await
        .unwrap();

    let mut alice_session = alice.connect(&document_uuid, "Alice A").await.unwrap();
    let _bob_session = bob.connect(&document_uuid, "Bob B").await.unwrap();
    let _carol_session = carol.connect(&document_uuid, "Carol C").await.unwrap();

    // alice sees presence updates arrive as the others join.
    let peers = next_presence_where(&mut alice_session, |peers| peers.len() == 3).await;
    let roles: BTreeMap<String, Role> = peers
        .iter()
        .map(|peer| (peer.subject.clone(), peer.role))
        .collect();
    assert_eq!(roles.get("alice"), Some(&Role::Owner));
    assert_eq!(roles.get("bob"), Some(&Role::Editor));
    assert_eq!(roles.get("carol"), Some(&Role::Viewer));

    let names: BTreeMap<String, String> = peers
        .iter()
        .map(|peer| (peer.subject.clone(), peer.display_name.clone()))
        .collect();
    assert_eq!(names.get("bob").map(String::as_str), Some("Bob B"));
    // Actors are server state and travel with the peer, so a client can map a
    // cursor to the operations that produced it.
    assert!(peers
        .iter()
        .any(|peer| peer.actor == ActorId("actor-carol".to_string())));

    harness.stop().await;
}

#[tokio::test]
async fn a_cursor_announcement_reaches_the_other_peers() {
    let root = TempRoot::new("presence-cursor");
    let harness = Harness::start(&root).await;
    let (alice, bob, document_uuid) = seeded_document(&harness).await;
    let mut alice_session = alice.connect(&document_uuid, "Alice").await.unwrap();
    let mut bob_session = bob.connect(&document_uuid, "Bob").await.unwrap();

    bob_session
        .announce_presence(Some("Bob Bobson"), Some(RUN_ID), None)
        .await
        .unwrap();

    let peers = next_presence_where(&mut alice_session, |peers| {
        peers
            .iter()
            .any(|peer| peer.subject == "bob" && peer.cursor_anchor.is_some())
    })
    .await;
    let bob_peer = peers.iter().find(|peer| peer.subject == "bob").unwrap();
    assert_eq!(bob_peer.cursor_anchor.as_deref(), Some(RUN_ID));
    assert_eq!(bob_peer.display_name, "Bob Bobson");

    alice_session.close().await;
    bob_session.close().await;
    harness.stop().await;
}

#[tokio::test]
async fn two_connections_from_one_subject_are_one_peer() {
    let root = TempRoot::new("presence-tabs");
    let harness = Harness::start(&root).await;
    let (alice, bob, document_uuid) = seeded_document(&harness).await;
    let mut alice_session = alice.connect(&document_uuid, "Alice").await.unwrap();
    let _bob_one = bob.connect(&document_uuid, "Bob tab one").await.unwrap();
    let _bob_two = bob.connect(&document_uuid, "Bob tab two").await.unwrap();

    let peers = next_presence_where(&mut alice_session, |peers| {
        peers.iter().any(|peer| peer.connections == 2)
    })
    .await;
    assert_eq!(peers.len(), 2, "two people, three sockets");
    let bob_peer = peers.iter().find(|peer| peer.subject == "bob").unwrap();
    assert_eq!(bob_peer.connections, 2);

    harness.stop().await;
}

#[tokio::test]
async fn a_departing_peer_leaves_the_presence_list() {
    let root = TempRoot::new("presence-leave");
    let harness = Harness::start(&root).await;
    let (alice, bob, document_uuid) = seeded_document(&harness).await;
    let mut alice_session = alice.connect(&document_uuid, "Alice").await.unwrap();
    let bob_session = bob.connect(&document_uuid, "Bob").await.unwrap();

    next_presence_where(&mut alice_session, |peers| peers.len() == 2).await;
    bob_session.close().await;
    let peers = next_presence_where(&mut alice_session, |peers| peers.len() == 1).await;
    assert_eq!(peers[0].subject, "alice");
    harness.stop().await;
}

// ---------------------------------------------------------------------------
// The HTTP surface.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_document_view_reports_the_callers_own_role_and_the_durable_head() {
    let root = TempRoot::new("view");
    let harness = Harness::start(&root).await;
    let (alice, bob, document_uuid) = seeded_document(&harness).await;

    let alice_view = alice.describe_document(&document_uuid).await.unwrap();
    assert_eq!(alice_view.role, Role::Owner);
    assert_eq!(alice_view.commit_seq, 1);
    assert!(alice_view
        .head
        .as_deref()
        .is_some_and(|head| head.starts_with("sha256:")));

    let bob_view = bob.describe_document(&document_uuid).await.unwrap();
    assert_eq!(bob_view.role, Role::Editor);
    assert_eq!(bob_view.head, alice_view.head);
    harness.stop().await;
}

#[tokio::test]
async fn an_owner_can_list_and_change_grants() {
    let root = TempRoot::new("grants-http");
    let harness = Harness::start(&root).await;
    let (alice, _bob, document_uuid) = seeded_document(&harness).await;

    let grants = alice.list_grants(&document_uuid).await.unwrap();
    let held: BTreeMap<String, Role> = grants
        .into_iter()
        .map(|grant| (grant.subject, grant.role))
        .collect();
    assert_eq!(held.get("alice"), Some(&Role::Owner));
    assert_eq!(held.get("bob"), Some(&Role::Editor));
    let initial_audit = alice.list_grant_audit(&document_uuid).await.unwrap();
    assert_eq!(initial_audit.len(), 2);
    assert_eq!(initial_audit[0].target_subject, "bob");
    assert_eq!(initial_audit[0].role, Some(Role::Editor));
    assert_eq!(initial_audit[1].target_subject, "alice");
    assert_eq!(initial_audit[1].role, Some(Role::Owner));

    alice
        .set_grant(&document_uuid, "bob", Some(Role::Commenter))
        .await
        .unwrap();
    let downgraded = alice.list_grants(&document_uuid).await.unwrap();
    assert!(downgraded
        .iter()
        .any(|grant| grant.subject == "bob" && grant.role == Role::Commenter));
    let audit = alice.list_grant_audit(&document_uuid).await.unwrap();
    assert_eq!(audit[0].actor_subject, "alice");
    assert_eq!(audit[0].target_subject, "bob");
    assert_eq!(audit[0].previous_role, Some(Role::Editor));
    assert_eq!(audit[0].role, Some(Role::Commenter));
    harness.stop().await;
}

#[tokio::test]
async fn a_document_nobody_may_read_is_indistinguishable_from_one_that_is_missing() {
    let root = TempRoot::new("opaque");
    let harness = Harness::start(&root).await;
    let (_alice, _bob, document_uuid) = seeded_document(&harness).await;
    let carol = ServiceClient::open_session(harness.address(), "carol", CAROL_KEY)
        .await
        .unwrap();

    let existing = carol.describe_document(&document_uuid).await.unwrap_err();
    let missing = carol
        .describe_document("doc-0000000000000000-0000000000000000")
        .await
        .unwrap_err();
    assert_eq!(
        existing.code(),
        missing.code(),
        "an unauthorized caller must not be able to probe for document ids"
    );
    harness.stop().await;
}

// ---------------------------------------------------------------------------
// Randomised convergence, over the transport, with three writers.
// ---------------------------------------------------------------------------

/// `opendoc-merge` proves convergence in process over 2,000 seeds. That is the
/// right place for it and this is not a second copy: what this asserts is that
/// nothing *between* the replicas — the socket, the JSON encoding, the commit
/// serializer, the fanout, the welcome that bootstraps a late joiner — loses
/// or reorders an operation in a way the in-process proof would not catch.
#[tokio::test]
async fn randomised_concurrent_sessions_converge_byte_identically_over_the_transport() {
    let root = TempRoot::new("converge-fuzz");
    let harness = Harness::start(&root).await;
    let alice = ServiceClient::open_session(harness.address(), "alice", ALICE_KEY)
        .await
        .unwrap();
    let bob = ServiceClient::open_session(harness.address(), "bob", BOB_KEY)
        .await
        .unwrap();
    let carol = ServiceClient::open_session(harness.address(), "carol", CAROL_KEY)
        .await
        .unwrap();

    let seeds = 24u64;
    let rounds = 4u64;
    let mut seeds_that_changed_the_document = 0u64;

    for seed in 0..seeds {
        let document_uuid = alice
            .create_document(&format!("Fuzz {seed}"))
            .await
            .unwrap();
        for subject in ["bob", "carol"] {
            alice
                .set_grant(&document_uuid, subject, Some(Role::Editor))
                .await
                .unwrap();
        }

        let mut sessions = vec![
            alice.connect(&document_uuid, "Alice").await.unwrap(),
            bob.connect(&document_uuid, "Bob").await.unwrap(),
            carol.connect(&document_uuid, "Carol").await.unwrap(),
        ];
        let seed_ops = sessions[0].author_batch(vec![seed_block()]);
        sessions[0].submit("seed", seed_ops).await.unwrap();
        for session in &mut sessions {
            drain_to(session, 1).await;
        }

        let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(1);
        let mut next = move || {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (state >> 33) as usize
        };

        let mut commit_seq = 1u64;
        for round in 0..rounds {
            // Every client authors before any client's batch lands, so the
            // three batches are genuinely concurrent with each other.
            let mut batches = Vec::new();
            for (index, session) in sessions.iter().enumerate() {
                let text = run_text(&session.document().unwrap());
                let length = text.chars().count();
                let choice = next() % 4;
                let kinds = if choice == 0 && length > 3 {
                    let start = next() % (length - 2);
                    vec![OperationKind::DeleteText {
                        inline_id: run_id(),
                        start,
                        end: start + 1 + next() % 2,
                    }]
                } else if choice == 1 {
                    vec![OperationKind::SetDocumentTitle {
                        title: format!("client {index} round {round}"),
                    }]
                } else {
                    let offset = next() % (length + 1);
                    vec![OperationKind::InsertText {
                        inline_id: run_id(),
                        offset,
                        text: format!("{}{round}", (b'P' + index as u8) as char),
                    }]
                };
                batches.push(session.author_batch(kinds));
            }
            for (index, session) in sessions.iter_mut().enumerate() {
                session
                    .submit(&format!("r{round}c{index}"), batches[index].clone())
                    .await
                    .unwrap();
            }
            commit_seq += sessions.len() as u64;
            for session in &mut sessions {
                drain_to(session, commit_seq).await;
            }
        }

        let expected = sessions[0].document_bytes().unwrap();
        for (index, session) in sessions.iter().enumerate() {
            assert_eq!(
                session.document_bytes().unwrap(),
                expected,
                "seed {seed}: client {index} diverged; its run reads {:?} and client 0's reads {:?}",
                run_text(&session.document().unwrap()),
                run_text(&sessions[0].document().unwrap())
            );
        }

        // A late joiner rebuilt from the welcome alone must land on the same
        // bytes as the clients that watched every commit arrive.
        let latecomer = carol.connect(&document_uuid, "Carol late").await.unwrap();
        assert_eq!(
            latecomer.document_bytes().unwrap(),
            expected,
            "seed {seed}: a client bootstrapped from the welcome diverged"
        );

        let server_document = harness
            .service
            .document(&document_uuid)
            .unwrap()
            .snapshot()
            .await
            .unwrap();
        assert_eq!(
            opendoc_format::encode_canonical_cbor(&server_document).unwrap(),
            expected,
            "seed {seed}: the server holds a third opinion"
        );

        if run_text(&server_document) != BASE_TEXT {
            seeds_that_changed_the_document += 1;
        }

        latecomer.close().await;
        for session in sessions {
            session.close().await;
        }
    }

    // Without this the whole loop could pass by never editing anything.
    assert!(
        seeds_that_changed_the_document * 10 >= seeds * 9,
        "only {seeds_that_changed_the_document} of {seeds} seeds changed the document"
    );
    harness.stop().await;
}
