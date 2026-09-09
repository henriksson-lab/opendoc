// Builds the static frontend bundle: every src/*.ts module is transpiled
// with the TypeScript compiler (types are checked separately by
// `npm run typecheck`), and the WebAssembly core glue from src/wasm/ is
// copied alongside so the browser build runs the real Rust engine.
import { cpSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const root = new URL("..", import.meta.url).pathname;
const src = join(root, "src");
const dist = join(root, "dist");
const assets = join(dist, "assets");

const ts = (await import("typescript")).default;

rmSync(assets, { recursive: true, force: true });
mkdirSync(assets, { recursive: true });

for (const entry of readdirSync(src)) {
  if (!entry.endsWith(".ts") || entry.endsWith(".d.ts")) {
    continue;
  }
  const source = readFileSync(join(src, entry), "utf8");
  const output = ts.transpileModule(source, {
    fileName: entry,
    reportDiagnostics: true,
    compilerOptions: {
      target: ts.ScriptTarget.ES2022,
      module: ts.ModuleKind.ESNext,
      useDefineForClassFields: true,
      isolatedModules: true,
    },
  });
  const errors = (output.diagnostics ?? []).filter((d) => d.category === ts.DiagnosticCategory.Error);
  if (errors.length > 0) {
    const messages = errors.map((d) => ts.flattenDiagnosticMessageText(d.messageText, "\n"));
    throw new Error(`transpile failed for ${entry}:\n${messages.join("\n")}`);
  }
  const js = output.outputText
    // Relative imports keep their extension-less form in TS; browsers need .js.
    .replace(/from "(\.\/[a-zA-Z0-9_/-]+)";/g, (match, path) => (path.endsWith(".js") ? match : `from "${path}.js";`))
    .replace(/import\("(\.\/[a-zA-Z0-9_/-]+)"\)/g, (match, path) => (path.endsWith(".js") ? match : `import("${path}.js")`))
    .replace(/^import "\.\/styles\.css";\n?/m, "");
  writeFileSync(join(assets, entry.replace(/\.ts$/, ".js")), js);
}

writeFileSync(join(assets, "styles.css"), readFileSync(join(src, "styles.css"), "utf8"));

const wasmDir = join(src, "wasm");
if (existsSync(wasmDir)) {
  cpSync(wasmDir, join(assets, "wasm"), { recursive: true });
} else {
  throw new Error("src/wasm/ missing: run `npm run build:wasm` before `npm run build`");
}

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

console.log("desktop frontend build written to dist/");
