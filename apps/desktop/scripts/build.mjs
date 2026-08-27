import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const root = new URL("..", import.meta.url).pathname;
const dist = join(root, "dist");
const assets = join(dist, "assets");

mkdirSync(assets, { recursive: true });

const main = transpileMain(readFileSync(join(root, "src/main.ts"), "utf8"));
const invoke = transpileInvoke(readFileSync(join(root, "src/invoke.ts"), "utf8"));
const styles = readFileSync(join(root, "src/styles.css"), "utf8");

writeFileSync(
  join(dist, "index.html"),
  `<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>OpenDoc</title>
    <link rel="stylesheet" href="./assets/styles.css" />
  </head>
  <body>
    <div id="app"></div>
    <script type="module" src="./assets/main.js"></script>
  </body>
</html>
`,
);
writeFileSync(join(assets, "main.js"), main);
writeFileSync(join(assets, "invoke.js"), invoke);
writeFileSync(join(assets, "styles.css"), styles);

console.log("desktop frontend build written to dist/");

function transpileMain(source) {
  return stripTypeScript(
    source
      .replace(`import { invoke } from "./invoke";`, `import { invoke } from "./invoke.js";`)
      .replace(/import type \{[\s\S]*?\} from "\.\/[a-z-]+";\n/g, "")
      .replace(`import "./styles.css";\n`, ""),
  );
}

function transpileInvoke(source) {
  return stripTypeScript(
    source
      .replace(/import type \{[\s\S]*?\} from "\.\/[a-z-]+";\n\n/g, "")
      .replace(/declare global \{[\s\S]*?\n\}\n\nexport async function/, "export async function"),
  );
}

function stripTypeScript(source) {
  return source
    .replace(/type\s+\w+\s*=\s*\{[\s\S]*?\};\n\n/g, "")
    .replace(/^type\s+\w+\s*=\s*[^;\n]+;\n?/gm, "")
    .replace(/<HTML[A-Za-z]+Element>/g, "")
    .replace(/<HTMLElement>/g, "")
    .replace(/<HTMLButtonElement>/g, "")
    .replace(/\binvoke<[^>]+>\(/g, "invoke(")
    .replace(/<K extends [^>]+>/g, "")
    .replace(/<T>/g, "")
    .replace(/\b([A-Za-z_$][A-Za-z0-9_$]*)<CommandResult<K>>\(/g, "$1(")
    .replace(/ as CommandArgs<[^>]+>/g, "")
    .replace(/ as Promise<CommandResult<K>>/g, "")
    .replace(/ as Promise<[^>]+>/g, "")
    .replace(/ as T/g, "")
    .replace(/ as [A-Za-z][A-Za-z0-9_]*/g, "")
    .replace(/: AppDocument \| null/g, "")
    .replace(/: string \| null/g, "")
    .replace(/: Record<string, unknown> = \{\}/g, " = {}")
    .replace(/: \(\) => void/g, "")
    .replace(/: CommandArgs<[^>]+> = \{\}/g, " = {}")
    .replace(/: CommandArgs<[^>]+>/g, "")
    .replace(/: CommandResult<[^>]+>/g, "")
    .replace(/: DesktopCommandName/g, "")
    .replace(/: Promise<CommandResult<K>>/g, "")
    .replace(/: Promise<unknown>/g, "")
    .replace(/: "[^"]+"(?: \| "[^"]+")+/g, "")
    .replace(/: Record<string, unknown>/g, "")
    .replace(/: Record<string, string>/g, "")
    .replace(/: Promise<T>/g, "")
    .replace(/: Promise/g, "")
    .replace(/: InvokeArgs = \{\}/g, " = {}")
    .replace(/: InvokeArgs/g, "")
    .replace(/: T/g, "")
    .replace(/: MockDocument/g, "")
    .replace(/: MockWorkbook/g, "")
    .replace(/: MockSheet/g, "")
    .replace(/: MockCell/g, "")
    .replace(/: MockCitation/g, "")
    .replace(/: Map<string, MockCell>/g, "")
    .replace(/: \[string, string\]/g, "")
    .replace(/: MockBlock\[\]/g, "")
    .replace(/: MockBlock/g, "")
    .replace(/: MockInline/g, "")
    .replace(/: MockInline \| null/g, "")
    .replace(/: AppBlock/g, "")
    .replace(/: AppInline/g, "")
    .replace(/: AppCitationDatabase/g, "")
    .replace(/: AppDocument/g, "")
    .replace(/: AppSpreadsheetWorkbook/g, "")
    .replace(/: AppSheet/g, "")
    .replace(/: AppCell/g, "")
    .replace(/: AppCommentThread\[\]/g, "")
    .replace(/: AppOperationRecord\[\]/g, "")
    .replace(/: AppSuggestion\[\]/g, "")
    .replace(/: AppWarning\[\]/g, "")
    .replace(/: string\[\]/g, "")
    .replace(/ \| null/g, "")
    .replace(/: boolean/g, "")
    .replace(/: number/g, "")
    .replace(/: string/g, "")
    .replace(/: K/g, "")
    .replace(/: unknown/g, "");
}
