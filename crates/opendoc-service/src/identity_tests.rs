//! Authentication: what the server believes about who is calling.

use crate::clock::ManualClock;
use crate::error::ServiceError;
use crate::identity::IdentityService;
use crate::test_support::{register_default_subjects, ALICE_KEY, BOB_KEY};
use opendoc_merge::ActorId;

fn identity(clock: &ManualClock) -> IdentityService {
    let identity = IdentityService::new(clock.clock());
    register_default_subjects(&identity);
    identity
}

#[test]
fn an_unknown_subject_and_a_wrong_key_fail_with_the_same_message() {
    let clock = ManualClock::new(1_000);
    let identity = identity(&clock);
    let unknown = identity.open_session("mallory", ALICE_KEY).unwrap_err();
    let wrong_key = identity
        .open_session("alice", "not-the-right-key-x")
        .unwrap_err();
    assert_eq!(unknown, wrong_key);
    assert!(matches!(unknown, ServiceError::Unauthenticated(_)));
}

#[test]
fn a_session_resolves_to_the_actor_the_directory_bound_not_one_the_caller_chose() {
    let clock = ManualClock::new(1_000);
    let identity = identity(&clock);
    let issued = identity.open_session("alice", ALICE_KEY).unwrap();
    assert_eq!(issued.actor, ActorId("actor-alice".to_string()));

    let authenticated = identity.authenticate(&issued.token).unwrap();
    assert_eq!(authenticated.subject, "alice");
    assert_eq!(authenticated.actor, ActorId("actor-alice".to_string()));
}

#[test]
fn two_sessions_for_one_subject_get_different_tokens() {
    let clock = ManualClock::new(1_000);
    let identity = identity(&clock);
    let first = identity.open_session("alice", ALICE_KEY).unwrap();
    let second = identity.open_session("alice", ALICE_KEY).unwrap();
    assert_ne!(first.token, second.token);
    assert_eq!(identity.open_session_count().unwrap(), 2);
}

#[test]
fn an_expired_session_is_refused_and_forgotten() {
    let clock = ManualClock::new(1_000);
    let identity = identity(&clock).with_session_ttl_ms(5_000);
    let issued = identity.open_session("alice", ALICE_KEY).unwrap();
    assert!(identity.authenticate(&issued.token).is_ok());

    clock.advance_ms(5_001);
    let error = identity.authenticate(&issued.token).unwrap_err();
    assert!(matches!(error, ServiceError::Unauthenticated(_)));
    // Refusing it also drops it, so an expired token does not accumulate.
    assert_eq!(identity.open_session_count().unwrap(), 0);
}

#[test]
fn revoking_a_subject_invalidates_every_token_it_held() {
    let clock = ManualClock::new(1_000);
    let identity = identity(&clock);
    let alice_first = identity.open_session("alice", ALICE_KEY).unwrap();
    let alice_second = identity.open_session("alice", ALICE_KEY).unwrap();
    let bob = identity.open_session("bob", BOB_KEY).unwrap();

    assert_eq!(identity.close_sessions_for_subject("alice").unwrap(), 2);
    assert!(identity.authenticate(&alice_first.token).is_err());
    assert!(identity.authenticate(&alice_second.token).is_err());
    // Somebody else's session is untouched.
    assert!(identity.authenticate(&bob.token).is_ok());
}

#[test]
fn closing_one_session_leaves_the_others_alone() {
    let clock = ManualClock::new(1_000);
    let identity = identity(&clock);
    let first = identity.open_session("alice", ALICE_KEY).unwrap();
    let second = identity.open_session("alice", ALICE_KEY).unwrap();
    assert!(identity.close_session(&first.token).unwrap());
    assert!(!identity.close_session(&first.token).unwrap());
    assert!(identity.authenticate(&second.token).is_ok());
}

#[test]
fn two_subjects_cannot_be_bound_to_one_actor_id() {
    let clock = ManualClock::new(1_000);
    let identity = identity(&clock);
    // Sharing an actor id would make `OperationId` ambiguous: the merge would
    // read two people's concurrent edits as one actor's sequential stream.
    let error = identity
        .register_subject("mallory", "actor-alice", "mallory-api-key-012345")
        .unwrap_err();
    assert!(matches!(error, ServiceError::Conflict(_)));
}

#[test]
fn a_subject_may_re_register_its_own_actor_with_a_new_key() {
    let clock = ManualClock::new(1_000);
    let identity = identity(&clock);
    identity
        .register_subject("alice", "actor-alice", "alice-rotated-key-0123")
        .unwrap();
    assert!(identity.open_session("alice", ALICE_KEY).is_err());
    assert!(identity
        .open_session("alice", "alice-rotated-key-0123")
        .is_ok());
}

#[test]
fn a_short_api_key_is_refused_at_registration() {
    let clock = ManualClock::new(1_000);
    let identity = identity(&clock);
    let error = identity
        .register_subject("dave", "actor-dave", "short")
        .unwrap_err();
    assert!(matches!(error, ServiceError::BadRequest(_)));
}

#[test]
fn a_token_that_was_never_issued_is_refused() {
    let clock = ManualClock::new(1_000);
    let identity = identity(&clock);
    assert!(matches!(
        identity.authenticate("not-a-token").unwrap_err(),
        ServiceError::Unauthenticated(_)
    ));
}
