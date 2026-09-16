import { spawnSync } from "node:child_process";

const requirements = [
  {
    pkg: "gdk-3.0",
    pc: "gdk-3.0.pc",
    debian: "libgtk-3-dev",
    fedora: "gtk3-devel",
  },
  {
    // What `wry` actually links against. The two below ship in the same
    // package but are separate .pc files, and a partial install shows up
    // here rather than three minutes into `cargo build`.
    pkg: "webkit2gtk-4.1",
    pc: "webkit2gtk-4.1.pc",
    debian: "libwebkit2gtk-4.1-dev",
    fedora: "webkit2gtk4.1-devel",
  },
  {
    pkg: "javascriptcoregtk-4.1",
    pc: "javascriptcoregtk-4.1.pc",
    debian: "libwebkit2gtk-4.1-dev",
    fedora: "webkit2gtk4.1-devel",
  },
  {
    pkg: "libsoup-3.0",
    pc: "libsoup-3.0.pc",
    debian: "libwebkit2gtk-4.1-dev",
    fedora: "webkit2gtk4.1-devel",
  },
];

function checkPkgConfig() {
  const result = spawnSync("pkg-config", ["--version"], { encoding: "utf8" });
  if (result.status === 0) {
    return true;
  }

  console.error("pkg-config is required to build the Tauri desktop backend.");
  console.error("Install pkg-config/pkgconf, then rerun `npm run preflight`.");
  return false;
}

function checkRequirement(requirement) {
  const result = spawnSync("pkg-config", ["--exists", requirement.pkg], {
    encoding: "utf8",
  });

  return result.status === 0;
}

if (!checkPkgConfig()) {
  process.exit(1);
}

const missing = requirements.filter((requirement) => !checkRequirement(requirement));

if (missing.length === 0) {
  console.log("desktop native preflight passed");
  process.exit(0);
}

console.error("desktop native preflight failed");
console.error("");
console.error("Missing pkg-config packages:");
for (const requirement of missing) {
  console.error(`- ${requirement.pkg} (${requirement.pc})`);
}
console.error("");
console.error("Common Linux package names:");
console.error(
  `- Ubuntu/Debian: ${[...new Set(missing.map((requirement) => requirement.debian)), "pkg-config"].join(" ")}`,
);
console.error(
  `- Fedora: ${[...new Set(missing.map((requirement) => requirement.fedora)), "pkgconf-pkg-config"].join(" ")}`,
);

process.exit(1);
