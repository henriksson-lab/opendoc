import type { CommandArgs, CommandResult, DesktopCommandName } from "./commands";
import type {
  AppBibliographyReference,
  AppBlock,
  AppCell,
  AppCitationDatabase,
  AppCitationGroup,
  AppCitationItem,
  AppComment,
  AppCommentThread,
  AppDocument,
  AppInline,
  AppOperationRecord,
  AppSheet,
  AppSignature,
  AppSpreadsheetWorkbook,
  AppSuggestion,
  AppWarning,
} from "./types";

type InvokeArgs = Record<string, unknown>;

type TauriGlobal = {
  core?: {
    invoke?: <T>(command: string, args?: InvokeArgs) => Promise<T>;
  };
};

declare global {
  interface Window {
    __TAURI__?: TauriGlobal;
    __TAURI_INTERNALS__?: {
      invoke?: <T>(command: string, args?: InvokeArgs) => Promise<T>;
    };
  }
}

export async function invoke<K extends DesktopCommandName>(
  command: K,
  args: CommandArgs<K> = {} as CommandArgs<K>,
): Promise<CommandResult<K>> {
  const tauriInvoke = window.__TAURI__?.core?.invoke ?? window.__TAURI_INTERNALS__?.invoke;
  if (tauriInvoke) {
    return tauriInvoke<CommandResult<K>>(command, args);
  }
  return mockInvoke<CommandResult<K>>(command, args);
}

type MockDocument = AppDocument;
type MockBlock = AppBlock;
type MockInline = AppInline;
type MockCitationDatabase = AppCitationDatabase;
type MockReference = AppBibliographyReference;
type MockCitation = AppCitationGroup;
type MockCitationItem = AppCitationItem;
type MockCommentThread = AppCommentThread;
type MockComment = AppComment;
type MockSuggestion = AppSuggestion;
type MockWarning = AppWarning;
type MockOperationRecord = AppOperationRecord;
type MockSignature = AppSignature;
type MockWorkbook = AppSpreadsheetWorkbook;
type MockSheet = AppSheet;
type MockCell = AppCell;

let nextId = 1;
let mockDocument: MockDocument;
mockDocument = createMockDocument();

async function mockInvoke<T>(command: DesktopCommandName, args: InvokeArgs): Promise<T> {
  switch (command) {
    case "create_document":
      mockDocument = createMockDocument(String(args.title ?? "Untitled OpenDoc"), false);
      return clone(mockDocument) as T;
    case "get_document":
      return clone(mockDocument) as T;
    case "add_heading":
      addBlock("heading", String(args.text ?? "New section"), Number(args.level ?? 2));
      return clone(mockDocument) as T;
    case "add_paragraph":
      addBlock("paragraph", String(args.text ?? "New paragraph"), null);
      return clone(mockDocument) as T;
    case "add_link":
      mockDocument.blocks.push({
        id: id("block"),
        kind: "paragraph",
        level: null,
        ordered: null,
        equation_source: null,
        content: [
          {
            id: id("link"),
            kind: "link",
            text: String(args.text ?? "Reference link"),
            href: String(args.href ?? "https://example.invalid"),
            target_id: null,
            marks: [],
          },
        ],
        rows: [],
      });
      record("insert-block", "link paragraph");
      return clone(mockDocument) as T;
    case "add_mention":
      mockDocument.blocks.push({
        id: id("block"),
        kind: "paragraph",
        level: null,
        ordered: null,
        equation_source: null,
        content: [
          textInline("Mention: "),
          {
            id: id("mention"),
            kind: "mention",
            text: String(args.label ?? "@local-user"),
            href: null,
            target_id: null,
            marks: [],
          },
        ],
        rows: [],
      });
      record("insert-block", "mention paragraph");
      return clone(mockDocument) as T;
    case "add_footnote_ref": {
      const footnoteId = id("footnote");
      mockDocument.blocks.push({
        id: id("block"),
        kind: "paragraph",
        level: null,
        ordered: null,
        equation_source: null,
        content: [
          textInline("Footnote reference: "),
          {
            id: id("footnote-ref"),
            kind: "footnote-ref",
            text: `[${footnoteId}]`,
            href: null,
            target_id: footnoteId,
            marks: [],
          },
        ],
        rows: [],
      });
      record("insert-block", "footnote reference paragraph");
      return clone(mockDocument) as T;
    }
    case "add_equation":
      mockDocument.blocks.push({
        id: id("block"),
        kind: "paragraph",
        level: null,
        ordered: null,
        equation_source: null,
        content: [
          textInline("Equation: "),
          {
            id: id("eq-inline"),
            kind: "equation",
            text: String(args.source ?? "E=mc^2"),
            href: null,
            target_id: id("eq"),
            marks: [],
          },
        ],
        rows: [],
      });
      record("insert-block", "inline equation");
      return clone(mockDocument) as T;
    case "add_equation_block":
      mockDocument.blocks.push({
        id: id("block"),
        kind: "equation-block",
        level: null,
        ordered: null,
        equation_source: String(args.source ?? "\\sum_{i=1}^{n} i = n(n+1)/2"),
        content: [],
        rows: [],
      });
      record("insert-block", "block equation");
      return clone(mockDocument) as T;
    case "add_list_item":
      mockDocument.blocks.push({
        id: id("block"),
        kind: "list-item",
        level: Number(args.level ?? 0),
        ordered: Boolean(args.ordered ?? false),
        equation_source: null,
        content: [textInline(String(args.text ?? "New list item"))],
        rows: [],
      });
      record("insert-block", "list item");
      return clone(mockDocument) as T;
    case "add_page_break":
      mockDocument.blocks.push({
        id: id("block"),
        kind: "page-break",
        level: null,
        ordered: null,
        equation_source: null,
        content: [],
        rows: [],
      });
      record("insert-block", "page break");
      return clone(mockDocument) as T;
    case "add_table":
      mockDocument.blocks.push({
        id: id("block"),
        kind: "table",
        level: null,
        ordered: null,
        equation_source: null,
        content: [],
        rows: [
          [[paragraphBlock("A1")], [paragraphBlock("B1")]],
          [[paragraphBlock("A2")], [paragraphBlock("B2")]],
        ],
      });
      record("insert-block", "table");
      return clone(mockDocument) as T;
    case "add_citation":
      addCitation();
      return clone(mockDocument) as T;
    case "add_comment":
      mockDocument.comments.push({
        id: id("comment-thread"),
        anchor: mockDocument.blocks[0] ? `nearest:${mockDocument.blocks[0].id}` : "document",
        comments: [
          {
            id: id("comment"),
            author: String(args.author ?? "Local User"),
            body: String(args.body ?? "New comment thread"),
            deleted: false,
          },
        ],
        deleted: false,
      });
      record("add-comment-thread", "comment thread");
      return clone(mockDocument) as T;
    case "add_suggestion":
      mockDocument.suggestions.push({
        id: id("suggestion"),
        author: String(args.author ?? "Local User"),
        kind: "insert",
        state: "proposed",
      });
      record("add-suggestion", "insert suggestion");
      return clone(mockDocument) as T;
    case "update_inline_text": {
      const inline = findInline(String(args.inlineId ?? ""));
      if (!inline) {
        mockDocument.warnings.push({
          code: "missing-inline",
          message: `inline ${String(args.inlineId ?? "")} was missing`,
        });
        return clone(mockDocument) as T;
      }
      if (inline.kind === "citation") {
        mockDocument.warnings.push({
          code: "non-editable-inline",
          message: `inline ${inline.id} is derived from structured state`,
        });
        return clone(mockDocument) as T;
      }
      inline.text = String(args.text ?? "");
      record("update-inline-text", "inline text edit");
      return clone(mockDocument) as T;
    }
    case "delete_comment_thread": {
      const thread = mockDocument.comments.find((item) => item.id === String(args.threadId ?? ""));
      if (thread) {
        thread.deleted = true;
        record("delete-comment-thread", "delete comment thread");
      } else {
        mockDocument.warnings.push({
          code: "missing-comment-thread",
          message: `comment thread ${String(args.threadId ?? "")} was missing`,
        });
      }
      return clone(mockDocument) as T;
    }
    case "accept_suggestion": {
      const suggestion = mockDocument.suggestions.find(
        (item) => item.id === String(args.suggestionId ?? ""),
      );
      if (suggestion) {
        suggestion.state = "accepted";
        record("accept-suggestion", "accept suggestion");
      } else {
        mockDocument.warnings.push({
          code: "missing-suggestion",
          message: `suggestion ${String(args.suggestionId ?? "")} was missing`,
        });
      }
      return clone(mockDocument) as T;
    }
    case "reject_suggestion": {
      const suggestion = mockDocument.suggestions.find(
        (item) => item.id === String(args.suggestionId ?? ""),
      );
      if (suggestion) {
        suggestion.state = "rejected";
        record("reject-suggestion", "reject suggestion");
      } else {
        mockDocument.warnings.push({
          code: "missing-suggestion",
          message: `suggestion ${String(args.suggestionId ?? "")} was missing`,
        });
      }
      return clone(mockDocument) as T;
    }
    case "add_text_mark": {
      const inline = findInline(String(args.inlineId ?? ""));
      if (!inline || inline.kind === "citation") {
        mockDocument.warnings.push({
          code: "missing-text",
          message: `mark target ${String(args.inlineId ?? "")} was missing`,
        });
        return clone(mockDocument) as T;
      }
      const mark = `${String(args.markKind ?? "bold")}:both`;
      if (!inline.marks.includes(mark)) {
        inline.marks.push(mark);
      }
      record("add-mark", "format inline");
      return clone(mockDocument) as T;
    }
    case "update_block_equation_source": {
      const block = findBlock(String(args.blockId ?? ""));
      if (!block) {
        mockDocument.warnings.push({
          code: "missing-block",
          message: `block equation target ${String(args.blockId ?? "")} was missing`,
        });
        return clone(mockDocument) as T;
      }
      if (block.kind !== "equation-block") {
        mockDocument.warnings.push({
          code: "non-equation-block",
          message: `block ${block.id} is not a block equation`,
        });
        return clone(mockDocument) as T;
      }
      block.equation_source = String(args.source ?? "");
      record("update-block-equation-source", "block equation edit");
      return clone(mockDocument) as T;
    }
    case "set_spreadsheet_cell": {
      setMockCell(String(args.address ?? "A1"), String(args.value ?? ""));
      record("set-spreadsheet-cell", `set ${String(args.address ?? "A1").toUpperCase()}`);
      return clone(mockDocument) as T;
    }
    case "update_bibliography_reference": {
      updateMockBibliographyReference(
        String(args.referenceId ?? ""),
        String(args.title ?? ""),
        args.issued == null ? null : String(args.issued),
      );
      return clone(mockDocument) as T;
    }
    case "save_local_repository":
      mockDocument.repository_root = String(args.path ?? "./opendoc-repo");
      mockDocument.last_manifest = `mock-sha256:${mockDocument.operation_count}`;
      return clone(mockDocument) as T;
    case "open_local_repository":
      mockDocument.repository_root = String(args.path ?? "./opendoc-repo");
      return clone(mockDocument) as T;
    case "sign_with_openssh_private_key":
      if (!String(args.privateKeyPem ?? "").trim()) {
        throw new Error("OpenSSH private key is required");
      }
      mockDocument.signature_state = "signed";
      mockDocument.signature = {
        target: `sha256:mock-${mockDocument.operation_count}`,
        signer: "mock-public-key",
        signer_display: String(args.signerDisplay ?? "Local User"),
        title: mockDocument.title,
        signed_at_ms: Date.now(),
      };
      mockDocument.signatures.push(mockDocument.signature);
      return clone(mockDocument) as T;
    case "verify_current_signature":
      if (!String(args.privateKeyPem ?? "").trim()) {
        throw new Error("OpenSSH private key is required");
      }
      return mockDocument.signature_state as T;
    default:
      throw new Error(`unsupported mock command: ${command}`);
  }
}

function createMockDocument(title = "OpenDoc Prototype", seedSample = true): MockDocument {
  const document: MockDocument = {
    uuid: `doc-browser-demo-${nextId++}`,
    title,
    locale: "en-US",
    visible_text: "",
    blocks: [],
    comments: [],
    suggestions: [],
    citations: {
      style: "apa-7th",
      locale: "en-US",
      references: [],
      citations: [],
    },
    workbook: createMockWorkbook(),
    warnings: [],
    signature_state: "unsigned",
    signature: null,
    signatures: [],
    repository_root: null,
    last_manifest: null,
    operation_count: 0,
    operations: [],
  };
  mockDocument = document;
  if (!seedSample) {
    return document;
  }
  addBlock("paragraph", "OpenDoc editing surface", null);
  addBlock("heading", "Schema coverage", 2);
  addBlock("paragraph", "This prototype renders the current docs-like model through a TypeScript UI.", null);
  addLinkBlock("Project note", "https://example.invalid/opendoc");
  addEquationInlineBlock("E=mc^2");
  addTableBlock();
  addCitation();
  addCommentThread("Alice", "Comment threads are part of signed document state.");
  addSuggestionRecord("Bob");
  return document;
}

function addBlock(kind: string, value: string, level: number | null) {
  mockDocument.blocks.push({
    id: id("block"),
    kind,
    level,
    ordered: null,
    equation_source: null,
    content: [textInline(value)],
    rows: [],
  });
  record("insert-block", kind);
}

function addLinkBlock(text: string, href: string) {
  mockDocument.blocks.push({
    id: id("block"),
    kind: "paragraph",
    level: null,
    ordered: null,
    equation_source: null,
    content: [
      {
        id: id("link"),
        kind: "link",
        text,
        href,
        target_id: null,
        marks: [],
      },
    ],
    rows: [],
  });
  record("insert-block", "link paragraph");
}

function addEquationInlineBlock(source: string) {
  mockDocument.blocks.push({
    id: id("block"),
    kind: "paragraph",
    level: null,
    ordered: null,
    equation_source: null,
    content: [
      textInline("Equation: "),
      {
        id: id("eq-inline"),
        kind: "equation",
        text: source,
        href: null,
        target_id: id("eq"),
        marks: [],
      },
    ],
    rows: [],
  });
  record("insert-block", "inline equation");
}

function addTableBlock() {
  mockDocument.blocks.push({
    id: id("block"),
    kind: "table",
    level: null,
    ordered: null,
    equation_source: null,
    content: [],
    rows: [
      [[paragraphBlock("A1")], [paragraphBlock("B1")]],
      [[paragraphBlock("A2")], [paragraphBlock("B2")]],
    ],
  });
  record("insert-block", "table");
}

function addCommentThread(author: string, body: string) {
  mockDocument.comments.push({
    id: id("comment-thread"),
    anchor: mockDocument.blocks[0] ? `nearest:${mockDocument.blocks[0].id}` : "document",
    comments: [
      {
        id: id("comment"),
        author,
        body,
        deleted: false,
      },
    ],
    deleted: false,
  });
  record("add-comment-thread", "comment thread");
}

function addSuggestionRecord(author: string) {
  mockDocument.suggestions.push({
    id: id("suggestion"),
    author,
    kind: "insert",
    state: "proposed",
  });
  record("add-suggestion", "insert suggestion");
}

function addCitation() {
  if (!mockDocument.citations.references.some((reference) => reference.id === "ref-doe-2020")) {
    mockDocument.citations.references.push({
      id: "ref-doe-2020",
      revision: nextId,
      format: "citum-native",
      source: "id: doe-2020\ntitle: Example Article\nauthor: Doe\nyear: 2020",
      title: "Example Article",
      authors: ["Doe"],
      issued: "2020",
      doi: "10.0000/example",
      url: null,
      deleted: false,
    });
    record("upsert-bibliography-reference", "sample citation reference");
  }
  if (!mockDocument.citations.citations.some((citation) => citation.id === "cite-intro")) {
    mockDocument.citations.citations.push({
      id: "cite-intro",
      revision: nextId,
      items: [
        {
          reference_id: "ref-doe-2020",
          locator: "42",
          label: "page",
          prefix: "see",
          suffix: null,
          suppress_author: false,
        },
      ],
      placement: "inline",
      rendered_cache: "(see Doe 2020, 42)",
      deleted: false,
    });
    record("upsert-citation-group", "sample citation group");
  }
  mockDocument.blocks.push({
    id: id("block"),
    kind: "paragraph",
    level: null,
    ordered: null,
    equation_source: null,
    content: [
      textInline("Citation label: "),
      {
        id: id("citation-label"),
        kind: "citation",
        text: "(see Doe 2020, 42)",
        href: null,
        target_id: "cite-intro",
        marks: [],
      },
    ],
    rows: [],
  });
  record("insert-block", "citation label paragraph");
}

function updateMockBibliographyReference(referenceId: string, title: string, issued: string | null) {
  const reference = mockDocument.citations.references.find((item) => item.id === referenceId);
  if (!reference) {
    mockDocument.warnings.push({
      code: "missing-bibliography-reference",
      message: `bibliography reference ${referenceId} was missing`,
    });
    return;
  }
  reference.revision += 1;
  reference.title = title;
  reference.issued = issued;
  reference.source = `title: ${title}\nauthor: ${reference.authors.join("; ")}\nyear: ${issued ?? ""}`;
  record("upsert-bibliography-reference", "update citation reference");
  for (const citation of mockDocument.citations.citations) {
    if (!citation.items.some((item) => item.reference_id === referenceId)) continue;
    citation.revision += 1;
    citation.rendered_cache = renderMockCitation(citation);
    record("upsert-citation-group", "rerender citation group");
  }
  refreshCitationLabels();
}

function renderMockCitation(citation: MockCitation): string {
  const labels = citation.items.map((item) => {
    const reference = mockDocument.citations.references.find(
      (candidate) => candidate.id === item.reference_id && !candidate.deleted,
    );
    let label = reference?.authors[0] ?? item.reference_id;
    if (reference?.issued) label += ` ${reference.issued}`;
    if (item.locator) label += `, ${item.locator}`;
    if (item.prefix) label = `${item.prefix} ${label}`;
    if (item.suffix) label += ` ${item.suffix}`;
    return label;
  });
  return `(${labels.join("; ")})`;
}

function refreshCitationLabels() {
  for (const block of mockDocument.blocks) {
    refreshCitationLabelsInBlocks([block]);
  }
  mockDocument.visible_text = visibleText(mockDocument.blocks);
}

function refreshCitationLabelsInBlocks(blocks: MockBlock[]) {
  for (const block of blocks) {
    for (const inline of block.content) {
      if (inline.kind !== "citation" || !inline.target_id) continue;
      const citation = mockDocument.citations.citations.find((item) => item.id === inline.target_id);
      if (citation?.rendered_cache) inline.text = citation.rendered_cache;
    }
    for (const row of block.rows) {
      for (const cell of row) {
        refreshCitationLabelsInBlocks(cell);
      }
    }
  }
}

function paragraphBlock(value: string): MockBlock {
  return {
    id: id("block"),
    kind: "paragraph",
    level: null,
    ordered: null,
    equation_source: null,
    content: [textInline(value)],
    rows: [],
  };
}

function textInline(value: string): MockInline {
  return {
    id: id("text"),
    kind: "text",
    text: value,
    href: null,
    target_id: null,
    marks: [],
  };
}

function createMockWorkbook(): MockWorkbook {
  const workbook: MockWorkbook = {
    title: "Prototype Sheet",
    locale: "en-US",
    timezone: "UTC",
    sheets: [
      {
        id: "sheet-1",
        title: "Sheet1",
        rows: ["1", "2", "3"],
        columns: ["A", "B"],
        cells: [
          mockCell("A1", "string", "Item"),
          mockCell("B1", "string", "Count"),
          mockCell("A2", "string", "Apples"),
          mockCell("B2", "number", "5"),
          mockCell("A3", "string", "Total"),
          mockCell("B3", "formula", "=SUM(B2:B2)"),
        ],
      },
    ],
  };
  evaluateMockWorkbook(workbook);
  return workbook;
}

function mockCell(address: string, userKind: string, userValue: string): MockCell {
  return {
    address,
    user_kind: userKind,
    user_value: userValue,
    computed_kind: userKind,
    computed_value: userValue,
    dependencies: [],
  };
}

function setMockCell(address: string, value: string) {
  const sheet = mockDocument.workbook.sheets[0];
  if (!sheet) return;
  address = normalizeCellAddress(address);
  const [column, row] = splitCellAddress(address);
  if (!sheet.columns.includes(column)) sheet.columns.push(column);
  if (!sheet.rows.includes(row)) sheet.rows.push(row);
  sheet.columns.sort();
  sheet.rows.sort((left, right) => Number(left) - Number(right));
  const userKind = classifyMockCellValue(value);
  const existing = sheet.cells.find((cell) => cell.address === address);
  if (existing) {
    existing.user_kind = userKind;
    existing.user_value = value;
  } else {
    sheet.cells.push(mockCell(address, userKind, value));
  }
  sheet.cells.sort((left, right) => left.address.localeCompare(right.address));
  evaluateMockWorkbook(mockDocument.workbook);
}

function evaluateMockWorkbook(workbook: MockWorkbook) {
  for (const sheet of workbook.sheets) {
    const values = new Map(sheet.cells.map((cell) => [cell.address, cell]));
    for (const cell of sheet.cells) {
      if (cell.user_kind === "formula") {
        const result = evaluateMockFormula(cell.user_value, values);
        cell.computed_kind = result.kind;
        cell.computed_value = result.value;
        cell.dependencies = result.dependencies;
      } else {
        cell.computed_kind = cell.user_kind;
        cell.computed_value = cell.user_value;
        cell.dependencies = [];
      }
    }
  }
}

function evaluateMockFormula(formula: string, values: Map<string, MockCell>) {
  const body = formula.trim().startsWith("=") ? formula.trim().slice(1) : formula.trim();
  const range = body.startsWith("SUM(") && body.endsWith(")") ? body.slice(4, -1) : "";
  if (!range) {
    const address = normalizeCellAddress(body);
    const cell = values.get(address);
    const value = Number(cell?.user_value);
    return Number.isFinite(value)
      ? { kind: "number", value: trimMockNumber(value), dependencies: [address] }
      : { kind: "error", value: "#VALUE", dependencies: [] };
  }
  const addresses = expandMockRange(range);
  let sum = 0;
  for (const address of addresses) {
    const value = Number(values.get(address)?.user_value);
    if (!Number.isFinite(value)) return { kind: "error", value: "#VALUE", dependencies: [] };
    sum += value;
  }
  return { kind: "number", value: trimMockNumber(sum), dependencies: addresses };
}

function expandMockRange(range: string): string[] {
  const [start, end] = range.split(":").map(normalizeCellAddress);
  const [column, firstRow] = splitCellAddress(start);
  const [endColumn, lastRow] = splitCellAddress(end);
  if (column !== endColumn) return [];
  const first = Math.min(Number(firstRow), Number(lastRow));
  const last = Math.max(Number(firstRow), Number(lastRow));
  return Array.from({ length: last - first + 1 }, (_, index) => `${column}${first + index}`);
}

function normalizeCellAddress(address: string): string {
  return address.trim().toUpperCase();
}

function splitCellAddress(address: string): [string, string] {
  const column = address.match(/^[A-Z]+/)?.[0] ?? "A";
  const row = address.match(/[0-9]+$/)?.[0] ?? "1";
  return [column, row];
}

function classifyMockCellValue(value: string): string {
  if (value.trim().startsWith("=")) return "formula";
  if (value.trim() === "") return "empty";
  if (Number.isFinite(Number(value))) return "number";
  if (["true", "false"].includes(value.trim().toLowerCase())) return "bool";
  return "string";
}

function trimMockNumber(value: number): string {
  return Number.isInteger(value) ? String(value) : String(value);
}

function findInline(inlineId: string): MockInline | null {
  return findInlineInBlocks(mockDocument.blocks, inlineId);
}

function findBlock(blockId: string): MockBlock | null {
  return findBlockInBlocks(mockDocument.blocks, blockId);
}

function findBlockInBlocks(blocks: MockBlock[], blockId: string): MockBlock | null {
  for (const block of blocks) {
    if (block.id === blockId) return block;
    for (const row of block.rows) {
      for (const cell of row) {
        const nested = findBlockInBlocks(cell, blockId);
        if (nested) return nested;
      }
    }
  }
  return null;
}

function findInlineInBlocks(blocks: MockBlock[], inlineId: string): MockInline | null {
  for (const block of blocks) {
    const inline = block.content.find((item) => item.id === inlineId);
    if (inline) return inline;
    for (const row of block.rows) {
      for (const cell of row) {
        const nested = findInlineInBlocks(cell, inlineId);
        if (nested) return nested;
      }
    }
  }
  return null;
}

function id(prefix: string): string {
  return `${prefix}-${nextId++}`;
}

function record(kind: string, summary: string) {
  const seq = mockDocument.operation_count + 1;
  mockDocument.operation_count = seq;
  mockDocument.operations.push({
    actor: "browser-demo",
    seq,
    kind,
    summary,
    created_at_ms: Date.now(),
  });
  mockDocument.visible_text = visibleText(mockDocument.blocks);
  mockDocument.signature_state = "unsigned";
  mockDocument.signature = null;
  mockDocument.signatures = [];
}

function visibleText(blocks: MockBlock[]): string {
  return blocks.map(blockText).join("");
}

function blockText(block: MockBlock): string {
  if (block.kind === "table") {
    return block.rows
      .map((row) => row.map((cell) => cell.map(blockText).join("")).join("\t"))
      .join("\n");
  }
  if (block.kind === "equation-block") {
    return `${block.equation_source ?? ""}\n`;
  }
  if (block.kind === "page-break") {
    return "\n";
  }
  return `${block.content.map((inline) => inline.text).join("")}\n`;
}

function clone<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T;
}
