//! Unit tests for the DOCX reader's standalone helpers.

use crate::docx::convert::hyperlink_field_target;
use crate::docx::package::resolve_part_path;
use crate::docx::styles::{detect_heading, HeadingStyle};
use crate::docx::util::parse_iso_datetime_ms;

#[test]
fn resolves_relationship_targets_relative_to_the_source_part() {
    assert_eq!(
        resolve_part_path("word", "media/image1.png").as_deref(),
        Some("word/media/image1.png")
    );
    assert_eq!(
        resolve_part_path("word", "/word/media/image1.png").as_deref(),
        Some("word/media/image1.png")
    );
    assert_eq!(
        resolve_part_path("word", "../customXml/item1.xml").as_deref(),
        Some("customXml/item1.xml")
    );
    assert_eq!(resolve_part_path("word", "../../etc/passwd"), None);
    assert_eq!(resolve_part_path("word", "https://example.invalid/x"), None);
}

#[test]
fn parses_iso_dates_into_unix_milliseconds() {
    assert_eq!(parse_iso_datetime_ms("1970-01-01T00:00:00Z"), Some(0));
    assert_eq!(
        parse_iso_datetime_ms("2024-01-15T10:30:00Z"),
        Some(1_705_314_600_000)
    );
    assert_eq!(
        parse_iso_datetime_ms("2024-01-15T11:30:00.250+01:00"),
        Some(1_705_314_600_250)
    );
    assert_eq!(parse_iso_datetime_ms("not a date"), None);
}

#[test]
fn parses_hyperlink_field_instructions() {
    assert_eq!(
        hyperlink_field_target(r#" HYPERLINK "https://example.invalid/a" \o "tip" "#).as_deref(),
        Some("https://example.invalid/a")
    );
    assert_eq!(
        hyperlink_field_target(r#" HYPERLINK \l "Bookmark1" "#).as_deref(),
        Some("#Bookmark1")
    );
    assert_eq!(hyperlink_field_target(" PAGE "), None);
}

#[test]
fn detects_heading_styles_from_ids_names_and_outline_levels() {
    assert_eq!(
        detect_heading("Heading3", "heading 3", Some(2)),
        Some(HeadingStyle::Level(3))
    );
    assert_eq!(
        detect_heading("berschrift1", "heading 1", None),
        Some(HeadingStyle::Level(1))
    );
    assert_eq!(
        detect_heading("Title", "Title", Some(0)),
        Some(HeadingStyle::Title)
    );
    assert_eq!(
        detect_heading("Custom", "My Style", Some(1)),
        Some(HeadingStyle::Level(2))
    );
    assert_eq!(detect_heading("Normal", "Normal", None), None);
    assert_eq!(detect_heading("Normal", "Normal", Some(9)), None);
}
