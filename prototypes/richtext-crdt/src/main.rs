#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct ElemId {
    actor: &'static str,
    seq: u64,
}

#[derive(Clone, Debug)]
struct CharElem {
    id: ElemId,
    ch: char,
    deleted: bool,
}

#[derive(Clone, Debug)]
struct Mark {
    name: &'static str,
    start: ElemId,
    end: ElemId,
}

#[derive(Clone, Debug)]
struct CitationAnchor {
    id: &'static str,
    after: ElemId,
}

#[derive(Default, Clone, Debug)]
struct Doc {
    chars: Vec<CharElem>,
    marks: Vec<Mark>,
    citations: Vec<CitationAnchor>,
}

impl Doc {
    fn insert_after(&mut self, after: Option<&ElemId>, id: ElemId, text: &str) {
        let mut insert_at = after
            .and_then(|target| self.chars.iter().position(|elem| &elem.id == target))
            .map(|idx| idx + 1)
            .unwrap_or(0);

        for (offset, ch) in text.chars().enumerate() {
            self.chars.insert(
                insert_at,
                CharElem {
                    id: ElemId {
                        actor: id.actor,
                        seq: id.seq + offset as u64,
                    },
                    ch,
                    deleted: false,
                },
            );
            insert_at += 1;
        }
    }

    fn merge(&self, other: &Doc) -> Doc {
        let mut merged = self.clone();
        for elem in &other.chars {
            if !merged.chars.iter().any(|existing| existing.id == elem.id) {
                merged.chars.push(elem.clone());
            }
        }
        merged.chars.sort_by(|left, right| left.id.cmp(&right.id));

        for mark in &other.marks {
            if !merged.marks.iter().any(|existing| {
                existing.name == mark.name
                    && existing.start == mark.start
                    && existing.end == mark.end
            }) {
                merged.marks.push(mark.clone());
            }
        }

        for citation in &other.citations {
            if !merged
                .citations
                .iter()
                .any(|existing| existing.id == citation.id)
            {
                merged.citations.push(citation.clone());
            }
        }
        merged
    }

    fn visible_text(&self) -> String {
        self.chars
            .iter()
            .filter(|elem| !elem.deleted)
            .map(|elem| elem.ch)
            .collect()
    }
}

fn main() {
    let mut alice = Doc::default();
    alice.insert_after(
        None,
        ElemId {
            actor: "alice",
            seq: 1,
        },
        "Hello",
    );
    let anchor = alice.chars.last().unwrap().id.clone();
    alice.citations.push(CitationAnchor {
        id: "cit_1",
        after: anchor.clone(),
    });

    let mut bob = alice.clone();
    alice.insert_after(
        Some(&anchor),
        ElemId {
            actor: "alice",
            seq: 100,
        },
        " world",
    );
    bob.insert_after(
        Some(&anchor),
        ElemId {
            actor: "bob",
            seq: 100,
        },
        " cited",
    );

    let start = alice.chars.first().unwrap().id.clone();
    let end = alice.chars.last().unwrap().id.clone();
    alice.marks.push(Mark {
        name: "bold",
        start,
        end,
    });

    let merged_ab = alice.merge(&bob);
    let merged_ba = bob.merge(&alice);

    println!("merged_ab={}", merged_ab.visible_text());
    println!("merged_ba={}", merged_ba.visible_text());
    println!(
        "converged={}",
        merged_ab.visible_text() == merged_ba.visible_text()
    );
    println!("citations={}", merged_ab.citations.len());
    println!("citation_after={:?}", merged_ab.citations[0].after);
    println!("marks={}", merged_ab.marks.len());
}
