//! Authorization: what the server decides, from storage it owns.

use crate::error::ServiceError;
use crate::permission::{Action, PermissionService, Role};
use crate::store::SharedStore;
use crate::test_support::TempRoot;
use opendoc_store::LocalObjectStore;

const DOCUMENT: &str = "doc-0000000000000001-0000000000000001";

fn permissions(root: &TempRoot) -> PermissionService<SharedStore<LocalObjectStore>> {
    PermissionService::new(SharedStore::new(root.store()))
}

#[test]
fn a_subject_with_no_grant_is_forbidden_from_reading() {
    let root = TempRoot::new("perm-none");
    let permissions = permissions(&root);
    let error = permissions
        .authorize(DOCUMENT, "mallory", Action::Read)
        .unwrap_err();
    assert!(matches!(error, ServiceError::Forbidden(_)));
    assert_eq!(permissions.role_for(DOCUMENT, "mallory").unwrap(), None);
}

#[test]
fn roles_gate_actions_by_rank() {
    let root = TempRoot::new("perm-rank");
    let permissions = permissions(&root);
    permissions.seed_owner(DOCUMENT, "alice").unwrap();
    permissions
        .set_role(DOCUMENT, "alice", "viewer", Some(Role::Viewer))
        .unwrap();
    permissions
        .set_role(DOCUMENT, "alice", "commenter", Some(Role::Commenter))
        .unwrap();
    permissions
        .set_role(DOCUMENT, "alice", "editor", Some(Role::Editor))
        .unwrap();

    for (subject, allowed, refused) in [
        (
            "viewer",
            vec![Action::Read, Action::Present],
            vec![Action::Comment, Action::Write, Action::Share],
        ),
        (
            "commenter",
            vec![Action::Read, Action::Comment],
            vec![Action::Write, Action::Share],
        ),
        (
            "editor",
            vec![Action::Read, Action::Comment, Action::Write],
            vec![Action::Share],
        ),
        (
            "alice",
            vec![Action::Read, Action::Comment, Action::Write, Action::Share],
            vec![],
        ),
    ] {
        for action in allowed {
            assert!(
                permissions.authorize(DOCUMENT, subject, action).is_ok(),
                "{subject} should be allowed {}",
                action.as_str()
            );
        }
        for action in refused {
            assert!(
                matches!(
                    permissions.authorize(DOCUMENT, subject, action),
                    Err(ServiceError::Forbidden(_))
                ),
                "{subject} should be refused {}",
                action.as_str()
            );
        }
    }
}

#[test]
fn grants_are_durable_not_cached() {
    let root = TempRoot::new("perm-durable");
    {
        let permissions = permissions(&root);
        permissions.seed_owner(DOCUMENT, "alice").unwrap();
        permissions
            .set_role(DOCUMENT, "alice", "bob", Some(Role::Editor))
            .unwrap();
    }
    // A completely new service over the same store: nothing carried over
    // except what reached the object store.
    let reopened = permissions(&root);
    assert_eq!(
        reopened.role_for(DOCUMENT, "bob").unwrap(),
        Some(Role::Editor)
    );
    assert_eq!(
        reopened.role_for(DOCUMENT, "alice").unwrap(),
        Some(Role::Owner)
    );
}

#[test]
fn a_non_owner_cannot_change_grants() {
    let root = TempRoot::new("perm-share");
    let permissions = permissions(&root);
    permissions.seed_owner(DOCUMENT, "alice").unwrap();
    permissions
        .set_role(DOCUMENT, "alice", "bob", Some(Role::Editor))
        .unwrap();

    let error = permissions
        .set_role(DOCUMENT, "bob", "mallory", Some(Role::Editor))
        .unwrap_err();
    assert!(matches!(error, ServiceError::Forbidden(_)));
    assert_eq!(permissions.role_for(DOCUMENT, "mallory").unwrap(), None);
}

#[test]
fn revoking_a_grant_removes_it() {
    let root = TempRoot::new("perm-revoke");
    let permissions = permissions(&root);
    permissions.seed_owner(DOCUMENT, "alice").unwrap();
    permissions
        .set_role(DOCUMENT, "alice", "bob", Some(Role::Editor))
        .unwrap();
    permissions
        .set_role(DOCUMENT, "alice", "bob", None)
        .unwrap();
    assert_eq!(permissions.role_for(DOCUMENT, "bob").unwrap(), None);
}

#[test]
fn the_last_owner_cannot_be_removed() {
    let root = TempRoot::new("perm-last-owner");
    let permissions = permissions(&root);
    permissions.seed_owner(DOCUMENT, "alice").unwrap();
    let error = permissions
        .set_role(DOCUMENT, "alice", "alice", None)
        .unwrap_err();
    assert!(matches!(error, ServiceError::Conflict(_)));
    // And demotion is the same trap by another name.
    assert!(permissions
        .set_role(DOCUMENT, "alice", "alice", Some(Role::Editor))
        .is_err());
    assert_eq!(
        permissions.role_for(DOCUMENT, "alice").unwrap(),
        Some(Role::Owner)
    );
}

#[test]
fn an_owner_may_step_down_once_another_owner_exists() {
    let root = TempRoot::new("perm-handover");
    let permissions = permissions(&root);
    permissions.seed_owner(DOCUMENT, "alice").unwrap();
    permissions
        .set_role(DOCUMENT, "alice", "bob", Some(Role::Owner))
        .unwrap();
    permissions
        .set_role(DOCUMENT, "alice", "alice", Some(Role::Editor))
        .unwrap();
    assert_eq!(
        permissions.role_for(DOCUMENT, "alice").unwrap(),
        Some(Role::Editor)
    );
}

#[test]
fn acl_history_is_durable_ordered_and_omits_noop_requests() {
    let root = TempRoot::new("perm-audit");
    {
        let permissions = permissions(&root);
        permissions.seed_owner(DOCUMENT, "alice").unwrap();
        permissions
            .set_role(DOCUMENT, "alice", "bob", Some(Role::Viewer))
            .unwrap();
        // A repeated request does not change access, so it is not an event.
        permissions
            .set_role(DOCUMENT, "alice", "bob", Some(Role::Viewer))
            .unwrap();
        permissions
            .set_role(DOCUMENT, "alice", "bob", None)
            .unwrap();
        let audit = permissions.grant_audit(DOCUMENT).unwrap();
        assert_eq!(audit.len(), 3);
        assert_eq!(audit[0].sequence, 3);
        assert_eq!(audit[0].actor_subject, "alice");
        assert_eq!(audit[0].target_subject, "bob");
        assert_eq!(audit[0].previous_role, Some(Role::Viewer));
        assert_eq!(audit[0].role, None);
        assert_eq!(audit[2].sequence, 1);
        assert_eq!(audit[2].role, Some(Role::Owner));
    }
    let reopened = permissions(&root);
    let audit = reopened.grant_audit(DOCUMENT).unwrap();
    assert_eq!(audit.len(), 3);
    assert_eq!(audit[0].sequence, 3);
    assert_eq!(audit[2].target_subject, "alice");
}

#[test]
fn seeding_an_owner_on_a_document_that_has_grants_is_refused() {
    let root = TempRoot::new("perm-reseed");
    let permissions = permissions(&root);
    permissions.seed_owner(DOCUMENT, "alice").unwrap();
    let error = permissions.seed_owner(DOCUMENT, "mallory").unwrap_err();
    assert!(matches!(error, ServiceError::Conflict(_)));
    assert_eq!(permissions.role_for(DOCUMENT, "mallory").unwrap(), None);
}

#[test]
fn a_document_uuid_that_is_not_a_key_segment_is_refused() {
    let root = TempRoot::new("perm-path");
    let permissions = permissions(&root);
    // A traversal in the uuid must not be able to name another document's
    // grant file, or a service subtree.
    let error = permissions
        .authorize("../../etc/passwd", "alice", Action::Read)
        .unwrap_err();
    assert!(matches!(error, ServiceError::BadRequest(_)));
}

#[test]
fn roles_round_trip_through_their_names() {
    for role in [Role::Viewer, Role::Commenter, Role::Editor, Role::Owner] {
        assert_eq!(Role::parse(role.as_str()).unwrap(), role);
    }
    assert!(Role::parse("superuser").is_err());
}

/// A document this service creates must be one a client can immediately type
/// into.
///
/// An editor resolves a caret against a block; a document with none has no
/// position a keystroke can name, so a freshly created document that a client
/// joined was a document nobody could edit. The browser shell hit exactly
/// that, and nothing here caught it because every other test seeds its own
/// blocks before typing.
#[test]
fn a_created_document_has_somewhere_to_put_the_caret() {
    let root = TempRoot::new("create-editable");
    let service = crate::service::OpenDocService::new(root.store(), crate::Clock::system());
    let document_uuid = service
        .create_document("alice", "Shared")
        .expect("creating the document");
    let log = crate::log::DocumentLog::load(service.repository(), &document_uuid)
        .expect("loading it back");
    assert_eq!(
        log.base().blocks.len(),
        1,
        "a created document must carry the one paragraph an empty document has"
    );
    assert!(
        matches!(
            log.base().blocks[0].kind,
            opendoc_core::BlockKind::Paragraph
        ),
        "and it must be a paragraph"
    );
    assert!(
        !log.base().blocks[0].content.is_empty(),
        "with a run in it, or there is still no offset a caret can name"
    );
}
