struct CslItem {
    id: &'static str,
    author_family: &'static str,
    year: u16,
    title: &'static str,
}

struct Citation {
    prefix: &'static str,
    item_id: &'static str,
    locator: Option<&'static str>,
    suppress_author: bool,
}

fn render_citation(citation: &Citation, item: &CslItem, style: &str) -> String {
    let locator = citation
        .locator
        .map(|value| format!(", {value}"))
        .unwrap_or_default();
    let prefix = if citation.prefix.is_empty() {
        String::new()
    } else {
        format!("{} ", citation.prefix)
    };
    match (style, citation.suppress_author) {
        ("numeric", _) => "[1]".to_string(),
        (_, true) => format!("({}{})", prefix, item.year),
        _ => format!(
            "({}{} {}{})",
            prefix, item.author_family, item.year, locator
        ),
    }
}

fn render_bibliography(item: &CslItem, style: &str) -> String {
    match style {
        "numeric" => format!("[1] {}. {}. {}", item.author_family, item.title, item.year),
        _ => format!("{} ({}). {}", item.author_family, item.year, item.title),
    }
}

fn main() {
    let item = CslItem {
        id: "doe-2020",
        author_family: "Doe",
        year: 2020,
        title: "Example Article",
    };
    let citation = Citation {
        prefix: "see",
        item_id: item.id,
        locator: Some("42"),
        suppress_author: false,
    };

    println!("item_id={}", citation.item_id);
    println!("apa_citation={}", render_citation(&citation, &item, "apa"));
    println!("apa_bibliography={}", render_bibliography(&item, "apa"));
    println!(
        "numeric_citation={}",
        render_citation(&citation, &item, "numeric")
    );
    println!(
        "numeric_bibliography={}",
        render_bibliography(&item, "numeric")
    );
}
