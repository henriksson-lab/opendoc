//! Shared scaffolding for the service tests.

use crate::identity::IdentityService;
use opendoc_core::{Block, BlockKind, Document, Inline, StableId};
use opendoc_merge::OperationKind;
use opendoc_store::LocalObjectStore;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_ROOT: AtomicU64 = AtomicU64::new(1);

/// A directory that removes itself. Cheaper than a dependency and the tests
/// do not need more than this.
pub struct TempRoot {
    path: PathBuf,
}

impl TempRoot {
    pub fn new(label: &str) -> Self {
        let unique = NEXT_ROOT.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "opendoc-service-{label}-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("creating the test root");
        Self { path }
    }

    pub fn store(&self) -> LocalObjectStore {
        LocalObjectStore::new(self.path.clone())
    }

    /// The directory the store writes into, for tests that open the same
    /// repository through another crate's reader.
    pub fn path(&self) -> &PathBuf {
        &self.path
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

pub const ALICE_KEY: &str = "alice-api-key-0123456789";
pub const BOB_KEY: &str = "bob-api-key-0123456789";
pub const CAROL_KEY: &str = "carol-api-key-0123456789";

pub fn register_default_subjects(identity: &IdentityService) {
    identity
        .register_subject("alice", "actor-alice", ALICE_KEY)
        .expect("registering alice");
    identity
        .register_subject("bob", "actor-bob", BOB_KEY)
        .expect("registering bob");
    identity
        .register_subject("carol", "actor-carol", CAROL_KEY)
        .expect("registering carol");
}

/// A document with one paragraph holding one text run, so character
/// operations have something to aim at.
pub fn seeded_document(title: &str) -> (Document, StableId, StableId) {
    let mut document = Document::new(title);
    let block_id = StableId::parse("blk-seed-0001").expect("block id");
    let inline_id = StableId::parse("inl-seed-0001").expect("inline id");
    document.blocks.push(Block {
        id: block_id.clone(),
        kind: BlockKind::Paragraph,
        properties: Default::default(),
        content: vec![Inline::Text {
            id: inline_id.clone(),
            text: "abcdefgh".to_string(),
            marks: Vec::new(),
        }],
    });
    (document, block_id, inline_id)
}

pub fn insert_text(inline_id: &StableId, offset: usize, text: &str) -> OperationKind {
    OperationKind::InsertText {
        inline_id: inline_id.clone(),
        offset,
        text: text.to_string(),
    }
}
