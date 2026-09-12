use crate::*;
use std::path::Path;

#[test]
fn plaintext_projection_imports_lines_as_paragraphs() {
    let report = import_plaintext_projection("Doc", "a\nb").unwrap();
    assert_eq!(report.document.blocks.len(), 2);
    assert_eq!(report.document.visible_text(), "a\nb\n");
}

#[test]
fn unsupported_import_aborts() {
    assert_eq!(
        import_doc_or_docx(Path::new("x.pdf")).unwrap_err(),
        ImportError::UnsupportedExtension("pdf".to_string())
    );
}

#[test]
fn mislabeled_legacy_doc_payload_aborts_before_converter_fallback() {
    let path = std::env::temp_dir().join(format!(
        "opendoc-import-mislabeled-legacy-doc-{}.doc",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    std::fs::write(&path, b"not a supported legacy Word binary").unwrap();

    assert!(matches!(
        import_doc_or_docx(&path),
        Err(ImportError::UnsupportedStructure(message))
            if message == "legacy .doc import requires an OLE compound document or RTF payload"
    ));
    let _ = std::fs::remove_file(path);
}
