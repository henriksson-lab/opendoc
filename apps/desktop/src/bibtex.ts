// A deliberately small, dependency-free BibTeX reader for the import surface.
// It reads balanced brace/quote values rather than splitting on commas, since
// titles and author lists commonly contain both.
export type ImportedReference = {
  title: string;
  authors: string[];
  issued: string | null;
  doi: string | null;
  url: string | null;
};

export function parseBibtex(source: string): ImportedReference[] {
  const references: ImportedReference[] = [];
  let cursor = 0;
  while (cursor < source.length) {
    const at = source.indexOf("@", cursor);
    if (at < 0) break;
    const kind = /^@(\w+)\s*([\{(])/i.exec(source.slice(at));
    if (!kind) {
      cursor = at + 1;
      continue;
    }
    const open = at + kind[0].lastIndexOf(kind[2]);
    const close = readBalanced(source, open, kind[2] === "{" ? "}" : ")");
    if (close < 0) throw new Error("BibTeX entry has an unclosed delimiter.");
    cursor = close + 1;
    if (/^(comment|string|preamble)$/i.test(kind[1])) continue;
    const fields = readFields(source.slice(open + 1, close));
    const title = clean(fields.title);
    if (!title) continue;
    const authors = clean(fields.author)
      .split(/\s+and\s+/i)
      .map(clean)
      .filter(Boolean);
    references.push({
      title,
      authors,
      issued: clean(fields.year ?? fields.date) || null,
      doi: clean(fields.doi).replace(/^https?:\/\/(?:dx\.)?doi\.org\//i, "") || null,
      url: clean(fields.url) || null,
    });
  }
  return references;
}

function readBalanced(source: string, open: number, closing: string): number {
  if (source[open] === '"') {
    for (let i = open + 1; i < source.length; i += 1) {
      if (source[i] === "\\") {
        i += 1;
      } else if (source[i] === '"') {
        return i;
      }
    }
    return -1;
  }
  let depth = 0;
  let quote = false;
  for (let i = open; i < source.length; i += 1) {
    const ch = source[i];
    if (ch === "\\") {
      i += 1;
      continue;
    }
    if (ch === '"') quote = !quote;
    if (quote) continue;
    if (ch === source[open]) depth += 1;
    if (ch === closing && --depth === 0) return i;
  }
  return -1;
}

function readFields(entry: string): Record<string, string> {
  const fields: Record<string, string> = {};
  let cursor = entry.indexOf(",") + 1; // skip the citation key
  if (cursor === 0) return fields;
  while (cursor < entry.length) {
    const match = /\s*([\w-]+)\s*=\s*/y;
    match.lastIndex = cursor;
    const found = match.exec(entry);
    if (!found) break;
    cursor = match.lastIndex;
    const start = entry[cursor];
    let end = cursor;
    if (start === "{" || start === '"') {
      end = readBalanced(entry, cursor, start === "{" ? "}" : '"');
      if (end < 0) throw new Error(`BibTeX field ${found[1]} has an unclosed value.`);
      fields[found[1].toLowerCase()] = entry.slice(cursor + 1, end);
      cursor = end + 1;
    } else {
      while (end < entry.length && entry[end] !== ",") end += 1;
      fields[found[1].toLowerCase()] = entry.slice(cursor, end);
      cursor = end;
    }
    while (cursor < entry.length && /\s|,/.test(entry[cursor])) cursor += 1;
  }
  return fields;
}

function clean(value: string | undefined): string {
  return (value ?? "")
    .replace(/[{}]/g, "")
    .replace(/\\([&%_$#])/g, "$1")
    .replace(/\s+/g, " ")
    .trim();
}
