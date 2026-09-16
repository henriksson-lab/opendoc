//! The registry of running documents: what it stops, and what it must not.
//!
//! One document is one thread, and the whole design rests on there being
//! exactly one for a given branch head. So a thread may only be stopped when
//! nothing can still be talking to it, and these tests are written to fail if
//! that ever stops being true — not by counting entries in a map, which would
//! be green for a service that had just started a second writer, but by
//! putting two edits through the document across a reclamation pass and
//! insisting they land on one head.

use crate::clock::{Clock, ManualClock};
use crate::document::DocumentHandle;
use crate::log::DocumentLog;
use crate::permission::Role;
use crate::service::OpenDocService;
use crate::test_support::{insert_text, seeded_document, TempRoot};
use opendoc_core::{Document, Inline, StableId};
use opendoc_merge::{ActorId, CausalContext, Operation, OperationId, OperationKind};
use opendoc_store::LocalObjectStore;
use tokio::sync::mpsc;

fn service(clock: Clock, root: &TempRoot) -> OpenDocService<LocalObjectStore> {
    OpenDocService::new(root.store(), clock)
}

/// A document in the store with one text run to aim operations at, owned by
/// alice. Deliberately written through `DocumentLog` rather than
/// `create_document`: a service-created document is an empty paragraph with no
/// inline run, and these tests need something to insert *into*.
fn seeded(service: &OpenDocService<LocalObjectStore>) -> (String, StableId) {
    let (document, _block, inline) = seeded_document("Registry");
    let document_uuid = document.uuid.as_str().to_string();
    DocumentLog::create(service.repository(), document).expect("the genesis commit");
    service
        .permissions()
        .seed_owner(&document_uuid, "alice")
        .expect("alice owns it");
    (document_uuid, inline)
}

fn operation(actor: &str, seq: u64, kind: OperationKind, observed: &[Operation]) -> Operation {
    Operation::in_context(
        OperationId {
            actor: ActorId(actor.to_string()),
            seq,
        },
        kind,
        CausalContext::observing(observed.iter()),
    )
}

/// Joins a connection and keeps the outbox alive for as long as the caller
/// keeps the returned receiver.
async fn join(
    handle: &DocumentHandle,
    connection: u64,
    subject: &str,
    actor: &str,
) -> mpsc::UnboundedReceiver<crate::protocol::ServerMessage> {
    let (outbox, inbox) = mpsc::unbounded_channel();
    handle
        .join(
            connection,
            subject,
            &ActorId(actor.to_string()),
            subject,
            outbox,
        )
        .await
        .expect("the connection joins");
    inbox
}

fn text_of(document: &Document) -> String {
    document
        .blocks
        .iter()
        .flat_map(|block| block.content.iter())
        .filter_map(|inline| match inline {
            Inline::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

/// A document something still holds is never reclaimed, however idle the
/// clock says it is — and the handle the registry hands out afterwards is the
/// *same thread*.
///
/// This is the test that has to be hard to fool. Counting registry entries
/// would not do it: a service that stopped an entry while a handle lived and
/// started a fresh thread on the next open would have exactly one entry
/// throughout and look perfect. So the assertion is made of two things a
/// second thread could not produce — the handle compares as the same thread,
/// and an operation submitted through the handle obtained *after* the sweep
/// both observes the operation submitted before it and commits onto the head
/// that one moved. A second thread would have loaded the log before alice's
/// commit: it would refuse bob's operation as claiming to have observed
/// something not in its log, and its own commit would lose the
/// compare-and-swap with "branch head moved under the commit".
#[tokio::test]
async fn a_document_a_handle_still_holds_is_never_reclaimed() {
    let root = TempRoot::new("registry-held");
    let clock = ManualClock::new(1_000);
    let service = service(clock.clock(), &root).with_document_idle_timeout_ms(100);
    let (document_uuid, inline) = seeded(&service);
    service
        .permissions()
        .set_role(&document_uuid, "alice", "bob", Some(Role::Editor))
        .expect("bob may edit");

    let held = service.document(&document_uuid).expect("the first open");
    let _alice_inbox = join(&held, 1, "alice", "actor-alice").await;
    let first = operation("actor-alice", 1, insert_text(&inline, 0, "X"), &[]);
    held.submit(1, "alice-batch", vec![first.clone()])
        .await
        .expect("alice's edit commits");

    // Far past the idle window, with a handle still alive.
    clock.advance_ms(10_000);
    assert_eq!(
        service.reclaim_idle_documents().expect("a sweep"),
        0,
        "a document a handle still holds must not be reclaimed, however idle the clock says it is"
    );

    let reopened = service
        .document(&document_uuid)
        .expect("the document is still open");
    assert!(
        held.is_same_thread(&reopened),
        "the sweep must have left the running thread alone, not replaced it"
    );

    let _bob_inbox = join(&reopened, 2, "bob", "actor-bob").await;
    let second = operation(
        "actor-bob",
        1,
        insert_text(&inline, 8, "Y"),
        std::slice::from_ref(&first),
    );
    reopened
        .submit(2, "bob-batch", vec![second])
        .await
        .expect("bob's edit must land on the same head alice's moved");

    let snapshot = reopened.snapshot().await.expect("the snapshot");
    // Bob's insert is at offset 8 of the text he had observed — alice's "X"
    // included — so it lands between "g" and "h". Pinned exactly, because a
    // test that only asked whether both letters were present would also pass
    // for a merge that put them anywhere.
    assert_eq!(
        text_of(&snapshot),
        "XabcdefgYh",
        "both edits must be in one history"
    );
    assert_eq!(
        service.open_document_count().expect("the count"),
        1,
        "one document, one thread, throughout"
    );
}

/// A document nothing holds keeps its thread until the idle window passes,
/// and then loses it — and what comes back is the document, from storage.
#[tokio::test]
async fn an_idle_document_loses_its_thread_and_comes_back_from_storage() {
    let root = TempRoot::new("registry-idle");
    let clock = ManualClock::new(1_000);
    let service = service(clock.clock(), &root).with_document_idle_timeout_ms(1_000);
    let (document_uuid, inline) = seeded(&service);

    {
        let handle = service.document(&document_uuid).expect("the first open");
        let _inbox = join(&handle, 1, "alice", "actor-alice").await;
        handle
            .submit(
                1,
                "alice-batch",
                vec![operation(
                    "actor-alice",
                    1,
                    insert_text(&inline, 0, "X"),
                    &[],
                )],
            )
            .await
            .expect("alice's edit commits");
    }
    assert_eq!(service.open_document_count().expect("the count"), 1);

    clock.advance_ms(999);
    assert_eq!(
        service.reclaim_idle_documents().expect("a sweep"),
        0,
        "a document inside the idle window keeps its thread, so a client that reconnects to what it just closed does not reload the log"
    );
    assert_eq!(service.open_document_count().expect("the count"), 1);

    clock.advance_ms(1);
    assert_eq!(
        service.reclaim_idle_documents().expect("a sweep"),
        1,
        "once the idle window has passed and nothing holds the document, its thread must be stopped"
    );
    assert_eq!(
        service.open_document_count().expect("the count"),
        0,
        "a reclaimed document holds no thread"
    );

    let reopened = service
        .document(&document_uuid)
        .expect("the document opens again");
    assert_eq!(service.open_document_count().expect("the count"), 1);
    let snapshot = reopened.snapshot().await.expect("the snapshot");
    assert_eq!(
        text_of(&snapshot),
        "Xabcdefgh",
        "the rebuilt thread must have loaded the durable log, not started an empty document"
    );
}

/// The open-document limit lifts when a document falls idle. It is a bound on
/// how many documents this process serves at once, not a fuse.
///
/// The refusal, while it stands, says which of the two things an operator
/// would do about it: wait for a connection to close, or wait out the idle
/// window. It used to say "until the process is restarted", which was the
/// truth then and is not now.
#[test]
fn the_open_document_limit_lifts_when_a_document_falls_idle() {
    let root = TempRoot::new("registry-limit");
    let clock = ManualClock::new(1_000);
    // Three documents, so the two counts in the refusal are different
    // numbers. At one each, a refusal that reported them the wrong way round
    // would read exactly like a correct one, and this test would be green for
    // a service that sent its operator to wait on the wrong thing.
    let service = service(clock.clock(), &root)
        .with_max_open_documents(3)
        .with_document_idle_timeout_ms(1_000);

    let first = service
        .create_document("alice", "First")
        .expect("the first");
    let second = service
        .create_document("alice", "Second")
        .expect("the second");
    let third = service
        .create_document("alice", "Third")
        .expect("the third");
    // One of the three is held by something outside the registry, like a live
    // connection; the other two are merely warm.
    let held = service.document(&first).expect("a handle on the first");

    let error = service
        .create_document("alice", "Fourth")
        .expect_err("the fourth must be refused while all three are in use");
    let message = error.message().to_string();
    assert!(
        message.contains("1 are held by a live connection"),
        "the refusal must say how many are held: {message}"
    );
    assert!(
        message.contains("2 were last used within this service's 1000 ms idle window"),
        "the refusal must say how many are merely warm, and for how long: {message}"
    );
    assert!(
        !message.contains("restart"),
        "a restart is no longer the remedy, and saying so would send an operator to do the wrong thing: {message}"
    );
    assert_eq!(
        service.documents_created_by("alice").expect("the count"),
        3,
        "a refused creation is not charged"
    );

    // A housekeeping pass that reclaims nothing, and then a wait that takes
    // the second document past the idle window without taking the process
    // past its next housekeeping pass. What follows can only succeed because
    // being *at the limit* forces a sweep of its own: a service that swept
    // only on its own schedule would refuse here, and go on refusing for the
    // rest of the window.
    clock.advance_ms(500);
    assert_eq!(
        service.reclaim_idle_documents().expect("a sweep"),
        0,
        "nothing has been idle long enough yet"
    );
    clock.advance_ms(600);
    let fourth = service
        .create_document("alice", "Fourth")
        .expect("the limit must lift once a document falls idle");
    assert_ne!(fourth, first);
    assert_ne!(fourth, second);
    assert_ne!(fourth, third);
    assert_eq!(
        service.open_document_count().expect("the count"),
        2,
        "the two reclaimed documents' threads are gone, and the new one took their place"
    );
    assert!(
        held.is_same_thread(&service.document(&first).expect("the first is still open")),
        "the document that was still held must have kept the thread it had"
    );
}

/// `stop_idle_documents` stops everything nothing is using, idle window or
/// not, and leaves alone what is in use.
#[test]
fn stopping_idle_documents_spares_the_ones_still_in_use() {
    let root = TempRoot::new("registry-stop");
    let clock = ManualClock::new(1_000);
    // A window nothing in this test will ever cross, so the only thing
    // deciding what stops is whether it is in use.
    let service = service(clock.clock(), &root).with_document_idle_timeout_ms(u64::MAX);
    let first = service
        .create_document("alice", "First")
        .expect("the first");
    let second = service
        .create_document("alice", "Second")
        .expect("the second");
    let held = service.document(&first).expect("a handle on the first");

    assert_eq!(
        service.stop_idle_documents().expect("stopping"),
        1,
        "only the document nothing holds can be stopped"
    );
    assert_eq!(service.open_document_count().expect("the count"), 1);
    assert!(
        held.is_same_thread(&service.document(&first).expect("the first is still open")),
        "a document a handle still holds keeps its thread"
    );
    let reopened = service.document(&second).expect("the second opens again");
    assert!(!reopened.is_same_thread(&held));
}

/// Looking a document up keeps it warm.
///
/// Handing out the handle is the only sign of use the registry can see for a
/// caller that does not hold one — an HTTP status read, a grant change.
/// Without this, a document read every second would still lose its thread on
/// the idle window's schedule and reload its whole log each time.
#[test]
fn looking_a_document_up_keeps_it_warm() {
    let root = TempRoot::new("registry-warm");
    let clock = ManualClock::new(1_000);
    let service = service(clock.clock(), &root).with_document_idle_timeout_ms(1_000);
    let (document_uuid, _inline) = seeded(&service);

    drop(service.document(&document_uuid).expect("the first open"));
    clock.advance_ms(999);
    drop(service.document(&document_uuid).expect("a second look"));
    clock.advance_ms(999);
    assert_eq!(
        service.reclaim_idle_documents().expect("a sweep"),
        0,
        "the second look reset the idle window; a document in use through lookups alone must keep its thread"
    );
    assert_eq!(service.open_document_count().expect("the count"), 1);

    clock.advance_ms(1);
    assert_eq!(
        service.reclaim_idle_documents().expect("a sweep"),
        1,
        "and it must lose it once nothing has looked at it for the whole window"
    );
}

/// Opening one document sweeps the ones that have gone idle, without waiting
/// for the process to reach its limit.
///
/// The limit is the backstop, not the schedule. A process well under it would
/// otherwise sit on the thread and the whole in-memory log of every document
/// anyone opened since it started.
#[test]
fn opening_one_document_sweeps_the_ones_that_have_gone_idle() {
    let root = TempRoot::new("registry-sweep");
    let clock = ManualClock::new(1_000);
    // Room for both, so nothing here is forced by the limit.
    let service = service(clock.clock(), &root)
        .with_max_open_documents(64)
        .with_document_idle_timeout_ms(1_000);
    let first = service
        .create_document("alice", "First")
        .expect("the first");
    let second = service
        .create_document("alice", "Second")
        .expect("the second");

    clock.advance_ms(2_000);
    let handle = service.document(&second).expect("opening the second");
    assert_eq!(
        service.open_document_count().expect("the count"),
        1,
        "opening a document must also have stopped the one that had gone idle"
    );
    assert_eq!(handle.document_uuid(), second);
    assert_ne!(first, second);
}
