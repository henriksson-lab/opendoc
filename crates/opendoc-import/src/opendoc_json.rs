//! The OpenDoc JSON extensions: equations and images Google Docs cannot express.

use crate::error::ImportError;
use crate::json::parse_imported_stable_id;
use opendoc_core::{
    Block, BlockKind, BlockProperties, Equation, EquationSourceFormat, ImageLayout, Inline,
    StableId,
};
use serde_json::Value;

pub(crate) fn import_opendoc_equation_block(equation: &Value) -> Result<Block, ImportError> {
    let source = equation
        .get("source")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            ImportError::UnsupportedStructure("OpenDoc block equation missing source".to_string())
        })?
        .to_string();
    let source_format = equation
        .get("sourceFormat")
        .and_then(Value::as_str)
        .map(import_equation_source_format)
        .transpose()?
        .unwrap_or(EquationSourceFormat::LatexLike);
    let block_id = equation
        .get("blockId")
        .and_then(Value::as_str)
        .map(parse_imported_stable_id)
        .transpose()?
        .unwrap_or_else(|| StableId::new("block"));
    let equation_id = equation
        .get("equationId")
        .and_then(Value::as_str)
        .map(parse_imported_stable_id)
        .transpose()?
        .unwrap_or_else(|| StableId::new("eq"));
    Ok(Block {
        id: block_id,
        kind: BlockKind::EquationBlock {
            equation: Equation {
                id: equation_id,
                source_format,
                source,
            },
        },
        content: Vec::new(),
        properties: BlockProperties::default(),
    })
}

pub(crate) fn import_opendoc_inline_equation(equation: &Value) -> Result<Inline, ImportError> {
    let source = equation
        .get("source")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            ImportError::UnsupportedStructure("OpenDoc inline equation missing source".to_string())
        })?
        .to_string();
    let source_format = equation
        .get("sourceFormat")
        .and_then(Value::as_str)
        .map(import_equation_source_format)
        .transpose()?
        .unwrap_or(EquationSourceFormat::LatexLike);
    let inline_id = equation
        .get("inlineId")
        .and_then(Value::as_str)
        .map(parse_imported_stable_id)
        .transpose()?
        .unwrap_or_else(|| StableId::new("equation"));
    let equation_id = equation
        .get("equationId")
        .and_then(Value::as_str)
        .map(parse_imported_stable_id)
        .transpose()?
        .unwrap_or_else(|| StableId::new("eq"));
    Ok(Inline::Equation {
        id: inline_id,
        equation: Equation {
            id: equation_id,
            source_format,
            source,
        },
    })
}

pub(crate) fn import_equation_source_format(
    value: &str,
) -> Result<EquationSourceFormat, ImportError> {
    match value {
        "latex-like" => Ok(EquationSourceFormat::LatexLike),
        other => Err(ImportError::UnsupportedStructure(format!(
            "unsupported OpenDoc equation source format {other}"
        ))),
    }
}

pub(crate) fn export_equation_source_format(source_format: &EquationSourceFormat) -> &'static str {
    match source_format {
        EquationSourceFormat::LatexLike => "latex-like",
    }
}

pub(crate) fn import_opendoc_image(image: &Value) -> Result<Block, ImportError> {
    let blob_hash = image
        .get("blobHash")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            ImportError::UnsupportedStructure("OpenDoc image missing blobHash".to_string())
        })?
        .trim()
        .to_string();
    opendoc_core::HashRef::parse(&blob_hash).map_err(|err| {
        ImportError::UnsupportedStructure(format!("OpenDoc image blobHash is invalid: {err}"))
    })?;
    let alt_text = image
        .get("altText")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let id = image
        .get("blockId")
        .and_then(Value::as_str)
        .map(parse_imported_stable_id)
        .transpose()?
        .unwrap_or_else(|| StableId::new("block"));
    let layout = image
        .get("layout")
        .map(|layout| serde_json::from_value::<ImageLayout>(layout.clone()))
        .transpose()
        .map_err(|error| {
            ImportError::UnsupportedStructure(format!("malformed OpenDoc image layout: {error}"))
        })?
        .unwrap_or_default();
    Ok(Block {
        id,
        kind: BlockKind::Image {
            blob_hash,
            alt_text,
            layout,
        },
        content: Vec::new(),
        properties: BlockProperties::default(),
    })
}
