use crate::{Block, Bookmark, Document, StableId};

#[test]
fn bookmark_names_are_portable_and_live_names_are_unique() {
    let mut document = Document::new("Bookmarks");
    let block = Block::paragraph("target");
    let target = block.id.clone();
    document.blocks.push(block);
    document.bookmarks.push(Bookmark {
        id: StableId::parse("bookmark-one").unwrap(),
        name: "chapter_one".to_string(),
        block_id: target.clone(),
        revision: 1,
        deleted: false,
    });
    document.validate().unwrap();

    document.bookmarks.push(Bookmark {
        id: StableId::parse("bookmark-two").unwrap(),
        name: "chapter_one".to_string(),
        block_id: target,
        revision: 1,
        deleted: false,
    });
    assert!(document.validate().is_err());

    document.bookmarks[1].deleted = true;
    document.validate().unwrap();
}

#[test]
fn bookmark_target_can_be_dangling_after_collaborative_block_delete() {
    let mut document = Document::new("Bookmarks");
    document.bookmarks.push(Bookmark {
        id: StableId::parse("bookmark-one").unwrap(),
        name: "restorable_target".to_string(),
        block_id: StableId::parse("deleted-block").unwrap(),
        revision: 1,
        deleted: false,
    });
    document.validate().unwrap();
}
