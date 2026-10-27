// Sets the version everywhere it lives, then builds the installer:
//   npm run release -- 0.1.1   (needs devlog/0.1.1.md first)
// The installer tells an update from a reinstall by this version, and the
// in-sim packages carry it too (msfs-panel/layout.ps1), so it must grow.
import { execSync } from "node:child_process";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../../", import.meta.url));
const version = process.argv[2];
const semver = /^(\d+)\.(\d+)\.(\d+)$/;
if (!semver.test(version ?? "")) {
  console.error("usage: npm run release -- <major.minor.patch>, e.g. 0.1.1");
  process.exit(1);
}

// The release notes are written first: devlog/<version>.md, whose frontmatter names this version.
const notes = `${root}devlog/${version}.md`;
if (!existsSync(notes)) {
  console.error(`missing ${notes}: write the release notes first (see devlog/README.md)`);
  process.exit(1);
}
const noted = /^---\r?\n(?:[^\n]*\n)*?version:\s*(\S+)/.exec(readFileSync(notes, "utf8").replace(/^﻿/, ""))?.[1];
if (noted !== version) {
  console.error(`${notes} says version ${noted ?? "(none)"}, expected ${version}`);
  process.exit(1);
}

const conf = `${root}app/src-tauri/tauri.conf.json`;
const current = JSON.parse(readFileSync(conf, "utf8")).version;
const parts = (v) => v.match(semver).slice(1).map(Number);
const newer = parts(version).reduce((cmp, n, i) => cmp || Math.sign(n - parts(current)[i]), 0) > 0;
if (!newer) {
  console.error(`${version} is not newer than ${current}`);
  process.exit(1);
}

// The app's own version, not a dependency's: each file's first, unless the
// pattern is global.
const bump = (file, pattern) => {
  const path = `${root}${file}`;
  const text = readFileSync(path, "utf8");
  if (!pattern.test(text)) throw new Error(`no version in ${file}`);
  writeFileSync(path, text.replace(pattern, (m) => m.replace(current, version)));
};
bump("app/src-tauri/tauri.conf.json", /"version": "[^"]+"/);
bump("app/package.json", /"version": "[^"]+"/);
// package-lock.json names it twice, each after the app's name: the
// lockfile's and its root package's.
bump("app/package-lock.json", /"name": "foqa-sim-app",\s*"version": "[^"]+"/g);
bump("Cargo.toml", /\[workspace\.package\]\s*version = "[^"]+"/);

const run = (cmd, cwd) => execSync(cmd, { cwd: `${root}${cwd}`, stdio: "inherit" });
run("cargo update --workspace", "");
run("npm run tauri build", "app");
console.log(`\nFOQA-Sim ${version}: target\\release\\bundle\\nsis\\FOQA-Sim_${version}_x64-setup.exe`);
console.log(`Release notes: ${notes}`);
