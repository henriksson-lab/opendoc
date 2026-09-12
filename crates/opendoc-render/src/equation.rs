//! Equation projection: LaTeX source -> MathML Core markup.
//!
//! Per ADR 0003 the LaTeX-like source is the single canonical store and the
//! rendered form is *derived*. This module therefore never mutates an
//! [`Equation`]; it reads the source, projects it, and hands back markup plus
//! any warnings the projection produced. Nothing here is persisted.
//!
//! Conversion is done in Rust by [`math_core`], which targets MathML Core —
//! the subset implemented natively by WebKitGTK (the Tauri shell) and
//! Chromium. No JavaScript math engine is bundled.
//!
//! Failures degrade rather than propagate: a source that cannot be parsed is
//! rendered as its own escaped source text, flagged with `equation-error`, and
//! reported as a [`ModelWarning`]. The document always renders.

use math_core::{LatexToMathML, MathCoreConfig, MathDisplay, UnicodeSubstitution};
use opendoc_core::{Equation, EquationSourceFormat, ModelWarning};
use std::sync::OnceLock;

use crate::escape_html;

/// Warning code emitted when an equation source could not be parsed at all and
/// the renderer fell back to showing the raw source.
pub const WARNING_EQUATION_RENDER_FAILED: &str = "equation-render-failed";
/// Warning code emitted when an equation rendered, but contained commands the
/// converter does not know; those parts show as error text inside the formula.
pub const WARNING_EQUATION_UNKNOWN_COMMAND: &str = "equation-unknown-command";
/// Warning code emitted when an equation references a label that is not
/// defined in the same equation.
pub const WARNING_EQUATION_UNDEFINED_REFERENCE: &str = "equation-undefined-reference";

/// Whether an equation occupies its own line or flows with surrounding text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EquationDisplay {
    /// `Inline::Equation` — flows inside a paragraph.
    Inline,
    /// `BlockKind::EquationBlock` — centred on its own line, display style.
    Block,
}

impl EquationDisplay {
    fn as_math_display(self) -> MathDisplay {
        match self {
            EquationDisplay::Inline => MathDisplay::Inline,
            EquationDisplay::Block => MathDisplay::Block,
        }
    }
}

/// One equation projected to markup.
pub(crate) struct RenderedEquation {
    /// Markup to place inside the equation wrapper element. Either a `<math>`
    /// element or the escaped source text when projection failed.
    pub html: String,
    /// Extra CSS class for the wrapper element, describing the outcome.
    pub state_class: &'static str,
    /// Set when the projection failed; becomes a `data-equation-error`
    /// attribute so the failure is visible in the DOM as well as in warnings.
    pub error: Option<String>,
    /// Warnings produced while projecting. Callers surface these; the renderer
    /// itself never writes them back into the document.
    pub warnings: Vec<ModelWarning>,
}

/// The converter is configuration-only and its conversion entry point takes
/// `&self` with per-call local state, so one shared instance is safe and keeps
/// rendering deterministic.
fn converter() -> &'static LatexToMathML {
    static CONVERTER: OnceLock<LatexToMathML> = OnceLock::new();
    CONVERTER.get_or_init(|| {
        let config = MathCoreConfig {
            // Unknown commands render as flagged error text inside the formula
            // instead of failing the whole equation, so one bad command does
            // not hide an otherwise valid formula. We still warn about them.
            ignore_unknown_commands: true,
            // Wrap the formula in `<semantics>` with an
            // `<annotation encoding="application/x-tex">` carrying the LaTeX
            // source, so MathML consumers (copy/paste into TeX-aware tools,
            // assistive technology) can recover the authored form. Verified in
            // Chromium 152: the annotation is not rendered, only the formula.
            // This is derived output; the canonical copy stays in the model
            // and on the wrapper's `data-equation-source` attribute.
            annotation: true,
            // Modern browsers do not need the namespace on inline MathML.
            xml_namespace: false,
            unicode_substitution: UnicodeSubstitution::Conventional,
            ..MathCoreConfig::default()
        };
        // `LatexToMathML::new` only fails while parsing user-supplied macros,
        // and this configuration declares none.
        LatexToMathML::new(config)
            .expect("math-core configuration without custom macros is always valid")
    })
}

/// Project an equation to markup. Pure: the same input always produces the
/// same output and nothing is mutated.
pub(crate) fn render_equation(equation: &Equation, display: EquationDisplay) -> RenderedEquation {
    match equation.source_format {
        // Exhaustive on purpose: a new source format must make an explicit
        // decision here rather than silently being treated as LaTeX.
        EquationSourceFormat::LatexLike => render_latex(equation, display),
    }
}

fn render_latex(equation: &Equation, display: EquationDisplay) -> RenderedEquation {
    match converter().convert_with_local_state(&equation.source, display.as_math_display()) {
        Ok(result) => {
            let mut warnings = Vec::new();
            if result.warnings.has_unknown_commands() {
                warnings.push(warning(
                    WARNING_EQUATION_UNKNOWN_COMMAND,
                    equation,
                    "contains commands the equation renderer does not support",
                ));
            }
            if result.warnings.has_undefined_references() {
                warnings.push(warning(
                    WARNING_EQUATION_UNDEFINED_REFERENCE,
                    equation,
                    "references a label that is not defined in the equation",
                ));
            }
            RenderedEquation {
                html: result.mathml,
                state_class: "equation-rendered",
                error: None,
                warnings,
            }
        }
        Err(error) => {
            let detail = normalize_detail(&error.to_string());
            RenderedEquation {
                html: escape_html(&equation.source),
                state_class: "equation-error",
                warnings: vec![warning(WARNING_EQUATION_RENDER_FAILED, equation, &detail)],
                error: Some(detail),
            }
        }
    }
}

fn warning(code: &str, equation: &Equation, detail: &str) -> ModelWarning {
    ModelWarning {
        code: code.to_string(),
        message: format!("equation {}: {}", equation.id.as_str(), detail),
    }
}

/// `ModelWarning::validate` rejects empty or untrimmed messages, and a
/// converter error message must never be able to produce an invalid warning.
fn normalize_detail(message: &str) -> String {
    let trimmed = message.trim();
    if trimmed.is_empty() {
        "could not be parsed as LaTeX".to_string()
    } else {
        trimmed.replace(['\n', '\r'], " ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opendoc_core::StableId;

    fn equation(source: &str) -> Equation {
        Equation {
            id: StableId::parse("eq-1").expect("valid id"),
            source_format: EquationSourceFormat::LatexLike,
            source: source.to_string(),
        }
    }

    #[test]
    fn block_equation_renders_display_mathml() {
        let rendered = render_equation(&equation(r"\frac{a}{b}"), EquationDisplay::Block);
        assert!(rendered.error.is_none());
        assert!(rendered.warnings.is_empty());
        assert_eq!(rendered.state_class, "equation-rendered");
        assert!(
            rendered.html.contains("display=\"block\""),
            "block equations must carry display=block: {}",
            rendered.html
        );
        assert!(rendered.html.contains("<mfrac>"), "{}", rendered.html);
    }

    #[test]
    fn inline_equation_renders_without_display_block() {
        let rendered = render_equation(&equation("E = mc^2"), EquationDisplay::Inline);
        assert!(rendered.error.is_none());
        assert!(
            !rendered.html.contains("display=\"block\""),
            "{}",
            rendered.html
        );
        assert!(rendered.html.contains("<msup>"), "{}", rendered.html);
    }

    #[test]
    fn malformed_source_falls_back_to_escaped_source_with_a_warning() {
        let rendered = render_equation(&equation(r"\frac{a}{"), EquationDisplay::Block);
        assert_eq!(rendered.state_class, "equation-error");
        assert!(!rendered.html.contains("<math"), "{}", rendered.html);
        assert!(rendered.html.contains(r"\frac{a}{"), "{}", rendered.html);
        assert_eq!(rendered.warnings.len(), 1);
        let warning = &rendered.warnings[0];
        assert_eq!(warning.code, WARNING_EQUATION_RENDER_FAILED);
        assert!(warning.message.contains("eq-1"), "{}", warning.message);
        assert!(warning.validate().is_ok(), "{}", warning.message);
    }

    #[test]
    fn unknown_command_still_renders_the_rest_and_warns() {
        let rendered = render_equation(
            &equation(r"a + \notarealcommand{b}"),
            EquationDisplay::Inline,
        );
        assert!(rendered.error.is_none());
        assert!(rendered.html.contains("<math"), "{}", rendered.html);
        assert_eq!(rendered.warnings.len(), 1);
        assert_eq!(rendered.warnings[0].code, WARNING_EQUATION_UNKNOWN_COMMAND);
        assert!(rendered.warnings[0].validate().is_ok());
    }

    #[test]
    fn projection_is_deterministic_and_leaves_the_equation_untouched() {
        let source = equation(r"\sum_{i=0}^{N} x_i");
        let before = source.clone();
        let first = render_equation(&source, EquationDisplay::Block);
        let second = render_equation(&source, EquationDisplay::Block);
        assert_eq!(first.html, second.html);
        assert_eq!(source, before);
    }

    #[test]
    fn html_special_characters_in_a_failed_source_are_escaped() {
        let rendered = render_equation(&equation(r"<script>{"), EquationDisplay::Inline);
        assert_eq!(rendered.state_class, "equation-error");
        assert!(!rendered.html.contains("<script>"), "{}", rendered.html);
        assert!(
            rendered.html.contains("&lt;script&gt;"),
            "{}",
            rendered.html
        );
    }
}
