//! Workbook-wide recalculation: one dependency graph across all sheets,
//! iterative (stack-safe) evaluation order, cycle detection, incremental
//! re-evaluation, array spill, and projection write-back.

use std::cell::{Cell as StdCell, RefCell};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::address::{column_to_number, number_to_column, split_cell_address};
use super::format::{self, Locale};
use super::formula::{
    parse_formula_source, static_dependencies, static_ranges, Evaluator, Expr, FormulaEnv,
    ResolvedRange,
};
use super::model::{graph_dependency_target, graph_dependent_label};
use super::value::{FormulaArray, FormulaError, FormulaValue};
use crate::{Cell, CellDependency, NamedRange, Sheet, SpreadsheetWarning, SpreadsheetWorkbook};

/// Maximum on-demand evaluation nesting (dynamic references such as
/// `INDIRECT`); the static graph handles arbitrarily long chains.
const MAX_DYNAMIC_DEPTH: usize = 128;
/// Maximum spill re-evaluation passes.
const MAX_SPILL_PASSES: usize = 3;

/// Deterministic clock and random seed for volatile functions. Tests set
/// this explicitly; otherwise evaluation uses the real clock.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct SpreadsheetEvaluationContext {
    pub now_ms: u64,
    pub seed: u64,
}

/// Snapshot of the source state used to detect what changed between
/// evaluations. Not serialized; compares equal so it never affects
/// workbook equality.
#[derive(Clone, Default)]
pub struct RecalcState {
    snapshot: Option<Arc<RecalcSnapshot>>,
}

impl PartialEq for RecalcState {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl Eq for RecalcState {}

impl std::fmt::Debug for RecalcState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "RecalcState({})",
            if self.snapshot.is_some() {
                "evaluated"
            } else {
                "stale"
            }
        )
    }
}

struct RecalcSnapshot {
    structure: String,
    cells: HashMap<(String, String), (String, String)>,
}

fn structure_fingerprint(workbook: &SpreadsheetWorkbook) -> String {
    let mut out = String::new();
    for sheet in &workbook.sheets {
        out.push_str(&sheet.id);
        out.push('\u{1}');
        out.push_str(&sheet.title);
        out.push('\u{1}');
        for row in &sheet.hidden_rows {
            out.push_str(row);
            out.push(',');
        }
        out.push('\u{2}');
    }
    for range in &workbook.named_ranges {
        out.push_str(&range.name);
        out.push('=');
        out.push_str(&range.sheet_id);
        out.push('!');
        out.push_str(&range.range);
        out.push('\u{2}');
    }
    out
}

fn build_snapshot(workbook: &SpreadsheetWorkbook) -> RecalcSnapshot {
    let mut cells = HashMap::new();
    for sheet in &workbook.sheets {
        for cell in &sheet.cells {
            if cell.user_kind == "empty" && cell.spill_source.is_none() {
                continue;
            }
            cells.insert(
                (sheet.id.clone(), cell.address.clone()),
                (cell.user_kind.clone(), cell.user_value.clone()),
            );
        }
    }
    RecalcSnapshot {
        structure: structure_fingerprint(workbook),
        cells,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct CellKey {
    sheet: usize,
    col: u32,
    row: u32,
}

impl CellKey {
    fn address(&self) -> String {
        format!(
            "{}{}",
            number_to_column(self.col).unwrap_or_default(),
            self.row
        )
    }
}

fn parse_address(address: &str) -> Option<(u32, u32)> {
    let (column, row) = split_cell_address(address);
    Some((column_to_number(&column)?, row.parse::<u32>().ok()?))
}

struct SheetIndex {
    cells: HashMap<(u32, u32), usize>,
    max_col: u32,
    max_row: u32,
    hidden_rows: HashSet<u32>,
}

struct CellResult {
    value: FormulaValue,
    array: Option<FormulaArray>,
    dependencies: BTreeSet<String>,
    cycle: bool,
}

/// Evaluation environment over an immutable workbook view.
struct WorkbookEnv<'a> {
    sheets: &'a [Sheet],
    index: Vec<SheetIndex>,
    by_id: HashMap<String, usize>,
    by_title: HashMap<String, usize>,
    named: HashMap<String, ResolvedRange>,
    values: RefCell<HashMap<CellKey, FormulaValue>>,
    formulas: HashMap<CellKey, Result<Rc<Expr>, FormulaError>>,
    pending: RefCell<HashSet<CellKey>>,
    in_progress: RefCell<HashSet<CellKey>>,
    depth: StdCell<usize>,
    results: RefCell<HashMap<CellKey, CellResult>>,
    now_ms: u64,
    seed: u64,
    locale: String,
}

impl<'a> WorkbookEnv<'a> {
    fn new(
        sheets: &'a [Sheet],
        named_ranges: &[NamedRange],
        now_ms: u64,
        seed: u64,
        locale: &str,
    ) -> Self {
        let mut index = Vec::with_capacity(sheets.len());
        let mut by_id = HashMap::new();
        let mut by_title = HashMap::new();
        for (sheet_index, sheet) in sheets.iter().enumerate() {
            by_id.insert(sheet.id.clone(), sheet_index);
            by_title.insert(sheet.title.clone(), sheet_index);
            let mut cells = HashMap::with_capacity(sheet.cells.len());
            let mut max_col = 0;
            let mut max_row = 0;
            for (cell_index, cell) in sheet.cells.iter().enumerate() {
                if let Some((col, row)) = parse_address(&cell.address) {
                    cells.insert((col, row), cell_index);
                    if cell.user_kind != "empty" || cell.spill_source.is_some() {
                        max_col = max_col.max(col);
                        max_row = max_row.max(row);
                    }
                }
            }
            for column in &sheet.columns {
                if let Some(col) = column_to_number(column) {
                    max_col = max_col.max(col);
                }
            }
            for row in &sheet.rows {
                if let Ok(row) = row.parse::<u32>() {
                    max_row = max_row.max(row);
                }
            }
            let hidden_rows = sheet
                .hidden_rows
                .iter()
                .filter_map(|row| row.parse::<u32>().ok())
                .collect();
            index.push(SheetIndex {
                cells,
                max_col,
                max_row,
                hidden_rows,
            });
        }
        let mut named = HashMap::new();
        for range in named_ranges {
            let Some(sheet) = by_id.get(&range.sheet_id).copied() else {
                continue;
            };
            let (start, end) = range
                .range
                .split_once(':')
                .unwrap_or((&range.range, &range.range));
            let (Some((c1, r1)), Some((c2, r2))) = (parse_address(start), parse_address(end))
            else {
                continue;
            };
            named.insert(
                range.name.to_ascii_uppercase(),
                ResolvedRange {
                    sheet,
                    col_start: c1.min(c2),
                    col_end: c1.max(c2),
                    row_start: r1.min(r2),
                    row_end: r1.max(r2),
                },
            );
        }
        Self {
            sheets,
            index,
            by_id,
            by_title,
            named,
            values: RefCell::new(HashMap::new()),
            formulas: HashMap::new(),
            pending: RefCell::new(HashSet::new()),
            in_progress: RefCell::new(HashSet::new()),
            depth: StdCell::new(0),
            results: RefCell::new(HashMap::new()),
            now_ms,
            seed,
            locale: locale.to_string(),
        }
    }

    fn cell(&self, key: CellKey) -> Option<&'a Cell> {
        let index = self.index.get(key.sheet)?;
        let cell_index = *index.cells.get(&(key.col, key.row))?;
        self.sheets[key.sheet].cells.get(cell_index)
    }

    fn source_value(cell: &Cell) -> FormulaValue {
        match cell.user_kind.as_str() {
            "number" => cell
                .user_value
                .parse::<f64>()
                .ok()
                .filter(|value| value.is_finite())
                .map(FormulaValue::Number)
                .unwrap_or(FormulaValue::Error(FormulaError::Value)),
            "bool" => FormulaValue::Bool(cell.user_value.eq_ignore_ascii_case("true")),
            "string" => FormulaValue::Text(cell.user_value.clone()),
            _ => FormulaValue::Blank,
        }
    }

    /// Evaluates a pending formula cell immediately (used for dynamic
    /// references and as the worker for the static evaluation order).
    fn evaluate_now(&self, key: CellKey) -> FormulaValue {
        if self.in_progress.borrow().contains(&key) {
            return FormulaValue::Error(FormulaError::Cycle);
        }
        if self.depth.get() >= MAX_DYNAMIC_DEPTH {
            return FormulaValue::Error(FormulaError::Ref);
        }
        self.in_progress.borrow_mut().insert(key);
        self.depth.set(self.depth.get() + 1);
        let result = self.evaluate_cell(key);
        self.depth.set(self.depth.get() - 1);
        self.in_progress.borrow_mut().remove(&key);
        let value = result.value.clone();
        self.values.borrow_mut().insert(key, value.clone());
        self.pending.borrow_mut().remove(&key);
        self.results.borrow_mut().insert(key, result);
        value
    }

    fn evaluate_cell(&self, key: CellKey) -> CellResult {
        let expr = match self.formulas.get(&key) {
            Some(Ok(expr)) => Rc::clone(expr),
            Some(Err(error)) => {
                return CellResult {
                    value: FormulaValue::Error(*error),
                    array: None,
                    dependencies: BTreeSet::new(),
                    cycle: false,
                }
            }
            None => {
                return CellResult {
                    value: FormulaValue::Blank,
                    array: None,
                    dependencies: BTreeSet::new(),
                    cycle: false,
                }
            }
        };
        let mut evaluator = Evaluator::new(self, key.sheet, key.col, key.row);
        let value = evaluator.eval(&expr);
        let dependencies = std::mem::take(&mut evaluator.dependencies);
        let (value, array) = match value {
            FormulaValue::Array(array) => {
                let spills = !matches!(expr.as_ref(), Expr::Binary { .. } | Expr::Unary { .. });
                let scalar = array
                    .values
                    .first()
                    .cloned()
                    .unwrap_or(FormulaValue::Error(FormulaError::Ref));
                if spills && array.values.len() > 1 {
                    (scalar, Some(array))
                } else {
                    (scalar, None)
                }
            }
            other => (other, None),
        };
        let cycle = matches!(value, FormulaValue::Error(FormulaError::Cycle));
        CellResult {
            value,
            array,
            dependencies,
            cycle,
        }
    }

    fn mark_cycle(&self, key: CellKey) {
        self.values
            .borrow_mut()
            .insert(key, FormulaValue::Error(FormulaError::Cycle));
        self.pending.borrow_mut().remove(&key);
        self.results.borrow_mut().insert(
            key,
            CellResult {
                value: FormulaValue::Error(FormulaError::Cycle),
                array: None,
                dependencies: BTreeSet::new(),
                cycle: true,
            },
        );
    }
}

impl FormulaEnv for WorkbookEnv<'_> {
    fn resolve_sheet(&self, prefix: &str) -> Option<usize> {
        self.by_id
            .get(prefix)
            .or_else(|| self.by_title.get(prefix))
            .copied()
    }

    fn sheet_title(&self, sheet: usize) -> String {
        self.sheets
            .get(sheet)
            .map(|sheet| sheet.title.clone())
            .unwrap_or_default()
    }

    fn cell_value(&self, sheet: usize, col: u32, row: u32) -> FormulaValue {
        let key = CellKey { sheet, col, row };
        if let Some(value) = self.values.borrow().get(&key) {
            return value.clone();
        }
        if self.pending.borrow().contains(&key) {
            return self.evaluate_now(key);
        }
        FormulaValue::Blank
    }

    fn cell_meta(&self, sheet: usize, col: u32, row: u32) -> Option<&Cell> {
        self.cell(CellKey { sheet, col, row })
    }

    fn sheet_extent(&self, sheet: usize) -> (u32, u32) {
        self.index
            .get(sheet)
            .map(|index| (index.max_col, index.max_row))
            .unwrap_or((0, 0))
    }

    fn named_range(&self, name: &str) -> Option<ResolvedRange> {
        self.named.get(&name.to_ascii_uppercase()).copied()
    }

    fn row_hidden(&self, sheet: usize, row: u32) -> bool {
        self.index
            .get(sheet)
            .is_some_and(|index| index.hidden_rows.contains(&row))
    }

    fn now_ms(&self) -> u64 {
        self.now_ms
    }

    fn seed(&self) -> u64 {
        self.seed
    }

    fn locale(&self) -> String {
        self.locale.clone()
    }
}

/// Strongly connected components in reverse topological order
/// (dependencies before dependents), computed iteratively.
fn tarjan_order(nodes: &[CellKey], edges: &HashMap<CellKey, Vec<CellKey>>) -> Vec<Vec<CellKey>> {
    let mut index_of: HashMap<CellKey, usize> = HashMap::with_capacity(nodes.len());
    let mut lowlink: HashMap<CellKey, usize> = HashMap::with_capacity(nodes.len());
    let mut on_stack: HashSet<CellKey> = HashSet::with_capacity(nodes.len());
    let mut stack: Vec<CellKey> = Vec::new();
    let mut components = Vec::new();
    let mut next_index = 0usize;
    let empty = Vec::new();
    for &root in nodes {
        if index_of.contains_key(&root) {
            continue;
        }
        // (node, next edge position)
        let mut work: Vec<(CellKey, usize)> = vec![(root, 0)];
        index_of.insert(root, next_index);
        lowlink.insert(root, next_index);
        next_index += 1;
        stack.push(root);
        on_stack.insert(root);
        while let Some(&mut (node, ref mut position)) = work.last_mut() {
            let neighbours = edges.get(&node).unwrap_or(&empty);
            if *position < neighbours.len() {
                let next = neighbours[*position];
                *position += 1;
                if let std::collections::hash_map::Entry::Vacant(entry) = index_of.entry(next) {
                    entry.insert(next_index);
                    lowlink.insert(next, next_index);
                    next_index += 1;
                    stack.push(next);
                    on_stack.insert(next);
                    work.push((next, 0));
                } else if on_stack.contains(&next) {
                    let candidate = index_of[&next];
                    let current = lowlink[&node];
                    lowlink.insert(node, current.min(candidate));
                }
                continue;
            }
            work.pop();
            if let Some(&(parent, _)) = work.last() {
                let child_low = lowlink[&node];
                let parent_low = lowlink[&parent];
                lowlink.insert(parent, parent_low.min(child_low));
            }
            if lowlink[&node] == index_of[&node] {
                let mut component = Vec::new();
                while let Some(member) = stack.pop() {
                    on_stack.remove(&member);
                    component.push(member);
                    if member == node {
                        break;
                    }
                }
                component.sort();
                components.push(component);
            }
        }
    }
    components
}

/// Outcome of a single evaluation pass.
struct PassOutcome {
    results: HashMap<CellKey, CellResult>,
    spilled: BTreeMap<CellKey, (CellKey, FormulaValue)>,
    collisions: HashSet<CellKey>,
    previous_values: HashMap<CellKey, FormulaValue>,
}

enum RecalcMode {
    Full,
    Incremental(HashSet<CellKey>),
    Clean,
}

/// What `detect_mode` concluded: which cells to recompute, and whether the
/// dependency graph the last evaluation left behind is still accurate.
struct ModeDecision {
    mode: RecalcMode,
    /// True when the workbook structure or some cell's *source* changed
    /// since the previous evaluation. The dependency graph is derived from
    /// formula source alone, so when this is false the existing graph is
    /// still exact and rebuilding it is pure waste. A volatile formula
    /// (`NOW`, `RAND`) makes cells dirty without changing any source, so
    /// this is deliberately not "the recalc was not clean".
    sources_changed: bool,
}

impl ModeDecision {
    fn full() -> Self {
        Self {
            mode: RecalcMode::Full,
            sources_changed: true,
        }
    }
}

fn detect_mode(workbook: &SpreadsheetWorkbook, env: &WorkbookEnv<'_>) -> ModeDecision {
    let Some(previous) = workbook.recalc.snapshot.as_ref() else {
        return ModeDecision::full();
    };
    if previous.structure != structure_fingerprint(workbook) {
        return ModeDecision::full();
    }
    let current = build_snapshot(workbook);
    let mut changed: HashSet<(String, String)> = HashSet::new();
    for (key, value) in &current.cells {
        if previous.cells.get(key) != Some(value) {
            changed.insert(key.clone());
        }
    }
    for key in previous.cells.keys() {
        if !current.cells.contains_key(key) {
            changed.insert(key.clone());
        }
    }
    let to_key = |sheet_id: &str, address: &str| -> Option<CellKey> {
        let sheet = *env.by_id.get(sheet_id)?;
        let (col, row) = parse_address(address)?;
        Some(CellKey { sheet, col, row })
    };
    let mut dirty: HashSet<CellKey> = HashSet::new();
    for (sheet_id, address) in &changed {
        if let Some(key) = to_key(sheet_id, address) {
            dirty.insert(key);
        }
    }
    // Volatile formulas are always dirty.
    for (key, expr) in &env.formulas {
        if let Ok(expr) = expr {
            if expr.is_volatile() {
                dirty.insert(*key);
            }
        }
    }
    // Spill anchors of changed targets and targets of dirty anchors.
    for (sheet_index, sheet) in workbook.sheets.iter().enumerate() {
        for cell in &sheet.cells {
            let Some(anchor) = &cell.spill_source else {
                continue;
            };
            let (Some((col, row)), Some((anchor_col, anchor_row))) =
                (parse_address(&cell.address), parse_address(anchor))
            else {
                continue;
            };
            let target = CellKey {
                sheet: sheet_index,
                col,
                row,
            };
            let anchor_key = CellKey {
                sheet: sheet_index,
                col: anchor_col,
                row: anchor_row,
            };
            if dirty.contains(&target) {
                dirty.insert(anchor_key);
            }
        }
    }
    let sources_changed = !changed.is_empty();
    if dirty.is_empty() {
        return ModeDecision {
            mode: RecalcMode::Clean,
            sources_changed,
        };
    }
    // Transitive dependents from the previous dependency graph.
    let mut dependents: HashMap<CellKey, Vec<CellKey>> = HashMap::new();
    for entry in &workbook.dependency_graph {
        let Some(source) = to_key(&entry.sheet_id, &entry.address) else {
            continue;
        };
        for dependent in &entry.dependents {
            let (sheet_id, address) =
                graph_dependency_target(&workbook.sheets, &entry.sheet_id, dependent);
            if let Some(target) = to_key(&sheet_id, &address) {
                dependents.entry(source).or_default().push(target);
            }
        }
    }
    let mut frontier: Vec<CellKey> = dirty.iter().copied().collect();
    while let Some(key) = frontier.pop() {
        if let Some(children) = dependents.get(&key) {
            for child in children {
                if dirty.insert(*child) {
                    frontier.push(*child);
                }
            }
        }
    }
    ModeDecision {
        mode: RecalcMode::Incremental(dirty),
        sources_changed,
    }
}

fn run_pass(
    workbook: &SpreadsheetWorkbook,
    env: &mut WorkbookEnv<'_>,
    pending_keys: &HashSet<CellKey>,
    spill_seed: &HashMap<CellKey, FormulaValue>,
) -> PassOutcome {
    // Seed values.
    {
        let mut values = env.values.borrow_mut();
        values.clear();
        for (sheet_index, sheet) in workbook.sheets.iter().enumerate() {
            for cell in &sheet.cells {
                let Some((col, row)) = parse_address(&cell.address) else {
                    continue;
                };
                let key = CellKey {
                    sheet: sheet_index,
                    col,
                    row,
                };
                if cell.user_kind == "formula" {
                    if !pending_keys.contains(&key) {
                        values.insert(
                            key,
                            FormulaValue::from_projection(
                                &cell.computed_kind,
                                &cell.computed_value,
                            ),
                        );
                    }
                    continue;
                }
                if cell.spill_source.is_some() && cell.user_kind == "empty" {
                    if let Some(seeded) = spill_seed.get(&key) {
                        values.insert(key, seeded.clone());
                    } else if let Some((anchor_col, anchor_row)) =
                        cell.spill_source.as_deref().and_then(parse_address)
                    {
                        let anchor = CellKey {
                            sheet: sheet_index,
                            col: anchor_col,
                            row: anchor_row,
                        };
                        if !pending_keys.contains(&anchor) {
                            values.insert(
                                key,
                                FormulaValue::from_projection(
                                    &cell.computed_kind,
                                    &cell.computed_value,
                                ),
                            );
                        }
                    }
                    continue;
                }
                let value = WorkbookEnv::source_value(cell);
                if !value.is_blank() {
                    values.insert(key, value);
                }
            }
        }
        for (key, value) in spill_seed {
            values.entry(*key).or_insert_with(|| value.clone());
        }
    }
    let previous_values = env.values.borrow().clone();
    *env.pending.borrow_mut() = pending_keys.clone();
    env.results.borrow_mut().clear();
    env.in_progress.borrow_mut().clear();
    env.depth.set(0);

    // Static dependency edges among pending cells.
    let mut nodes: Vec<CellKey> = pending_keys.iter().copied().collect();
    nodes.sort();
    let mut edges: HashMap<CellKey, Vec<CellKey>> = HashMap::with_capacity(nodes.len());
    for &node in &nodes {
        let Some(Ok(expr)) = env.formulas.get(&node) else {
            continue;
        };
        let mut targets = Vec::new();
        for range in static_ranges(expr, env, node.sheet) {
            let cell_count = range.width() as u64 * range.height() as u64;
            if cell_count > 250_000 {
                continue;
            }
            for (col, row) in range.cells() {
                let key = CellKey {
                    sheet: range.sheet,
                    col,
                    row,
                };
                if pending_keys.contains(&key) {
                    targets.push(key);
                }
            }
        }
        targets.sort();
        targets.dedup();
        edges.insert(node, targets);
    }
    let components = tarjan_order(&nodes, &edges);
    for component in components {
        let is_cycle = component.len() > 1
            || edges
                .get(&component[0])
                .is_some_and(|targets| targets.contains(&component[0]));
        if is_cycle {
            for key in component {
                env.mark_cycle(key);
            }
            continue;
        }
        let key = component[0];
        if env.pending.borrow().contains(&key) {
            env.evaluate_now(key);
        }
    }

    let results = std::mem::take(&mut *env.results.borrow_mut());
    // Spill assignment.
    let mut spilled: BTreeMap<CellKey, (CellKey, FormulaValue)> = BTreeMap::new();
    let mut collisions = HashSet::new();
    let mut anchors: Vec<&CellKey> = results
        .iter()
        .filter(|(_, result)| result.array.is_some())
        .map(|(key, _)| key)
        .collect();
    anchors.sort();
    for anchor in anchors {
        let array = results[anchor].array.as_ref().expect("array result");
        let mut targets = Vec::new();
        let mut collided = false;
        for row_offset in 0..array.rows {
            for col_offset in 0..array.cols {
                if row_offset == 0 && col_offset == 0 {
                    continue;
                }
                let key = CellKey {
                    sheet: anchor.sheet,
                    col: anchor.col + col_offset as u32,
                    row: anchor.row + row_offset as u32,
                };
                let occupied = env.cell(key).is_some_and(|cell| {
                    cell.user_kind != "empty"
                        || cell
                            .spill_source
                            .as_deref()
                            .is_some_and(|source| source != anchor.address())
                }) || spilled.contains_key(&key);
                if occupied {
                    collided = true;
                    break;
                }
                targets.push((key, array.get(row_offset, col_offset).clone()));
            }
            if collided {
                break;
            }
        }
        if collided {
            collisions.insert(*anchor);
            continue;
        }
        for (key, value) in targets {
            spilled.insert(key, (*anchor, value));
        }
    }
    PassOutcome {
        results,
        spilled,
        collisions,
        previous_values,
    }
}

/// What one evaluation concluded. The warnings it produced are left on
/// `workbook.evaluation_warnings`, which is the workbook's own record of
/// them; this says only what evaluation could not settle on its own.
pub struct EvaluationOutcome {
    /// True when the dependency graph on the workbook no longer describes
    /// the formula source and must be rebuilt. False means nothing that the
    /// graph is derived from has changed, so the existing graph is exact.
    pub dependency_graph_stale: bool,
}

/// Evaluates the workbook in place.
pub fn evaluate_workbook(workbook: &mut SpreadsheetWorkbook) -> EvaluationOutcome {
    let (now_ms, seed) = match &workbook.evaluation_context {
        Some(context) => (context.now_ms, context.seed),
        None => {
            let now = crate::now_ms();
            (now, now.wrapping_mul(0x9E37_79B9_7F4A_7C15))
        }
    };
    let locale = workbook.locale.clone();
    let sheets_snapshot = workbook.sheets.clone();
    let named_ranges = workbook.named_ranges.clone();
    let mut env = WorkbookEnv::new(&sheets_snapshot, &named_ranges, now_ms, seed, &locale);
    let mut all_formulas: HashSet<CellKey> = HashSet::new();
    for (sheet_index, sheet) in sheets_snapshot.iter().enumerate() {
        for cell in &sheet.cells {
            if cell.user_kind != "formula" {
                continue;
            }
            let Some((col, row)) = parse_address(&cell.address) else {
                continue;
            };
            let key = CellKey {
                sheet: sheet_index,
                col,
                row,
            };
            env.formulas
                .insert(key, parse_formula_source(&cell.user_value).map(Rc::new));
            all_formulas.insert(key);
        }
    }

    let ModeDecision {
        mode,
        sources_changed,
    } = detect_mode(workbook, &env);
    let mut pending: HashSet<CellKey> = match &mode {
        RecalcMode::Full => all_formulas.clone(),
        RecalcMode::Incremental(dirty) => dirty
            .iter()
            .copied()
            .filter(|key| all_formulas.contains(key))
            .collect(),
        RecalcMode::Clean => HashSet::new(),
    };
    let mut previous_warnings = std::mem::take(&mut workbook.evaluation_warnings);
    if matches!(mode, RecalcMode::Full) {
        previous_warnings.clear();
    }
    if matches!(mode, RecalcMode::Clean) {
        refresh_display_values(workbook);
        workbook.evaluation_warnings = previous_warnings;
        workbook.recalc.snapshot = Some(Arc::new(build_snapshot(workbook)));
        return EvaluationOutcome {
            dependency_graph_stale: sources_changed,
        };
    }

    let mut spill_seed: HashMap<CellKey, FormulaValue> = HashMap::new();
    let mut outcome = run_pass(workbook, &mut env, &pending, &spill_seed);
    for _ in 1..MAX_SPILL_PASSES {
        let mut changed = false;
        for (key, (_, value)) in &outcome.spilled {
            if outcome.previous_values.get(key) != Some(value) {
                changed = true;
                break;
            }
        }
        if !changed {
            // Targets that stopped being spilled also change values.
            for sheet in &sheets_snapshot {
                let sheet_index = env.by_id[&sheet.id];
                for cell in &sheet.cells {
                    if cell.spill_source.is_none() || cell.user_kind != "empty" {
                        continue;
                    }
                    let Some((col, row)) = parse_address(&cell.address) else {
                        continue;
                    };
                    let key = CellKey {
                        sheet: sheet_index,
                        col,
                        row,
                    };
                    if !outcome.spilled.contains_key(&key)
                        && outcome
                            .previous_values
                            .get(&key)
                            .is_some_and(|v| !v.is_blank())
                    {
                        changed = true;
                    }
                }
            }
        }
        if !changed {
            break;
        }
        pending = all_formulas.clone();
        spill_seed = outcome
            .spilled
            .iter()
            .map(|(key, (_, value))| (*key, value.clone()))
            .collect();
        outcome = run_pass(workbook, &mut env, &pending, &spill_seed);
    }
    drop(env);

    // Write back.
    let mut warnings = Vec::new();
    let sheet_ids: Vec<String> = workbook
        .sheets
        .iter()
        .map(|sheet| sheet.id.clone())
        .collect();
    let mut evaluated_addresses: HashSet<(usize, String)> = HashSet::new();
    for (key, result) in &outcome.results {
        let sheet = &mut workbook.sheets[key.sheet];
        let address = key.address();
        evaluated_addresses.insert((key.sheet, address.clone()));
        let Some(cell) = sheet.cells.iter_mut().find(|cell| cell.address == address) else {
            continue;
        };
        let value = if outcome.collisions.contains(key) {
            FormulaValue::Error(FormulaError::Ref)
        } else {
            result.value.clone()
        };
        cell.computed_kind = value.kind_label().to_string();
        cell.computed_value = value.canonical_text();
        cell.dependencies = if value.is_error() {
            Vec::new()
        } else {
            result.dependencies.iter().cloned().collect()
        };
        if result.cycle {
            warnings.push(SpreadsheetWarning {
                code: "spreadsheet-circular-reference".to_string(),
                message: format!(
                    "circular dependency detected at {}!{address}",
                    sheet_ids[key.sheet]
                ),
            });
        }
    }
    // Keep warnings for cells that were not re-evaluated.
    for warning in previous_warnings {
        let cell_ref = warning
            .message
            .rsplit(' ')
            .next()
            .unwrap_or_default()
            .to_string();
        let still_valid = cell_ref.split_once('!').is_some_and(|(sheet_id, address)| {
            sheet_ids
                .iter()
                .position(|id| id == sheet_id)
                .is_some_and(|sheet_index| {
                    !evaluated_addresses.contains(&(sheet_index, address.to_string()))
                        && workbook.sheets[sheet_index]
                            .cells
                            .iter()
                            .any(|cell| cell.address == address && cell.computed_value == "#REF!")
                })
        });
        if still_valid && !warnings.contains(&warning) {
            warnings.push(warning);
        }
    }
    warnings.sort_by(|left, right| left.message.cmp(&right.message));

    // Spill targets: clear stale ones for re-evaluated anchors, then apply.
    let spilled_by_sheet: BTreeMap<usize, Vec<(CellKey, CellKey, FormulaValue)>> = outcome
        .spilled
        .iter()
        .fold(BTreeMap::new(), |mut acc, (key, (anchor, value))| {
            acc.entry(key.sheet)
                .or_default()
                .push((*key, *anchor, value.clone()));
            acc
        });
    for (sheet_index, sheet) in workbook.sheets.iter_mut().enumerate() {
        let evaluated_anchors: HashSet<String> = evaluated_addresses
            .iter()
            .filter(|(index, _)| *index == sheet_index)
            .map(|(_, address)| address.clone())
            .collect();
        let new_targets: HashMap<String, (String, FormulaValue)> = spilled_by_sheet
            .get(&sheet_index)
            .map(|targets| {
                targets
                    .iter()
                    .map(|(key, anchor, value)| (key.address(), (anchor.address(), value.clone())))
                    .collect()
            })
            .unwrap_or_default();
        for cell in &mut sheet.cells {
            let Some(anchor) = cell.spill_source.clone() else {
                continue;
            };
            if cell.user_kind != "empty" {
                cell.spill_source = None;
                continue;
            }
            if evaluated_anchors.contains(&anchor) && !new_targets.contains_key(&cell.address) {
                cell.spill_source = None;
                cell.computed_kind = "empty".to_string();
                cell.computed_value = String::new();
            }
        }
        let mut addresses: Vec<&String> = new_targets.keys().collect();
        addresses.sort();
        for address in addresses {
            let (anchor, value) = &new_targets[address];
            sheet.ensure_address(address);
            let cell = sheet.cell_mut_or_insert(address);
            cell.spill_source = Some(anchor.clone());
            cell.computed_kind = value.kind_label().to_string();
            cell.computed_value = value.canonical_text();
            cell.dependencies = Vec::new();
        }
    }
    refresh_display_values(workbook);
    workbook.evaluation_warnings = warnings;
    workbook.recalc.snapshot = Some(Arc::new(build_snapshot(workbook)));
    EvaluationOutcome {
        dependency_graph_stale: sources_changed,
    }
}

/// Recomputes projections for non-formula cells and display text for all
/// cells.
pub fn refresh_display_values(workbook: &mut SpreadsheetWorkbook) {
    let locale = Locale::for_tag(&workbook.locale);
    for sheet in &mut workbook.sheets {
        for cell in &mut sheet.cells {
            if cell.user_kind != "formula" && cell.spill_source.is_none() {
                cell.computed_kind = cell.user_kind.clone();
                cell.computed_value = cell.user_value.clone();
                cell.dependencies = Vec::new();
            }
            cell.display_value = format::display_value(
                &cell.computed_kind,
                &cell.computed_value,
                cell.format.number_format.as_deref(),
                &locale,
            );
        }
    }
}

/// Dependency labels of a formula (same shape as evaluation-time
/// dependencies), used by the dependency graph.
pub fn build_dependency_graph(
    sheets: &[Sheet],
    named_ranges: &[NamedRange],
) -> Vec<CellDependency> {
    let env = WorkbookEnv::new(sheets, named_ranges, 0, 0, "en-US");
    let mut dependencies: BTreeMap<(String, String), BTreeSet<String>> = BTreeMap::new();
    let mut dependents: BTreeMap<(String, String), BTreeSet<String>> = BTreeMap::new();
    for (sheet_index, sheet) in sheets.iter().enumerate() {
        for cell in &sheet.cells {
            if cell.user_kind != "formula" {
                continue;
            }
            let labels = match parse_formula_source(&cell.user_value) {
                Ok(expr) => static_dependencies(&expr, &env, sheet_index),
                Err(_) => BTreeSet::new(),
            };
            let key = (sheet.id.clone(), cell.address.clone());
            for dependency in &labels {
                let (dependency_sheet_id, dependency_address) =
                    graph_dependency_target(sheets, &sheet.id, dependency);
                let dependent_label =
                    graph_dependent_label(sheets, &dependency_sheet_id, &sheet.id, &cell.address);
                dependents
                    .entry((dependency_sheet_id, dependency_address))
                    .or_default()
                    .insert(dependent_label);
            }
            dependencies.insert(key, labels);
        }
    }
    let mut addresses = BTreeSet::new();
    addresses.extend(dependencies.keys().cloned());
    addresses.extend(dependents.keys().cloned());
    addresses
        .into_iter()
        .map(|(sheet_id, address)| {
            let direct_dependencies = dependencies
                .get(&(sheet_id.clone(), address.clone()))
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .collect::<Vec<_>>();
            let direct_dependents = dependents
                .get(&(sheet_id.clone(), address.clone()))
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .collect::<Vec<_>>();
            CellDependency {
                sheet_id,
                address,
                dependencies: direct_dependencies,
                dependents: direct_dependents,
            }
        })
        .collect()
}
