/**
 * Cuts a release of Hark: build, sign, publish, point the updater at it.
 *
 *   1. writes the version into tauri.conf.json, Cargo.toml and package.json
 *   2. builds the signed installer (the .sig is what the updater verifies)
 *   3. uploads the installer to tyan:/data/aurora-web/hark/, served as
 *      https://madgodinc.net/hark/, under its versioned name and as
 *      Hark-setup.exe, the stable name the download button points at
 *   4. records the release in docs/history.json and uploads it
 *   5. writes latest.json LAST: until it points at the new version, no
 *      installed copy tries to fetch a file that is still uploading
 *
 * Usage:
 *   node scripts/release.mjs 0.2.0 "Что нового
 *   - пункт
 *   - пункт"
 *   node scripts/release.mjs --same "Пересборка той же версии"
 *
 * Needs .keys/hark.key (never committed; lose it and no installed copy can be
 * updated again) and the ssh alias `tyan`.
 */

import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync, existsSync, readdirSync, copyFileSync, mkdirSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(fileURLToPath(new URL(".", import.meta.url)), "..");
const confPath = join(root, "src-tauri", "tauri.conf.json");
const cargoPath = join(root, "src-tauri", "Cargo.toml");
const pkgPath = join(root, "package.json");
const keyPath = join(root, ".keys", "hark.key");
const historyPath = join(root, "docs", "history.json");

const REMOTE = "tyan";
const REMOTE_DIR = "/data/aurora-web/hark";
const PUBLIC = "https://madgodinc.net/hark";

const [versionArg, notesArg] = process.argv.slice(2);
const notes = (notesArg ?? "Обновление Hark").trim();

if (!existsSync(keyPath)) {
  console.error(`No signing key at ${keyPath}. Without it no installed copy accepts the update.`);
  process.exit(1);
}

const conf = JSON.parse(readFileSync(confPath, "utf8"));
if (versionArg && versionArg !== "--same") {
  if (!/^\d+\.\d+\.\d+$/.test(versionArg)) {
    console.error(`Version must look like 1.2.3, got "${versionArg}".`);
    process.exit(1);
  }
  conf.version = versionArg;
  writeFileSync(confPath, `${JSON.stringify(conf, null, 2)}\n`);
}
const version = conf.version;

// The binary reports CARGO_PKG_VERSION; keep all three in step before building.
const cargo = readFileSync(cargoPath, "utf8");
writeFileSync(cargoPath, cargo.replace(/^version = ".*"$/m, `version = "${version}"`));
const pkg = JSON.parse(readFileSync(pkgPath, "utf8"));
pkg.version = version;
writeFileSync(pkgPath, `${JSON.stringify(pkg, null, 2)}\n`);

console.log(`building Hark ${version}`);
execFileSync("npx", ["tauri", "build"], {
  cwd: root,
  stdio: "inherit",
  shell: true,
  // The key contents, not a path: a relative path resolves differently per shell.
  env: {
    ...process.env,
    TAURI_SIGNING_PRIVATE_KEY: readFileSync(keyPath, "utf8").trim(),
    TAURI_SIGNING_PRIVATE_KEY_PASSWORD: "",
  },
});

const nsis = join(root, "src-tauri", "target", "release", "bundle", "nsis");
const files = readdirSync(nsis);
const installer = files.find((f) => f.endsWith(".exe") && f.includes(`_${version}_`));
const sigFile = files.find((f) => f.endsWith(".exe.sig") && f.includes(`_${version}_`));
if (!installer || !sigFile) {
  console.error(`Built, but no signed installer for ${version} in ${nsis}: ${files.join(", ")}`);
  process.exit(1);
}
const signature = readFileSync(join(nsis, sigFile), "utf8").trim();

const out = join(root, "release");
mkdirSync(out, { recursive: true });
copyFileSync(join(nsis, installer), join(out, "Hark-setup.exe"));

console.log(`uploading ${installer}`);
execFileSync("ssh", [REMOTE, `mkdir -p ${REMOTE_DIR}`], { stdio: "inherit" });
execFileSync("scp", [join(nsis, installer), `${REMOTE}:${REMOTE_DIR}/`], { stdio: "inherit" });
// Stable name for the download button: upload under a temp name, then rename,
// so a download that starts mid-upload never gets half a file.
execFileSync("scp", [join(out, "Hark-setup.exe"), `${REMOTE}:${REMOTE_DIR}/Hark-setup.exe.part`], { stdio: "inherit" });
execFileSync("ssh", [REMOTE, `mv -f ${REMOTE_DIR}/Hark-setup.exe.part ${REMOTE_DIR}/Hark-setup.exe`], { stdio: "inherit" });

// History: the file in the repository is the source of truth, the server gets a copy.
let history = [];
try {
  history = JSON.parse(readFileSync(historyPath, "utf8"));
} catch {}
const lines = notes.split("\n").map((l) => l.trim()).filter(Boolean);
const entry = {
  version,
  date: new Date().toISOString().slice(0, 10),
  summary: lines.find((l) => !/^[-–—]/.test(l)) ?? `Hark ${version}`,
  changes: lines.filter((l) => /^[-–—]/.test(l)).map((l) => l.replace(/^[-–—]\s*/, "")),
};
history = [entry, ...history.filter((h) => h.version !== version)];
mkdirSync(join(root, "docs"), { recursive: true });
writeFileSync(historyPath, `${JSON.stringify(history, null, 2)}\n`);
execFileSync("scp", [historyPath, `${REMOTE}:${REMOTE_DIR}/history.json`], { stdio: "inherit" });

const manifest = {
  version,
  notes,
  pub_date: new Date().toISOString(),
  platforms: {
    "windows-x86_64": { signature, url: `${PUBLIC}/${encodeURIComponent(installer)}` },
  },
};
const manifestPath = join(out, "latest.json");
writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
execFileSync("scp", [manifestPath, `${REMOTE}:${REMOTE_DIR}/latest.json.part`], { stdio: "inherit" });
execFileSync("ssh", [REMOTE, `mv -f ${REMOTE_DIR}/latest.json.part ${REMOTE_DIR}/latest.json`], { stdio: "inherit" });

console.log(`released Hark ${version}: ${PUBLIC}/latest.json`);
