use super::*;
use opendoc_core::{CitationItem, CitationPlacement, CitationSource, CitationSummary, StableId};

fn reference(id: &str, author: &str, year: &str, title: &str) -> BibliographyReference {
    BibliographyReference {
        id: StableId::parse(id).unwrap(),
        revision: 1,
        source: CitationSource {
            format: CitationSourceFormat::CitumNative,
            bytes: Vec::new(),
        },
        summary: CitationSummary {
            title: title.to_string(),
            authors: vec![author.to_string()],
            issued: Some(year.to_string()),
            doi: None,
            url: None,
        },
        deleted: false,
    }
}

fn citation(items: Vec<CitationItem>) -> CitationGroup {
    CitationGroup {
        id: StableId::parse("cite-test").unwrap(),
        revision: 1,
        items,
        placement: CitationPlacement::Inline,
        rendered_cache: None,
        deleted: false,
    }
}

#[test]
fn renders_author_year_and_numeric_labels_from_document_local_database() {
    let mut database = CitationDatabase::default();
    database.upsert_reference(reference("ref-doe", "Doe", "2020", "Example"));
    database.upsert_reference(reference("ref-smith", "Smith", "2021", "Second"));
    let group = citation(vec![
        CitationItem {
            reference_id: StableId::parse("ref-doe").unwrap(),
            locator: Some("17".to_string()),
            label: Some("page".to_string()),
            prefix: Some("see".to_string()),
            suffix: None,
            suppress_author: false,
        },
        CitationItem {
            reference_id: StableId::parse("ref-smith").unwrap(),
            locator: Some("22".to_string()),
            label: Some("page".to_string()),
            prefix: None,
            suffix: Some("reviewed".to_string()),
            suppress_author: false,
        },
    ]);

    assert_eq!(
        render_citation_group(&database, &group),
        "(see Doe 2020, page 17; Smith 2021, page 22 reviewed)"
    );

    database.style = "ieee".to_string();
    assert_eq!(render_citation_group(&database, &group), "[1, 2]");

    database.style = "vancouver".to_string();
    assert_eq!(
        render_citation_group(&database, &group),
        "(see 1,2 reviewed)"
    );
}

#[test]
fn incomplete_author_year_reference_renders_group_placeholder() {
    let mut database = CitationDatabase::default();
    database.upsert_reference(BibliographyReference {
        id: StableId::parse("ref-title-only").unwrap(),
        revision: 1,
        source: opendoc_core::CitationSource {
            format: CitationSourceFormat::CitumNative,
            bytes: b"title: Title Only".to_vec(),
        },
        summary: CitationSummary {
            title: "Title Only".to_string(),
            authors: Vec::new(),
            issued: None,
            doi: None,
            url: None,
        },
        deleted: false,
    });
    let group = citation(vec![CitationItem {
        reference_id: StableId::parse("ref-title-only").unwrap(),
        locator: Some("12".to_string()),
        label: Some("page".to_string()),
        prefix: None,
        suffix: None,
        suppress_author: false,
    }]);

    assert_eq!(render_citation_group(&database, &group), "[cite-test]");
}

#[test]
fn renders_bibliography_projection_without_deleted_references() {
    let mut database = CitationDatabase::default();
    database.upsert_reference(reference("ref-doe", "Doe", "2020", "Example"));
    database.upsert_reference(reference("ref-smith", "Smith", "2021", "Second"));
    database.delete_reference(&StableId::parse("ref-doe").unwrap(), 2);

    let entries = render_bibliography(&database);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].reference_id, "ref-smith");
    assert_eq!(entries[0].text, "Smith (2021). Second.");

    database.style = "numeric".to_string();
    let entries = render_bibliography(&database);
    assert_eq!(entries[0].text, "[1] Smith. Second. 2021");

    database.style = "ieee".to_string();
    let entries = render_bibliography(&database);
    assert_eq!(entries[0].text, "[1] Smith. Second. 2021");

    database.style = "vancouver".to_string();
    let entries = render_bibliography(&database);
    assert!(entries[0].text.starts_with("1. "), "{}", entries[0].text);
    assert!(entries[0].text.contains("Second"), "{}", entries[0].text);
}

#[test]
fn citum_native_source_bytes_are_regenerated_from_summary() {
    let mut item = reference("ref-doe", "Doe", "2020", "Example");
    item.summary.doi = Some("10.123/example".to_string());
    item.source.bytes = b"stale".to_vec();

    let bytes = citation_source_bytes(&item);
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.contains("title: Example\n"));
    assert!(text.contains("author: Doe\n"));
    assert!(text.contains("year: 2020\n"));
    assert!(text.contains("doi: 10.123/example\n"));
}

#[test]
fn citum_native_source_bytes_escape_line_and_author_separators() {
    let mut item = reference("ref-doe", "Doe; Lab", "2020", "Line one\nLine two; escaped");
    item.summary.authors.push("Roe\\Unit".to_string());
    item.summary.url = Some("https://example.invalid/a;b".to_string());

    let bytes = citation_source_bytes(&item);
    let text = String::from_utf8(bytes.clone()).unwrap();
    assert!(text.contains("title: Line one\\nLine two\\; escaped\n"));
    assert!(text.contains("author: Doe\\; Lab; Roe\\\\Unit\n"));
    assert!(text.contains("url: https://example.invalid/a\\;b\n"));

    let parsed = parse_citum_native_summary(&bytes).unwrap();
    assert_eq!(parsed.title, "Line one\nLine two; escaped");
    assert_eq!(parsed.authors, vec!["Doe; Lab", "Roe\\Unit"]);
    assert_eq!(parsed.url.as_deref(), Some("https://example.invalid/a;b"));
}

#[test]
fn citum_native_parser_keeps_unknown_escape_sequences_literal() {
    let parsed = parse_citum_native_summary(
        b"title: Literal \\x escape\n\
          author: Doe\\; Lab; Roe\n\
          year: 2020\n",
    )
    .unwrap();

    assert_eq!(parsed.title, "Literal \\x escape");
    assert_eq!(parsed.authors, vec!["Doe; Lab", "Roe"]);
}

#[test]
fn non_citum_source_bytes_are_preserved_for_future_adapters() {
    let mut item = reference("ref-doe", "Doe", "2020", "Example");
    item.source.format = CitationSourceFormat::CslJson;
    item.source.bytes = br#"{"id":"ref-doe"}"#.to_vec();

    assert_eq!(citation_source_bytes(&item), br#"{"id":"ref-doe"}"#);
}

#[test]
fn parses_citum_native_summary_for_import_fallbacks() {
    let parsed = parse_citum_native_summary(
        b"title: Example\n\
          author: Doe; Roe\n\
          year: 2020\n\
          doi: 10.123/example\n\
          url: https://example.invalid\n\
          unknown: ignored\n",
    )
    .unwrap();

    assert_eq!(parsed.title, "Example");
    assert_eq!(parsed.authors, vec!["Doe", "Roe"]);
    assert_eq!(parsed.issued.as_deref(), Some("2020"));
    assert_eq!(parsed.doi.as_deref(), Some("10.123/example"));
    assert_eq!(parsed.url.as_deref(), Some("https://example.invalid"));
}

#[test]
fn merges_missing_summary_fields_from_citum_native_source() {
    let summary = CitationSummary {
        title: "Provided".to_string(),
        authors: Vec::new(),
        issued: None,
        doi: None,
        url: None,
    };

    let merged = merge_summary_with_citum_native_source(
        summary,
        b"title: Source Title\nauthor: Doe; Roe\nyear: 2020\ndoi: 10.123/example\n",
    );

    assert_eq!(merged.title, "Provided");
    assert_eq!(merged.authors, vec!["Doe", "Roe"]);
    assert_eq!(merged.issued.as_deref(), Some("2020"));
    assert_eq!(merged.doi.as_deref(), Some("10.123/example"));
}

mod csl {
    use super::*;
    use opendoc_core::{CitationItem, CitationPlacement, CitationSource, StableId};

    const BIBTEX: &str = r#"
@article{zed2021,
  author = {Zed, Ada and Young, Bo and Xu, Cy},
  title = {Late alphabet study},
  journal = {Journal of Letters},
  volume = {7},
  number = {2},
  pages = {100--110},
  year = {2021},
  doi = {10.1/zed}
}
@book{smitha,
  author = {Smith, John},
  title = {Alpha book},
  publisher = {Press},
  address = {Oslo},
  year = {2020}
}
@book{smithb,
  author = {Smith, John},
  title = {Beta book},
  publisher = {Press},
  year = {2020}
}
@incollection{mueller,
  author = {Müller, Jürgen and Østergaard, Søren},
  title = {Über Umlaute},
  booktitle = {Handbuch},
  editor = {Roe, Rita},
  publisher = {Verlag},
  pages = {5--9},
  year = {2019}
}
@misc{adams,
  author = {Adams, Q.},
  title = {Never cited},
  year = {2000}
}
"#;

    const RIS: &str = "TY  - JOUR\nAU  - García, María\nAU  - O'Neil, Seán\nTI  - Accents in metadata\nJO  - Journal of Names\nPY  - 2018\nVL  - 3\nIS  - 4\nSP  - 12\nEP  - 34\nDO  - https://doi.org/10.5/ris\nER  - \n";

    const CSL_JSON: &str = r#"[{
  "id": "lee2022",
  "type": "article-journal",
  "title": "Graphs",
  "author": [{"family": "Lee", "given": "Min"}, {"family": "Ångström", "given": "Anders"}],
  "container-title": "Journal of Graphs",
  "volume": "9",
  "issue": "1",
  "page": "1-10",
  "issued": {"date-parts": [[2022, 3, 1]]},
  "DOI": "10.9/graphs",
  "URL": "https://example.org/graphs"
}, {
  "id": "org2001",
  "type": "report",
  "title": "Annual report",
  "author": [{"literal": "World Health Organization"}],
  "publisher": "WHO",
  "issued": {"raw": "2001"}
}]"#;

    fn source_reference(
        id: &str,
        format: CitationSourceFormat,
        bytes: &str,
    ) -> BibliographyReference {
        BibliographyReference {
            id: StableId::parse(id).unwrap(),
            revision: 1,
            source: CitationSource {
                format,
                bytes: bytes.as_bytes().to_vec(),
            },
            summary: CitationSummary {
                title: String::new(),
                authors: Vec::new(),
                issued: None,
                doi: None,
                url: None,
            },
            deleted: false,
        }
    }

    fn bibtex_reference(id: &str) -> BibliographyReference {
        let entry = BIBTEX
            .split("\n@")
            .find(|chunk| chunk.contains(&format!("{{{id},")))
            .map(|chunk| format!("@{}", chunk.trim_start_matches('@')))
            .expect("bibtex entry");
        source_reference(id, CitationSourceFormat::Bibtex, &entry)
    }

    fn item(id: &str) -> CitationItem {
        CitationItem {
            reference_id: StableId::parse(id).unwrap(),
            locator: None,
            label: None,
            prefix: None,
            suffix: None,
            suppress_author: false,
        }
    }

    fn located(id: &str, label: &str, locator: &str) -> CitationItem {
        CitationItem {
            locator: Some(locator.to_string()),
            label: Some(label.to_string()),
            ..item(id)
        }
    }

    fn group(id: &str, items: Vec<CitationItem>) -> CitationGroup {
        CitationGroup {
            id: StableId::parse(id).unwrap(),
            revision: 1,
            items,
            placement: CitationPlacement::Inline,
            rendered_cache: None,
            deleted: false,
        }
    }

    fn footnote_group(id: &str, items: Vec<CitationItem>) -> CitationGroup {
        CitationGroup {
            placement: CitationPlacement::Footnote {
                footnote_id: StableId::parse(format!("fn-{id}")).unwrap(),
            },
            ..group(id, items)
        }
    }

    fn database(style: &str) -> CitationDatabase {
        let mut database = CitationDatabase {
            style: style.to_string(),
            ..CitationDatabase::default()
        };
        for id in ["zed2021", "smitha", "smithb", "mueller", "adams"] {
            database.upsert_reference(bibtex_reference(id));
        }
        database
    }

    fn text(database: &CitationDatabase, group: &CitationGroup) -> String {
        render_citation(database, group).unwrap().text
    }

    #[test]
    fn parses_bibtex_with_unicode_names_and_multiple_authors() {
        let details = reference_details(&bibtex_reference("mueller"));
        assert_eq!(details.kind, "chapter");
        assert_eq!(details.title, "Über Umlaute");
        assert_eq!(details.authors.len(), 2);
        assert_eq!(details.authors[0].family, "Müller");
        assert_eq!(details.authors[0].given.as_deref(), Some("Jürgen"));
        assert_eq!(details.authors[1].family, "Østergaard");
        assert_eq!(details.authors[1].given.as_deref(), Some("Søren"));
        assert_eq!(details.editors[0].sort_name(), "Roe, Rita");
        assert_eq!(details.container_title.as_deref(), Some("Handbuch"));
        assert_eq!(details.pages.as_deref(), Some("5-9"));
        assert_eq!(details.publisher.as_deref(), Some("Verlag"));
        assert_eq!(details.year, Some(2019));

        let details = reference_details(&bibtex_reference("zed2021"));
        assert_eq!(details.kind, "article");
        assert_eq!(
            details
                .authors
                .iter()
                .map(ReferenceAuthor::display_name)
                .collect::<Vec<_>>(),
            vec!["Ada Zed", "Bo Young", "Cy Xu"]
        );
        assert_eq!(
            details.container_title.as_deref(),
            Some("Journal of Letters")
        );
        assert_eq!(details.volume.as_deref(), Some("7"));
        assert_eq!(details.issue.as_deref(), Some("2"));
        assert_eq!(details.pages.as_deref(), Some("100-110"));
        assert_eq!(details.doi.as_deref(), Some("10.1/zed"));

        let summary = summary_from_details(&details);
        assert_eq!(summary.authors, vec!["Ada Zed", "Bo Young", "Cy Xu"]);
        assert_eq!(summary.issued.as_deref(), Some("2021"));
        assert_eq!(summary.doi.as_deref(), Some("10.1/zed"));

        assert!(matches!(
            parse_bibtex("@article{broken, title = {Unclosed"),
            Err(ReferenceParseError::Bibtex(_))
        ));
    }

    #[test]
    fn parses_ris_records() {
        let details = parse_reference_details(&CitationSourceFormat::Ris, RIS.as_bytes()).unwrap();
        assert_eq!(details.len(), 1);
        let details = &details[0];
        assert_eq!(details.kind, "article");
        assert_eq!(details.title, "Accents in metadata");
        assert_eq!(details.authors[0].family, "García");
        assert_eq!(details.authors[0].given.as_deref(), Some("María"));
        assert_eq!(details.authors[1].family, "O'Neil");
        assert_eq!(details.authors[1].given.as_deref(), Some("Seán"));
        assert_eq!(details.container_title.as_deref(), Some("Journal of Names"));
        assert_eq!(details.volume.as_deref(), Some("3"));
        assert_eq!(details.issue.as_deref(), Some("4"));
        assert_eq!(details.pages.as_deref(), Some("12-34"));
        assert_eq!(details.year, Some(2018));
        assert_eq!(details.doi.as_deref(), Some("10.5/ris"));

        let two = format!("{RIS}TY  - BOOK\nAU  - Roe, Rita\nTI  - A Book\nPB  - Press\nCY  - Oslo\nPY  - 1999/05/01\nSN  - 978-3-16-148410-0\nER  - \n");
        let details = parse_reference_details(&CitationSourceFormat::Ris, two.as_bytes()).unwrap();
        assert_eq!(details.len(), 2);
        assert_eq!(details[1].kind, "book");
        assert_eq!(details[1].publisher.as_deref(), Some("Press"));
        assert_eq!(details[1].place.as_deref(), Some("Oslo"));
        assert_eq!(details[1].date.as_deref(), Some("1999-05-01"));
        assert_eq!(details[1].isbn.as_deref(), Some("978-3-16-148410-0"));

        assert!(matches!(
            parse_ris("just text"),
            Err(ReferenceParseError::Ris(_))
        ));
    }

    #[test]
    fn parses_csl_json_items() {
        let details =
            parse_reference_details(&CitationSourceFormat::CslJson, CSL_JSON.as_bytes()).unwrap();
        assert_eq!(details.len(), 2);
        assert_eq!(details[0].id, "lee2022");
        assert_eq!(details[0].kind, "article");
        assert_eq!(details[0].authors[0].sort_name(), "Lee, Min");
        assert_eq!(details[0].authors[1].family, "Ångström");
        assert_eq!(
            details[0].container_title.as_deref(),
            Some("Journal of Graphs")
        );
        assert_eq!(details[0].volume.as_deref(), Some("9"));
        assert_eq!(details[0].issue.as_deref(), Some("1"));
        assert_eq!(details[0].pages.as_deref(), Some("1-10"));
        assert_eq!(details[0].year, Some(2022));
        assert_eq!(details[0].date.as_deref(), Some("2022-03-01"));
        assert_eq!(details[0].doi.as_deref(), Some("10.9/graphs"));
        assert_eq!(
            details[0].url.as_deref(),
            Some("https://example.org/graphs")
        );

        assert_eq!(details[1].kind, "report");
        assert_eq!(details[1].authors[0].family, "World Health Organization");
        assert_eq!(details[1].authors[0].given, None);
        assert_eq!(details[1].publisher.as_deref(), Some("WHO"));
        assert_eq!(details[1].year, Some(2001));

        let single = parse_csl_json(r#"{"id": "x", "type": "book", "title": "Solo"}"#).unwrap();
        assert_eq!(single[0].key(), "x");
        assert!(matches!(
            parse_csl_json("[1]"),
            Err(ReferenceParseError::CslJson(_))
        ));
    }

    #[test]
    fn detects_source_formats() {
        assert_eq!(
            detect_source_format(BIBTEX.as_bytes()),
            CitationSourceFormat::Bibtex
        );
        assert_eq!(
            detect_source_format(RIS.as_bytes()),
            CitationSourceFormat::Ris
        );
        assert_eq!(
            detect_source_format(CSL_JSON.as_bytes()),
            CitationSourceFormat::CslJson
        );
        assert_eq!(
            detect_source_format(b"title: Example\nauthor: Doe\n"),
            CitationSourceFormat::CitumNative
        );
    }

    #[test]
    fn reference_details_fall_back_to_summary_for_native_sources() {
        let mut reference = source_reference("doe", CitationSourceFormat::CitumNative, "x");
        reference.summary = CitationSummary {
            title: "Example Article".to_string(),
            authors: vec!["Jane Q. Public".to_string(), "Doe".to_string()],
            issued: Some("2020".to_string()),
            doi: Some("10.0/example".to_string()),
            url: None,
        };
        let details = reference_details(&reference);
        assert_eq!(details.title, "Example Article");
        assert_eq!(details.authors[0].family, "Public");
        assert_eq!(details.authors[0].given.as_deref(), Some("Jane Q."));
        assert_eq!(details.authors[1].family, "Doe");
        assert_eq!(details.year, Some(2020));
        assert_eq!(details.doi.as_deref(), Some("10.0/example"));

        let mut database = CitationDatabase {
            style: "apa".to_string(),
            ..CitationDatabase::default()
        };
        database.upsert_reference(reference);
        let citation = group("c1", vec![located("doe", "page", "4")]);
        assert_eq!(text(&database, &citation), "(Public & Doe, 2020, p. 4)");
        let bibliography = render_bibliography(&database);
        assert_eq!(
            bibliography[0].text,
            "Public, J. Q., & Doe. (2020). Example Article. https://doi.org/10.0/example"
        );
    }

    #[test]
    fn styles_render_differently() {
        let citation = group("c1", vec![located("zed2021", "page", "12")]);
        let mut database = database("apa");
        assert_eq!(text(&database, &citation), "(Zed et al., 2021, p. 12)");
        database.style = "ieee".to_string();
        assert_eq!(text(&database, &citation), "[1, p. 12]");
        database.style = "chicago-author-date".to_string();
        assert_eq!(text(&database, &citation), "(Zed et al. 2021, 12)");
        database.style = "mla".to_string();
        assert_eq!(text(&database, &citation), "(Zed et al. 12)");
        database.style = "harvard".to_string();
        assert_eq!(
            text(&database, &citation),
            "(Zed, Young and Xu, 2021, p. 12)"
        );
        database.style = "vancouver".to_string();
        assert_eq!(text(&database, &citation), "(1)");
        database.style = "nature".to_string();
        assert_eq!(text(&database, &citation), "1");

        let two_authors = group("c2", vec![item("mueller")]);
        database.style = "apa".to_string();
        assert_eq!(text(&database, &two_authors), "(Müller & Østergaard, 2019)");
    }

    #[test]
    fn bibliographies_differ_per_style_and_carry_rich_formatting() {
        let mut database = database("apa");
        database.citations.push(group("c1", vec![item("zed2021")]));
        database.citations.push(group("c2", vec![item("mueller")]));

        let apa = render_bibliography_rich(&database).unwrap();
        let zed = apa
            .iter()
            .find(|entry| entry.reference_id == "zed2021")
            .unwrap();
        assert_eq!(
            zed.text,
            "Zed, A., Young, B., & Xu, C. (2021). Late alphabet study. Journal of Letters, 7(2), 100–110. https://doi.org/10.1/zed"
        );
        assert!(zed
            .rich
            .iter()
            .any(|segment| segment.italic && segment.text == "Journal of Letters"));
        assert!(zed
            .rich
            .iter()
            .any(|segment| segment.link.as_deref() == Some("https://doi.org/10.1/zed")));
        let mueller = apa
            .iter()
            .find(|entry| entry.reference_id == "mueller")
            .unwrap();
        assert_eq!(
            mueller.text,
            "Müller, J., & Østergaard, S. (2019). Über Umlaute. In R. Roe (Ed.), Handbuch (pp. 5–9). Verlag."
        );
        assert!(apa.iter().all(|entry| entry.number.is_none()));

        database.style = "ieee".to_string();
        let ieee = render_bibliography_rich(&database).unwrap();
        assert_eq!(ieee[0].number, Some(1));
        assert_eq!(
            ieee[0].text,
            "[1] A. Zed, B. Young, and C. Xu, “Late alphabet study,” Journal of Letters, vol. 7, no. 2, pp. 100–110, 2021, doi: 10.1/zed."
        );

        database.style = "chicago-author-date".to_string();
        let chicago = render_bibliography(&database);
        let zed = chicago
            .iter()
            .find(|entry| entry.reference_id == "zed2021")
            .unwrap();
        assert_eq!(
            zed.text,
            "Zed, Ada, Bo Young, and Cy Xu. 2021. “Late Alphabet Study.” Journal of Letters 7 (2): 100–110. https://doi.org/10.1/zed."
        );
    }

    #[test]
    fn disambiguates_same_author_and_year() {
        let mut database = database("apa");
        database.citations.push(group("c1", vec![item("smitha")]));
        database.citations.push(group("c2", vec![item("smithb")]));
        let rendered = render_citations(&database).unwrap();
        assert_eq!(rendered[0].text, "(Smith, 2020a)");
        assert_eq!(rendered[1].text, "(Smith, 2020b)");
        let bibliography = render_bibliography(&database);
        assert!(bibliography
            .iter()
            .any(|entry| entry.text.starts_with("Smith, J. (2020a). Alpha book")));
        assert!(bibliography
            .iter()
            .any(|entry| entry.text.starts_with("Smith, J. (2020b). Beta book")));

        let grouped = group("c3", vec![item("smitha"), item("smithb")]);
        assert_eq!(text(&database, &grouped), "(Smith, 2020a; 2020b)");
    }

    #[test]
    fn renders_locators_with_labels() {
        let database = database("apa");
        assert_eq!(
            text(
                &database,
                &group("c1", vec![located("zed2021", "page", "12")])
            ),
            "(Zed et al., 2021, p. 12)"
        );
        assert_eq!(
            text(
                &database,
                &group("c1", vec![located("zed2021", "pp.", "12-14")])
            ),
            "(Zed et al., 2021, pp. 12–14)"
        );
        assert_eq!(
            text(
                &database,
                &group("c1", vec![located("zed2021", "chapter", "3")])
            ),
            "(Zed et al., 2021, Chapter 3)"
        );
        assert_eq!(
            text(
                &database,
                &group("c1", vec![located("zed2021", "slide", "4")])
            ),
            "(Zed et al., 2021, slide 4)"
        );
        let unlabeled = CitationItem {
            locator: Some("9".to_string()),
            ..item("zed2021")
        };
        assert_eq!(
            text(&database, &group("c1", vec![unlabeled])),
            "(Zed et al., 2021, p. 9)"
        );

        // Year suffixes are assigned against the whole reference list, so a
        // lone citation of one of two Smith 2020 books is still disambiguated.
        assert_eq!(
            text(
                &database,
                &group("c1", vec![located("smitha", "page", "12")])
            ),
            "(Smith, 2020a, p. 12)"
        );
    }

    #[test]
    fn suppresses_author_and_applies_affixes() {
        let mut database = database("apa");
        let suppressed = CitationItem {
            suppress_author: true,
            ..located("zed2021", "page", "7")
        };
        assert_eq!(
            text(&database, &group("c1", vec![suppressed.clone()])),
            "(2021, p. 7)"
        );

        let affixed = CitationItem {
            prefix: Some("see".to_string()),
            suffix: Some("for details".to_string()),
            ..located("zed2021", "page", "7")
        };
        assert_eq!(
            text(&database, &group("c1", vec![affixed.clone()])),
            "(see Zed et al., 2021, p. 7 for details)"
        );

        database.style = "chicago-author-date".to_string();
        assert_eq!(
            text(&database, &group("c1", vec![suppressed.clone()])),
            "(2021, 7)"
        );

        database.style = "ieee".to_string();
        assert_eq!(text(&database, &group("c1", vec![suppressed])), "[1, p. 7]");
        assert_eq!(
            text(&database, &group("c1", vec![affixed])),
            "see [1, p. 7] for details"
        );
    }

    #[test]
    fn numeric_styles_number_by_first_appearance_and_sort_bibliography_by_number() {
        let mut database = database("ieee");
        database.citations.push(group("c1", vec![item("smithb")]));
        database
            .citations
            .push(group("c2", vec![item("zed2021"), item("smithb")]));
        database.citations.push(group("c3", vec![item("mueller")]));
        let rendered = render_database(&database).unwrap();
        assert!(rendered.numeric);
        assert_eq!(rendered.citations[0].text, "[1]");
        assert_eq!(rendered.citations[1].text, "[1], [2]");
        assert_eq!(rendered.citations[2].text, "[3]");
        assert_eq!(
            rendered
                .bibliography
                .iter()
                .map(|entry| (entry.reference_id.as_str(), entry.number))
                .collect::<Vec<_>>(),
            vec![
                ("smithb", Some(1)),
                ("zed2021", Some(2)),
                ("mueller", Some(3)),
                ("adams", Some(4)),
                ("smitha", Some(5)),
            ]
        );

        // A group that is not stored yet is numbered after the stored ones.
        let pending = group("c4", vec![item("smitha")]);
        assert_eq!(text(&database, &pending), "[4]");
        let pending = group("c4", vec![item("adams")]);
        assert_eq!(text(&database, &pending), "[4]");
    }

    #[test]
    fn author_date_bibliographies_sort_alphabetically_and_include_uncited_references() {
        let mut database = database("apa");
        database.citations.push(group("c1", vec![item("zed2021")]));
        database.citations.push(group("c2", vec![item("smithb")]));
        let bibliography = render_bibliography(&database);
        assert_eq!(
            bibliography
                .iter()
                .map(|entry| entry.reference_id.as_str())
                .collect::<Vec<_>>(),
            vec!["adams", "mueller", "smitha", "smithb", "zed2021"]
        );

        database.style = "chicago-notes".to_string();
        let bibliography = render_bibliography(&database);
        let adams = bibliography
            .iter()
            .find(|entry| entry.reference_id == "adams")
            .unwrap();
        assert!(
            !adams.text.is_empty(),
            "misc entries fall back to a plain entry"
        );
    }

    #[test]
    fn locales_change_terms() {
        let citation = group("c1", vec![located("zed2021", "page", "12")]);
        let chapter = group("c2", vec![located("zed2021", "chapter", "3")]);
        let mut database = database("apa");
        database.locale = "en-US".to_string();
        assert_eq!(text(&database, &citation), "(Zed et al., 2021, p. 12)");
        database.locale = "en-GB".to_string();
        assert_eq!(text(&database, &citation), "(Zed et al., 2021, p. 12)");
        database.locale = "de-DE".to_string();
        assert_eq!(text(&database, &citation), "(Zed et al., 2021, S. 12)");
        assert_eq!(text(&database, &chapter), "(Zed et al., 2021, Kapitel 3)");
        database.locale = "fr-FR".to_string();
        assert_eq!(text(&database, &chapter), "(Zed et al., 2021, Chapitre 3)");
        database.locale = "sv-SE".to_string();
        assert_eq!(text(&database, &citation), "(Zed m.fl., 2021, s. 12)");
        database.locale = "sv".to_string();
        assert_eq!(text(&database, &citation), "(Zed m.fl., 2021, s. 12)");
        database.locale = "xx-XX".to_string();
        assert_eq!(text(&database, &citation), "(Zed et al., 2021, p. 12)");

        database.style = "harvard".to_string();
        database.locale = "de-DE".to_string();
        assert_eq!(
            text(&database, &citation),
            "(Zed, Young und Xu, 2021, S. 12)"
        );

        for locale in ["en-US", "en-GB", "de-DE", "fr-FR", "sv-SE"] {
            assert!(available_locales().iter().any(|code| code == locale));
        }
        assert_eq!(resolve_locale("de").0, "de-DE");
        assert_eq!(resolve_locale("nonsense").0, "en-US");
    }

    #[test]
    fn note_styles_render_footnotes_with_numbers() {
        let mut database = database("chicago-notes");
        database
            .citations
            .push(footnote_group("c1", vec![item("zed2021")]));
        database
            .citations
            .push(footnote_group("c2", vec![located("zed2021", "page", "12")]));
        let rendered = render_database(&database).unwrap();
        assert!(rendered.note);
        assert_eq!(rendered.citations[0].note_number, Some(1));
        assert_eq!(
            rendered.citations[0].text,
            "Ada Zed et al., “Late Alphabet Study,” Journal of Letters 7, no. 2 (2021): 100–110, https://doi.org/10.1/zed."
        );
        assert_eq!(rendered.citations[1].note_number, Some(2));
        assert_eq!(
            rendered.citations[1].text,
            "Zed et al., “Late Alphabet Study,” 12."
        );

        let inline = group("c3", vec![item("smitha")]);
        let footnote = render_footnote_citation(&database, &inline).unwrap();
        assert_eq!(footnote.note_number, Some(3));
        assert_eq!(footnote.text, "John Smith, Alpha Book (Press, 2020).");

        // In-text styles render footnote-placed citations like inline ones.
        database.style = "apa".to_string();
        let rendered = render_database(&database).unwrap();
        assert!(!rendered.note);
        assert_eq!(rendered.citations[0].text, "(Zed et al., 2021)");
        assert_eq!(rendered.citations[0].note_number, Some(1));
    }

    #[test]
    fn missing_or_uncitable_references_render_placeholders() {
        let mut database = database("apa");
        let missing = group("cite-missing", vec![item("nope")]);
        let rendered = render_citation(&database, &missing).unwrap();
        assert!(!rendered.resolved);
        assert_eq!(rendered.text, "[cite-missing]");
        assert_eq!(render_citation_group(&database, &missing), "[cite-missing]");

        database.upsert_reference(BibliographyReference {
            summary: CitationSummary {
                title: "Title Only".to_string(),
                authors: Vec::new(),
                issued: None,
                doi: None,
                url: None,
            },
            ..source_reference(
                "title-only",
                CitationSourceFormat::CitumNative,
                "title: Title Only",
            )
        });
        let title_only = group("cite-title", vec![item("title-only")]);
        assert_eq!(
            render_citation_group(&database, &title_only),
            "[cite-title]"
        );
        database.style = "ieee".to_string();
        // The legacy numeric renderer numbers by position in the reference
        // list, which is kept sorted by id.
        assert_eq!(render_citation_group(&database, &title_only), "[5]");
        database.style = "vancouver".to_string();
        assert_eq!(render_citation_group(&database, &title_only), "(1)");
    }

    #[test]
    fn lists_styles_and_resolves_names() {
        let styles = available_styles();
        assert_eq!(
            styles
                .iter()
                .map(|style| style.name.as_str())
                .collect::<Vec<_>>(),
            vec![
                "apa",
                "mla",
                "chicago-author-date",
                "chicago-notes",
                "ieee",
                "vancouver",
                "harvard",
                "nature"
            ]
        );
        let ieee = styles.iter().find(|style| style.name == "ieee").unwrap();
        assert_eq!(ieee.label, "IEEE");
        assert!(ieee.numeric);
        assert!(!ieee.note);
        let chicago = styles
            .iter()
            .find(|style| style.name == "chicago-notes")
            .unwrap();
        assert!(chicago.note);
        assert!(!chicago.numeric);
        assert_eq!(styles[0].label, "APA (7th edition)");
        assert!(styles[0].csl_id.contains("apa"));

        assert_eq!(resolve_style_name("APA").as_deref(), Some("apa"));
        assert_eq!(
            resolve_style_name("Chicago").as_deref(),
            Some("chicago-author-date")
        );
        assert_eq!(
            resolve_style_name("harvard-cite-them-right").as_deref(),
            Some("harvard")
        );
        assert_eq!(
            resolve_style_name("american-chemical-society").as_deref(),
            Some("american-chemical-society")
        );
        assert_eq!(resolve_style_name("apa-7th"), None);
        assert_eq!(resolve_style_name("numeric"), None);
        assert!(style_uses_csl("apa"));
        assert!(style_uses_csl("vancouver"));
        assert!(!style_uses_csl("apa-7th"));
        assert!(!style_uses_csl("ieee"), "ieee keeps legacy wrapper output");
        assert!(is_legacy_wrapper_style("IEEE"));
        assert!(all_style_names().len() > 8);
        assert_eq!(
            load_style("made-up").unwrap_err(),
            CitationError::UnknownStyle("made-up".to_string())
        );
    }

    #[test]
    fn unknown_styles_keep_legacy_rendering() {
        let mut database = database("apa-7th");
        let mut reference = bibtex_reference("smitha");
        reference.summary = summary_from_details(&reference_details(&reference));
        database.upsert_reference(reference);
        let citation = group("c1", vec![located("smitha", "page", "7")]);
        assert_eq!(
            render_citation_group(&database, &citation),
            "(John Smith 2020, page 7)"
        );
        assert_eq!(
            render_bibliography(&database)
                .iter()
                .find(|entry| entry.reference_id == "smitha")
                .unwrap()
                .text,
            "John Smith (2020). Alpha book."
        );
        assert_eq!(
            render_citation(&database, &citation).unwrap_err(),
            CitationError::UnknownStyle("apa-7th".to_string())
        );
        database.style = "apa".to_string();
        assert_eq!(
            render_citation_group(&database, &citation),
            "(Smith, 2020a, p. 7)"
        );
    }
}
