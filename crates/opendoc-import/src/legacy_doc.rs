//! Legacy `.doc` conversion through an external converter, when one exists.

use crate::error::ImportError;
use std::fs;
use std::path::Path;
use std::process::Command;

/// Legacy binary `.doc` files are converted through an external converter
/// (pandoc or LibreOffice) into a plain-text projection. This is the only
/// import path that shells out; `.docx` is parsed natively.
pub(crate) fn convert_legacy_doc_to_plaintext(path: &Path) -> Result<String, ImportError> {
    if let Some(text) = try_pandoc_plaintext(path)? {
        return Ok(text);
    }
    if let Some(text) = try_libreoffice_plaintext(path)? {
        return Ok(text);
    }
    Err(ImportError::ConverterUnavailable)
}

pub(crate) fn validate_legacy_doc_container(path: &Path) -> Result<(), ImportError> {
    const OLE_COMPOUND_DOCUMENT_MAGIC: &[u8] = &[0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1];
    let bytes = fs::read(path).map_err(|err| ImportError::InvalidInput(err.to_string()))?;
    if bytes.starts_with(OLE_COMPOUND_DOCUMENT_MAGIC) || bytes.starts_with(b"{\\rtf") {
        Ok(())
    } else {
        Err(ImportError::UnsupportedStructure(
            "legacy .doc import requires an OLE compound document or RTF payload".to_string(),
        ))
    }
}

pub(crate) fn try_pandoc_plaintext(path: &Path) -> Result<Option<String>, ImportError> {
    let Ok(output) = Command::new("pandoc")
        .arg(path)
        .args(["-t", "plain", "--wrap=none"])
        .output()
    else {
        return Ok(None);
    };
    if !output.status.success() {
        return Ok(None);
    }
    let text = String::from_utf8(output.stdout)
        .map_err(|err| ImportError::InvalidInput(err.to_string()))?;
    Ok(non_empty_text(text))
}

pub(crate) fn try_libreoffice_plaintext(path: &Path) -> Result<Option<String>, ImportError> {
    let out_dir =
        std::env::temp_dir().join(format!("opendoc-import-convert-{}", std::process::id()));
    let _ = fs::remove_dir_all(&out_dir);
    fs::create_dir_all(&out_dir).map_err(|err| ImportError::InvalidInput(err.to_string()))?;
    let output = Command::new("soffice")
        .args([
            "--headless",
            "--convert-to",
            "txt:Text",
            "--outdir",
            out_dir
                .to_str()
                .ok_or_else(|| ImportError::InvalidInput("invalid output path".to_string()))?,
        ])
        .arg(path)
        .output();
    let Ok(output) = output else {
        let _ = fs::remove_dir_all(out_dir);
        return Ok(None);
    };
    if !output.status.success() {
        let _ = fs::remove_dir_all(out_dir);
        return Ok(None);
    }
    let converted = out_dir.join(format!(
        "{}.txt",
        path.file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("document")
    ));
    let text = match fs::read_to_string(&converted) {
        Ok(text) => text,
        Err(_) => {
            let _ = fs::remove_dir_all(out_dir);
            return Ok(None);
        }
    };
    let _ = fs::remove_dir_all(out_dir);
    Ok(non_empty_text(text))
}

pub(crate) fn non_empty_text(text: String) -> Option<String> {
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}
