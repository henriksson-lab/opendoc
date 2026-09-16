//! Text, checkbox and degraded-equation content writers.

use crate::html::attr;
use crate::*;

/// Emit the failure attributes for an equation that could not be projected, so
/// the degraded state is visible in the DOM and on hover as well as in warnings.
pub(crate) fn write_equation_error(rendered: &RenderedEquation, out: &mut String) {
    if let Some(error) = &rendered.error {
        let escaped = attr(error);
        let _ = write!(
            out,
            " data-equation-error=\"{escaped}\" title=\"{escaped}\""
        );
    }
}

pub(crate) fn render_text_content(text: &str, out: &mut String) {
    let mut first = true;
    for line in text.split('\n') {
        if !first {
            out.push_str("<br data-soft-break=\"true\">");
        }
        first = false;
        out.push_str(&escape_html(line));
    }
}

/// A checklist item's checkbox.
///
/// The checkbox is a real `<input type="checkbox">` so it looks and reads like
/// one, but it is never the source of truth: `pointer-events` is off on the
/// input itself (see `.doc-checkbox` in the frontend stylesheet), the wrapper
/// carries the click, and toggling goes through the `set_list_item_checked`
/// command. That keeps the checked state a document fact rather than a DOM
/// fact, and it keeps the input's checkedness attribute-driven so a re-render
/// can move it — a user-clicked checkbox would go "dirty" and stop following
/// its attribute.
pub(crate) fn render_checkbox(block_id: &str, checked: bool, out: &mut String) {
    let _ = write!(
        out,
        "<span class=\"doc-checkbox\" contenteditable=\"false\" tabindex=\"0\" role=\"checkbox\" aria-checked=\"{checked}\" aria-label=\"Done\" data-action=\"toggle-checklist-item\" data-checklist-block-id=\"{}\" data-checked=\"{checked}\"><input type=\"checkbox\" tabindex=\"-1\" aria-hidden=\"true\"{}></span>",
        attr(block_id),
        if checked { " checked" } else { "" }
    );
}
