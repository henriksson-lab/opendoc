#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct RowId {
    actor: &'static str,
    seq: u64,
}

#[derive(Clone, Debug)]
struct Row {
    id: RowId,
    label: &'static str,
}

#[derive(Clone, Debug)]
struct Formula {
    cell: RowId,
    expression: String,
    references: Vec<RowId>,
}

#[derive(Default, Clone, Debug)]
struct Sheet {
    rows: Vec<Row>,
    formulas: Vec<Formula>,
}

impl Sheet {
    fn insert_row(&mut self, id: RowId, label: &'static str) {
        if !self.rows.iter().any(|row| row.id == id) {
            self.rows.push(Row { id, label });
            self.rows.sort_by(|left, right| left.id.cmp(&right.id));
        }
    }

    fn merge(&self, other: &Sheet) -> Sheet {
        let mut merged = self.clone();
        for row in &other.rows {
            merged.insert_row(row.id.clone(), row.label);
        }
        for formula in &other.formulas {
            if !merged
                .formulas
                .iter()
                .any(|existing| existing.cell == formula.cell)
            {
                merged.formulas.push(formula.clone());
            }
        }
        merged
    }

    fn visible_ref(&self, row_id: &RowId, column: char) -> String {
        let row_index = self
            .rows
            .iter()
            .position(|row| &row.id == row_id)
            .map(|idx| idx + 1)
            .unwrap_or(0);
        format!("{column}{row_index}")
    }
}

fn main() {
    let mut alice = Sheet::default();
    alice.insert_row(
        RowId {
            actor: "base",
            seq: 1,
        },
        "header",
    );
    alice.insert_row(
        RowId {
            actor: "base",
            seq: 2,
        },
        "value",
    );

    let mut bob = alice.clone();
    alice.insert_row(
        RowId {
            actor: "alice",
            seq: 3,
        },
        "alice row",
    );
    bob.insert_row(
        RowId {
            actor: "bob",
            seq: 3,
        },
        "bob row",
    );

    let target = RowId {
        actor: "base",
        seq: 2,
    };
    alice.formulas.push(Formula {
        cell: RowId {
            actor: "alice",
            seq: 3,
        },
        expression: "=B(base:2)".to_string(),
        references: vec![target.clone()],
    });

    let merged_ab = alice.merge(&bob);
    let merged_ba = bob.merge(&alice);
    let labels_ab: Vec<_> = merged_ab.rows.iter().map(|row| row.label).collect();
    let labels_ba: Vec<_> = merged_ba.rows.iter().map(|row| row.label).collect();

    println!("rows={labels_ab:?}");
    println!("converged={}", labels_ab == labels_ba);
    println!("formula_ref={}", merged_ab.visible_ref(&target, 'B'));
    println!("formula_expression={}", merged_ab.formulas[0].expression);
    println!(
        "formula_dependencies={}",
        merged_ab.formulas[0].references.len()
    );
}
