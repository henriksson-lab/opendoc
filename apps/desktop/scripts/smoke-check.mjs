import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { spawnSync } from "node:child_process";

const root = new URL("..", import.meta.url).pathname;
const main = readFileSync(join(root, "src/main.ts"), "utf8");
const invoke = readFileSync(join(root, "src/invoke.ts"), "utf8");
const types = readFileSync(join(root, "src/types.ts"), "utf8");
const commandTypes = readFileSync(join(root, "src/commands.ts"), "utf8");
const tauriMain = readFileSync(join(root, "src-tauri/src/main.rs"), "utf8");
const tauriBuild = readFileSync(join(root, "src-tauri/build.rs"), "utf8");
const tauriCapability = JSON.parse(
  readFileSync(join(root, "src-tauri/capabilities/default.json"), "utf8"),
);
const appApi = readFileSync(join(root, "../../crates/opendoc-app-api/src/lib.rs"), "utf8");
const contract = JSON.parse(readFileSync(join(root, "commands.v0.json"), "utf8"));
const appContract = readFileSync(join(root, "../../docs/APP_API_CONTRACT_V0.md"), "utf8");
const distFiles = ["dist/index.html", "dist/assets/main.js", "dist/assets/invoke.js", "dist/assets/styles.css"];
const contractCommands = contract.commands.map((command) => command.name);
const rustCommandArgs = parseTauriCommandArgs(tauriMain);
const frontendCommandArgs = parseFrontendCommandArgs(main);
const appApiDispatchCommands = parseAppApiDispatchCommands(appApi);
const tsCommandArgs = parseTypescriptCommandArgs(commandTypes);

const externalImports = [...main.matchAll(/from\s+["']([^."'][^"']*)["']/g)].map((match) => match[1]);
if (externalImports.length > 0) {
  throw new Error(`unexpected external imports in main.ts: ${externalImports.join(", ")}`);
}

assertSameSet(
  "AppDocument contract fields",
  parseDocumentedAppDocumentFields(appContract),
  parseTypeFields(types, "AppDocument"),
);

const tauriCommands = [
  ...tauriMain.matchAll(/#\[tauri::command\]\s*fn\s+([a-zA-Z0-9_]+)/g),
].map((match) => match[1]);
const handlerCommands = parseGenerateHandlerCommands(tauriMain);
const buildManifestCommands = parseTauriBuildManifestCommands(tauriBuild);
const capabilityCommands = parseCapabilityCommands(tauriCapability);
const mockCommands = [...invoke.matchAll(/case\s+"([^"]+)"/g)].map((match) => match[1]);
const frontendCommands = [
  ...main.matchAll(/\b(?:command|invoke)(?:<[^>]+>)?\(\s*"([^"]+)"/g),
  ...main.matchAll(/\bbindPerItemCommand\(\s*"[^"]+"\s*,\s*"[^"]+"\s*,\s*"([^"]+)"/g),
].map((match) => match[1]);

assertSameSet("Command contract and Tauri functions", contractCommands, tauriCommands);
assertSameSet("Tauri command handler", tauriCommands, handlerCommands);
assertSameSet("Command contract and Tauri build manifest", contractCommands, buildManifestCommands);
assertSameSet("Command contract and Tauri capability permissions", contractCommands, capabilityCommands);
assertSameSet("Command contract and app-api dispatcher", contractCommands, appApiDispatchCommands);
assertSameSet("Command contract and mock backend", contractCommands, mockCommands);
assertSameSet("Command contract and frontend calls", contractCommands, frontendCommands);
for (const command of contract.commands) {
  assertSameSet(
    `TypeScript command args for ${command.name}`,
    Object.keys(command.args ?? {}),
    tsCommandArgs.get(command.name) ?? [],
  );
  assertSameSet(
    `Command args for ${command.name}`,
    Object.keys(command.args ?? {}),
    rustCommandArgs.get(command.name) ?? [],
  );
  assertSameSet(
    `Frontend args for ${command.name}`,
    Object.keys(command.args ?? {}),
    frontendCommandArgs.get(command.name) ?? [],
  );
}

for (const file of distFiles) {
  if (!existsSync(join(root, file))) {
    throw new Error(`desktop build output is missing ${file}; run npm run build`);
  }
}

for (const file of ["dist/assets/main.js", "dist/assets/invoke.js"]) {
  const result = spawnSync(process.execPath, ["--check", join(root, file)], {
    encoding: "utf8",
  });
  if (result.status !== 0) {
    throw new Error(`generated JavaScript syntax check failed for ${file}\n${result.stderr}`);
  }
}

console.log("desktop frontend smoke check passed");

function parseGenerateHandlerCommands(source) {
  const match = source.match(/tauri::generate_handler!\s*\[([\s\S]*?)\]/);
  if (!match) {
    throw new Error("missing tauri::generate_handler command list");
  }
  return match[1]
    .split(",")
    .map((item) => item.trim())
    .filter(Boolean);
}

function parseTauriBuildManifestCommands(source) {
  const match = source.match(/const COMMANDS:\s*&\[[\s\S]*?\]\s*=\s*&\[([\s\S]*?)\];/);
  if (!match) {
    throw new Error("missing Tauri build COMMANDS manifest");
  }
  return [...match[1].matchAll(/"([a-z0-9_]+)"/g)].map((item) => item[1]);
}

function parseCapabilityCommands(capability) {
  return capability.permissions
    .filter((permission) => permission.startsWith("allow-"))
    .map((permission) => permission.slice("allow-".length).replace(/-/g, "_"));
}

function parseDocumentedAppDocumentFields(source) {
  const section = source.match(/`AppDocument` contains:\n\n([\s\S]*?)\n\n`visible_text`/);
  if (!section) {
    throw new Error("could not parse AppDocument field list from APP_API_CONTRACT_V0.md");
  }
  return [
    ...section[1].matchAll(/`([^`]+)`/g),
  ].flatMap((match) => match[1].split(",").map((field) => field.trim()));
}

function parseTypeFields(source, typeName) {
  const match = source.match(new RegExp(`export type ${typeName} = \\{([\\s\\S]*?)\\n\\};`));
  if (!match) {
    throw new Error(`could not parse ${typeName} from src/types.ts`);
  }
  return [...match[1].matchAll(/^\s*([A-Za-z_$][A-Za-z0-9_$]*)[?:]?\s*:/gm)].map(
    (field) => field[1],
  );
}

function parseTypescriptCommandArgs(source) {
  const match = source.match(/export type DesktopCommandArgs = \{([\s\S]*?)\n\};/);
  if (!match) {
    throw new Error("could not parse DesktopCommandArgs from src/commands.ts");
  }
  const argsByCommand = new Map();
  const lines = match[1].split("\n");
  for (let index = 0; index < lines.length; index += 1) {
    const start = lines[index].match(/^\s*([a-z0-9_]+):\s*\{(.*)$/);
    if (!start) {
      continue;
    }
    const name = start[1];
    let body = start[2];
    while (!body.includes("};") && index + 1 < lines.length) {
      index += 1;
      body += `\n${lines[index]}`;
    }
    const end = body.indexOf("};");
    argsByCommand.set(name, parseTypeObjectFields(end >= 0 ? body.slice(0, end) : body));
  }
  return argsByCommand;
}

function parseTypeObjectFields(source) {
  return source
    .split(";")
    .map((property) => property.trim())
    .filter(Boolean)
    .map((property) => property.split(":")[0].replace(/[?;]/g, "").trim())
    .filter(Boolean);
}

function parseTauriCommandArgs(source) {
  const commands = new Map();
  const commandPattern = /#\[tauri::command\]\s*fn\s+([a-zA-Z0-9_]+)\s*\(/g;
  let match;
  while ((match = commandPattern.exec(source))) {
    const name = match[1];
    const argsStart = commandPattern.lastIndex;
    const argsEnd = findMatchingParen(source, argsStart - 1);
    if (argsEnd < 0) {
      throw new Error(`could not parse args for Tauri command ${name}`);
    }
    commands.set(name, parseRustArgs(source.slice(argsStart, argsEnd)));
    commandPattern.lastIndex = argsEnd;
  }
  return commands;
}

function parseAppApiDispatchCommands(source) {
  const dispatchStart = source.indexOf("pub fn dispatch_command");
  if (dispatchStart < 0) {
    throw new Error("missing OpenDocApp::dispatch_command");
  }
  const matchStart = source.indexOf("match command", dispatchStart);
  if (matchStart < 0) {
    throw new Error("missing dispatch_command match command block");
  }
  const braceStart = source.indexOf("{", matchStart);
  const braceEnd = findMatchingBrace(source, braceStart);
  if (braceEnd < 0) {
    throw new Error("could not parse dispatch_command match block");
  }
  return [...source.slice(braceStart + 1, braceEnd).matchAll(/"([a-z0-9_]+)"\s*=>/g)].map(
    (match) => match[1],
  );
}

function parseRustArgs(argsSource) {
  return splitTopLevel(argsSource)
    .map((arg) => arg.trim())
    .filter(Boolean)
    .filter((arg) => !arg.includes("tauri::State"))
    .map((arg) => arg.split(":")[0].trim())
    .map(snakeToCamel);
}

function splitTopLevel(source) {
  const parts = [];
  let start = 0;
  let angleDepth = 0;
  for (let index = 0; index < source.length; index += 1) {
    const char = source[index];
    if (char === "<") angleDepth += 1;
    if (char === ">") angleDepth = Math.max(0, angleDepth - 1);
    if (char === "," && angleDepth === 0) {
      parts.push(source.slice(start, index));
      start = index + 1;
    }
  }
  parts.push(source.slice(start));
  return parts;
}

function findMatchingParen(source, openIndex) {
  let depth = 0;
  for (let index = openIndex; index < source.length; index += 1) {
    if (source[index] === "(") depth += 1;
    if (source[index] === ")") {
      depth -= 1;
      if (depth === 0) return index;
    }
  }
  return -1;
}

function snakeToCamel(value) {
  return value.replace(/_([a-z])/g, (_, letter) => letter.toUpperCase());
}

function parseFrontendCommandArgs(source) {
  const argsByCommand = new Map();
  for (const match of source.matchAll(/\b(?:command|invoke)(?:<[^>]+>)?\(\s*"([^"]+)"/g)) {
    const name = match[1];
    const openParen = source.indexOf("(", match.index);
    const closeParen = findMatchingParen(source, openParen);
    if (closeParen < 0) {
      throw new Error(`could not parse frontend call for ${name}`);
    }
    const callSource = source.slice(openParen + 1, closeParen);
    mergeCommandArgs(argsByCommand, name, parseObjectArgKeys(callSource));
  }

  for (const match of source.matchAll(/\bbindPerItemCommand\(\s*"[^"]+"\s*,\s*"([^"]+)"\s*,\s*"([^"]+)"/g)) {
    const idKey = match[1];
    const name = match[2];
    const openParen = source.indexOf("(", match.index);
    const closeParen = findMatchingParen(source, openParen);
    if (closeParen < 0) {
      throw new Error(`could not parse per-item frontend call for ${name}`);
    }
    mergeCommandArgs(argsByCommand, name, [idKey, ...parseObjectArgKeys(source.slice(openParen + 1, closeParen))]);
  }

  return new Map([...argsByCommand].map(([name, args]) => [name, [...args].sort()]));
}

function parseObjectArgKeys(callSource) {
  const objectStart = callSource.indexOf("{");
  if (objectStart < 0) {
    return [];
  }
  const objectEnd = findMatchingBrace(callSource, objectStart);
  if (objectEnd < 0) {
    throw new Error(`could not parse frontend object args from ${callSource}`);
  }
  const objectSource = callSource.slice(objectStart + 1, objectEnd);
  const keys = [];
  for (const property of splitTopLevel(objectSource)) {
    const trimmed = property.trim();
    if (!trimmed || trimmed.startsWith("...")) {
      continue;
    }
    const key = (trimmed.split(":")[0] ?? "").trim();
    if (/^[A-Za-z_$][A-Za-z0-9_$]*$/.test(key) && !keys.includes(key)) {
      keys.push(key);
    }
  }
  return keys;
}

function findMatchingBrace(source, openIndex) {
  let depth = 0;
  for (let index = openIndex; index < source.length; index += 1) {
    if (source[index] === "{") depth += 1;
    if (source[index] === "}") {
      depth -= 1;
      if (depth === 0) return index;
    }
  }
  return -1;
}

function mergeCommandArgs(target, command, args) {
  const existing = target.get(command) ?? new Set();
  for (const arg of args) {
    existing.add(arg);
  }
  target.set(command, existing);
}

function assertSameSet(label, left, right) {
  const missing = left.filter((item) => !right.includes(item));
  const extra = right.filter((item) => !left.includes(item));
  if (missing.length > 0 || extra.length > 0) {
    throw new Error(
      `${label} mismatch; missing=[${missing.join(", ")}] extra=[${extra.join(", ")}]`,
    );
  }
}
