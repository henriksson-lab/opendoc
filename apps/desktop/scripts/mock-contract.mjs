const root = new URL("..", import.meta.url).pathname;

globalThis.window = {};

const { invoke } = await import(`${root}/dist/assets/invoke.js`);

const commandArgs = {
  get_document: {},
  create_document: { title: "Mock Contract Document" },
  add_heading: { text: "Contract Heading", level: 2 },
  add_paragraph: { text: "Contract paragraph" },
  add_link: { text: "Contract link", href: "https://example.invalid/contract" },
  add_mention: { label: "@contract-user" },
  add_footnote_ref: {},
  add_equation: { source: "a^2+b^2=c^2" },
  add_equation_block: { source: "x=1" },
  add_list_item: { text: "Contract list item", level: 1, ordered: false },
  add_page_break: {},
  add_table: {},
  add_citation: {},
  add_comment: { author: "Contract Reviewer", body: "Contract comment" },
  add_suggestion: { author: "Contract Editor", text: "Contract suggestion" },
  add_text_mark: null,
  update_inline_text: null,
  update_block_equation_source: null,
  set_spreadsheet_cell: { address: "B2", value: "13" },
  update_bibliography_reference: {
    referenceId: "ref-doe-2020",
    title: "Contract Article",
    issued: "2027",
  },
  save_local_repository: { path: "./mock-contract-repo" },
  open_local_repository: { path: "./mock-contract-repo", documentUuid: "doc-browser-demo" },
  sign_with_openssh_private_key: {
    privateKeyPem: "mock-private-key",
    signerDisplay: "Contract Signer",
  },
};

let documentState = await invoke("get_document");
validateDocument(documentState);

for (const [command, args] of Object.entries(commandArgs)) {
  if (command === "get_document") {
    continue;
  }
  const resolvedArgs = resolveArgs(command, args, documentState);
  documentState = await invoke(command, resolvedArgs);
  validateDocument(documentState);
}

const verifyState = await invoke("verify_current_signature", { privateKeyPem: "mock-private-key" });
if (typeof verifyState !== "string") {
  throw new Error("verify_current_signature did not return a string");
}

console.log("desktop mock contract check passed");

function resolveArgs(command, args, state) {
  if (args) {
    return args;
  }
  if (command === "add_text_mark") {
    return { inlineId: firstInline(state, "text").id, markKind: "bold" };
  }
  if (command === "update_inline_text") {
    return { inlineId: firstInline(state, "text").id, text: "Contract edited inline" };
  }
  if (command === "update_block_equation_source") {
    return { blockId: firstBlock(state, "equation-block").id, source: "y=2" };
  }
  throw new Error(`missing args for ${command}`);
}

function validateDocument(value) {
  assertObject(value, "AppDocument");
  for (const field of [
    "uuid",
    "title",
    "locale",
    "visible_text",
    "blocks",
    "comments",
    "suggestions",
    "citations",
    "workbook",
    "warnings",
    "signature_state",
    "signatures",
    "operation_count",
    "operations",
  ]) {
    assertHas(value, field, "AppDocument");
  }
  assertArray(value.blocks, "AppDocument.blocks");
  value.blocks.forEach((block, index) => validateBlock(block, `blocks[${index}]`));
  value.comments.forEach(validateCommentThread);
  value.suggestions.forEach(validateSuggestion);
  validateCitations(value.citations);
  validateWorkbook(value.workbook);
  value.operations.forEach(validateOperation);
  value.signatures.forEach(validateSignature);
}

function validateBlock(block, path) {
  assertObject(block, path);
  for (const field of ["id", "kind", "level", "ordered", "equation_source", "content", "rows"]) {
    assertHas(block, field, path);
  }
  block.content.forEach((inline, index) => validateInline(inline, `${path}.content[${index}]`));
  block.rows.forEach((row, rowIndex) => {
    assertArray(row, `${path}.rows[${rowIndex}]`);
    row.forEach((cell, cellIndex) => {
      assertArray(cell, `${path}.rows[${rowIndex}][${cellIndex}]`);
      cell.forEach((nested, nestedIndex) =>
        validateBlock(nested, `${path}.rows[${rowIndex}][${cellIndex}][${nestedIndex}]`),
      );
    });
  });
}

function validateInline(inline, path) {
  assertObject(inline, path);
  for (const field of ["id", "kind", "text", "href", "target_id", "marks"]) {
    assertHas(inline, field, path);
  }
}

function validateCommentThread(thread) {
  for (const field of ["id", "anchor", "comments", "deleted"]) {
    assertHas(thread, field, "comment");
  }
}

function validateSuggestion(suggestion) {
  for (const field of ["id", "author", "kind", "state"]) {
    assertHas(suggestion, field, "suggestion");
  }
}

function validateCitations(citations) {
  for (const field of ["style", "locale", "references", "citations"]) {
    assertHas(citations, field, "citations");
  }
}

function validateWorkbook(workbook) {
  for (const field of ["title", "locale", "timezone", "sheets"]) {
    assertHas(workbook, field, "workbook");
  }
}

function validateOperation(operation) {
  for (const field of ["actor", "seq", "kind", "summary", "created_at_ms"]) {
    assertHas(operation, field, "operation");
  }
}

function validateSignature(signature) {
  for (const field of ["target", "signer", "signer_display", "title", "signed_at_ms"]) {
    assertHas(signature, field, "signature");
  }
}

function firstInline(state, kind) {
  for (const block of state.blocks) {
    const found = firstInlineInBlock(block, kind);
    if (found) return found;
  }
  throw new Error(`missing inline kind ${kind}`);
}

function firstInlineInBlock(block, kind) {
  const direct = block.content.find((inline) => inline.kind === kind);
  if (direct) return direct;
  for (const row of block.rows) {
    for (const cell of row) {
      for (const nested of cell) {
        const found = firstInlineInBlock(nested, kind);
        if (found) return found;
      }
    }
  }
  return null;
}

function firstBlock(state, kind) {
  const block = state.blocks.find((item) => item.kind === kind);
  if (!block) {
    throw new Error(`missing block kind ${kind}`);
  }
  return block;
}

function assertObject(value, path) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`${path} is not an object`);
  }
}

function assertArray(value, path) {
  if (!Array.isArray(value)) {
    throw new Error(`${path} is not an array`);
  }
}

function assertHas(value, field, path) {
  if (!(field in value)) {
    throw new Error(`${path} missing field ${field}`);
  }
}
