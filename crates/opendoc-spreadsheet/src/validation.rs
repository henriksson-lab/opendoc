//! Enforcement of cell data validation on typed input.
//!
//! A [`CellValidation`](crate::CellValidation) with `strict` set is the
//! sheet saying *reject this entry*, not *flag it*. Storing the rule and
//! then writing whatever the user typed makes the rule worse than absent:
//! the dropdown, the Google Sheets export and the audit view all advertise a
//! constraint that nothing holds up.
//!
//! # What is checked, and what deliberately is not
//!
//! The candidate is the text the user typed, run through the same
//! [`parse_input`] the cell itself stores through, so `50%`, `$5` and
//! `2024-01-05` are compared as the numbers they name rather than as the
//! characters they were typed with. Two inputs are never refused:
//!
//! * **Empty** — clearing a cell is not an entry. A strict rule that made a
//!   cell undeletable would be a trap, and it is not what Sheets does.
//! * **A formula** — the rule constrains the formula's *result*, which does
//!   not exist until recalculation, and recalculation runs after the write.
//!   Refusing on the source text would reject `=1+1` under
//!   `number_greater 10` for the wrong reason, and accept `=1/0`.
//!
//! `custom_formula` is likewise not enforced here: it needs the evaluator and
//! a cell to evaluate against. An unknown or unenforceable kind accepts, so a
//! rule this module cannot decide never silently blocks a user.

use super::format::{parse_input, Locale};
use super::model::CellValidation;

/// The effective allowed set for a list rule, when the caller could resolve
/// one. `one_of_range` names a range rather than literal values, and only a
/// caller holding the workbook can read it.
pub(crate) type ListValues<'a> = Option<&'a [String]>;

/// Why a strict rule refused `text`, or `None` when it accepts.
///
/// The message names the rule and the value, because the only place it can
/// surface is an error on the write the user just made.
pub(crate) fn refusal(
    validation: &CellValidation,
    text: &str,
    locale: &Locale,
    range_values: ListValues<'_>,
) -> Option<String> {
    let parsed = parse_input(text, locale);
    if matches!(parsed.kind, "empty" | "formula") {
        return None;
    }
    let number = number_of(&parsed.value, parsed.kind);
    let bound = |index: usize| -> Option<f64> {
        let raw = validation.values.get(index)?;
        let parsed = parse_input(raw.trim(), locale);
        number_of(&parsed.value, parsed.kind)
    };
    let typed = text.trim();
    match validation.kind.as_str() {
        "list" | "one_of_list" | "one_of_range" => match range_reference(validation) {
            // A rule whose values name a range rather than spell out the
            // allowed set. An unresolvable range accepts: the rule is real,
            // but this call site could not read it, and refusing on a rule
            // nobody evaluated is the failure mode this module exists to
            // remove.
            Some(reference) => in_list(typed, range_values?)
                .map(|()| format!("{typed} is not one of the values {reference} allows")),
            None => in_list(typed, &validation.values).map(|()| {
                format!(
                    "{typed} is not one of the values this cell allows: {}",
                    validation.values.join(", ")
                )
            }),
        },
        "number_greater" => {
            let limit = bound(0)?;
            match number {
                Some(value) if value > limit => None,
                _ => Some(format!(
                    "{typed} is not a number greater than {}",
                    trim(limit)
                )),
            }
        }
        "number_less" => {
            let limit = bound(0)?;
            match number {
                Some(value) if value < limit => None,
                _ => Some(format!("{typed} is not a number less than {}", trim(limit))),
            }
        }
        "number_between" => {
            let (low, high) = {
                let (first, second) = (bound(0)?, bound(1)?);
                if first <= second {
                    (first, second)
                } else {
                    (second, first)
                }
            };
            match number {
                Some(value) if value >= low && value <= high => None,
                _ => Some(format!(
                    "{typed} is not a number between {} and {}",
                    trim(low),
                    trim(high)
                )),
            }
        }
        "text_contains" => {
            let needle = validation.values.first()?;
            if typed.to_lowercase().contains(&needle.to_lowercase()) {
                None
            } else {
                Some(format!("{typed} does not contain {needle}"))
            }
        }
        // `custom_formula`, and any kind a future model adds: not decidable
        // here, so not refused here.
        _ => None,
    }
}

/// The range a list rule reads its allowed values from, when it has one.
///
/// A legacy document may have folded `one_of_range` into `list`, so a single
/// list value that parses as a range is still read as a reference. New Google
/// Sheets imports retain `one_of_range` distinctly, which lets export preserve
/// the native condition type. Enforcing either form literally would refuse
/// every real entry, so the rule is decided against the range's contents or
/// not at all.
///
/// A range needs a colon to be a range, which is what keeps an ordinary
/// one-item list (`Yes`) and a value that merely looks addressable (`A1`)
/// out of this path.
pub(crate) fn range_reference(validation: &CellValidation) -> Option<&str> {
    if !matches!(
        validation.kind.as_str(),
        "list" | "one_of_list" | "one_of_range"
    ) {
        return None;
    }
    let [only] = validation.values.as_slice() else {
        return None;
    };
    let body = only.trim().trim_start_matches('=');
    let body = body.rsplit_once('!').map_or(body, |(_, range)| range);
    let body = body.replace('$', "");
    if !body.contains(':') || super::address::normalize_cell_range(&body).is_err() {
        return None;
    }
    Some(only.as_str())
}

/// `Some(())` when `typed` is *absent* from `values` — that is, when the list
/// rule refuses it. An empty list decides nothing and so refuses nothing.
///
/// Matching is case-insensitive, as it is in Sheets: a dropdown offering
/// `Yes` accepts `yes`, and a rule that did not would refuse the value its
/// own export round-trips.
fn in_list(typed: &str, values: &[String]) -> Option<()> {
    if values.is_empty() {
        return None;
    }
    (!values.iter().any(|value| value.eq_ignore_ascii_case(typed))).then_some(())
}

fn number_of(value: &str, kind: &str) -> Option<f64> {
    (kind == "number")
        .then(|| value.parse::<f64>().ok())
        .flatten()
}

fn trim(value: f64) -> String {
    super::address::trim_number(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(kind: &str, values: &[&str]) -> CellValidation {
        CellValidation::new(
            kind,
            values.iter().map(|value| (*value).to_string()).collect(),
            true,
        )
        .expect("a valid rule")
    }

    fn check(kind: &str, values: &[&str], text: &str) -> Option<String> {
        refusal(&rule(kind, values), text, &Locale::for_tag("en-US"), None)
    }

    #[test]
    fn number_rules_refuse_values_outside_their_bound() {
        assert!(check("number_greater", &["10"], "1").is_some());
        assert!(check("number_greater", &["10"], "11").is_none());
        assert!(check("number_greater", &["10"], "10").is_some());
        assert!(check("number_less", &["10"], "9").is_none());
        assert!(check("number_less", &["10"], "10").is_some());
        assert!(check("number_between", &["1", "5"], "3").is_none());
        assert!(check("number_between", &["1", "5"], "6").is_some());
        // A reversed bound pair still names the same interval.
        assert!(check("number_between", &["5", "1"], "3").is_none());
    }

    #[test]
    fn a_number_rule_refuses_text() {
        assert!(check("number_greater", &["10"], "lots").is_some());
    }

    #[test]
    fn typed_input_is_compared_as_the_number_it_names() {
        // `50%` is the number 0.5, not the string "50%".
        assert!(check("number_less", &["1"], "50%").is_none());
        assert!(check("number_greater", &["1"], "50%").is_some());
        assert!(check("number_greater", &["1000"], "$2,000").is_none());
    }

    #[test]
    fn a_list_rule_matches_case_insensitively() {
        assert!(check("list", &["Yes", "No"], "yes").is_none());
        assert!(check("list", &["Yes", "No"], "Maybe").is_some());
    }

    #[test]
    fn clearing_a_cell_and_writing_a_formula_are_never_refused() {
        assert!(check("number_greater", &["10"], "").is_none());
        assert!(check("number_greater", &["10"], "=1+1").is_none());
    }

    #[test]
    fn an_unenforceable_rule_accepts_rather_than_blocks() {
        assert!(check("custom_formula", &["=A1>0"], "anything").is_none());
        // `one_of_range` with no resolved values cannot decide.
        assert!(check("one_of_range", &["Sheet1!A1:A3"], "anything").is_none());
    }

    #[test]
    fn a_resolved_range_rule_behaves_like_a_list() {
        let rule = rule("one_of_range", &["Sheet1!A1:A2"]);
        let values = vec!["Red".to_string(), "Green".to_string()];
        let locale = Locale::for_tag("en-US");
        assert!(refusal(&rule, "Red", &locale, Some(&values)).is_none());
        assert!(refusal(&rule, "Blue", &locale, Some(&values)).is_some());
    }

    #[test]
    fn text_contains_matches_a_substring() {
        assert!(check("text_contains", &["cat"], "concatenate").is_none());
        assert!(check("text_contains", &["cat"], "dog").is_some());
    }
}
