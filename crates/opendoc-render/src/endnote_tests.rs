use crate::{render_endnotes, render_footnotes_html};
use opendoc_core::{Footnote, Inline, StableId};

#[test]
fn note_placement_selects_the_correct_document_trailer() {
    let mut document = opendoc_core::Document::new("notes");
    document.blocks.push(opendoc_core::Block::paragraph("body"));
    let footnote_id = StableId::parse("footnote").unwrap();
    let endnote_id = StableId::parse("endnote").unwrap();
    document.blocks[0].content.extend([
        Inline::FootnoteRef {
            id: StableId::new("ref"),
            footnote_id: footnote_id.clone(),
        },
        Inline::FootnoteRef {
            id: StableId::new("ref"),
            footnote_id: endnote_id.clone(),
        },
    ]);
    document.footnotes = vec![
        Footnote {
            id: footnote_id,
            revision: 1,
            body: vec![Inline::text("foot")],
            deleted: false,
        },
        Footnote {
            id: endnote_id.clone(),
            revision: 1,
            body: vec![Inline::text("end")],
            deleted: false,
        },
    ];
    document.endnote_ids.insert(endnote_id);
    assert!(document.validate().is_ok());
    let footnotes = render_footnotes_html(&document, []);
    let endnotes = render_endnotes(&document, []).html;
    assert!(footnotes.contains("foot"));
    assert!(!footnotes.contains(">end<"));
    assert!(endnotes.contains("doc-endnotes") && endnotes.contains("end"));
    assert!(!endnotes.contains(">foot<"));
}
