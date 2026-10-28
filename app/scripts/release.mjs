// Sets the version everywhere it lives, builds and signs the installer, and prepares the GitHub release:
//   npm run release -- 0.1.1             (needs devlog/0.1.1.md first; prints the `gh release create` command)
//   npm run release -- 0.1.1 --publish   (runs it, as a DRAFT: check it on GitHub, then publish it there)
// The installer tells an update from a reinstall by this version, and the
// in-sim packages carry it too (msfs-panel/layout.ps1), so it must grow.
// The updater key (docs/releasing.md, section 1) signs the installer; its password is asked for here, hidden.
import { execSync, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { copyFileSync, existsSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import { join } from "node:path";
import { createInterface } from "node:readline";
import { Writable } from "node:stream";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../../", import.meta.url));
const args = process.argv.slice(2);
const publish = args.includes("--publish");
const version = args.find((arg) => !arg.startsWith("--"));
const semver = /^(\d+)\.(\d+)\.(\d+)$/;
if (!semver.test(version ?? "")) {
  console.error("usage: npm run release -- <major.minor.patch> [--publish], e.g. 0.1.1");
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

// Fail before anything is changed: the build needs the updater key, the release needs gh.
const keyFile = process.env.FOQA_UPDATER_KEY ?? join(homedir(), ".tauri", "foqa-sim.key");
if (!existsSync(keyFile)) {
  console.error(`missing updater key ${keyFile}: generate it (docs/releasing.md, section 1) or set FOQA_UPDATER_KEY to its path`);
  process.exit(1);
}
if (spawnSync("gh", ["auth", "status"], { stdio: "ignore" }).status !== 0) {
  console.error("gh is not signed in (or not installed): run `gh auth login`, then try again");
  process.exit(1);
}

const conf = `${root}app/src-tauri/tauri.conf.json`;
const current = JSON.parse(readFileSync(conf, "utf8")).version;
const parts = (v) => v.match(semver).slice(1).map(Number);
const order = parts(version).reduce((cmp, n, i) => cmp || Math.sign(n - parts(current)[i]), 0);
if (order < 0) {
  console.error(`${version} is older than ${current}`);
  process.exit(1);
}
// Equal: a release whose build failed after the bump; build it again.
const bumping = order > 0;

// The app's own version, not a dependency's: each file's first, unless the
// pattern is global.
const bump = (file, pattern) => {
  const path = `${root}${file}`;
  const text = readFileSync(path, "utf8");
  if (!pattern.test(text)) throw new Error(`no version in ${file}`);
  writeFileSync(path, text.replace(pattern, (m) => m.replace(current, version)));
};
if (bumping) {
  bump("app/src-tauri/tauri.conf.json", /"version": "[^"]+"/);
  bump("app/package.json", /"version": "[^"]+"/);
  // package-lock.json names it twice, each after the app's name: the
  // lockfile's and its root package's.
  bump("app/package-lock.json", /"name": "foqa-sim-app",\s*"version": "[^"]+"/g);
  bump("Cargo.toml", /\[workspace\.package\]\s*version = "[^"]+"/);
}

/** Reads a line without echoing it. Nothing typed is shown, stored or logged. */
const askHidden = (question) => new Promise((resolve) => {
  let muted = false;
  const output = new Writable({ write(chunk, encoding, done) { if (!muted) process.stdout.write(chunk, encoding); done(); } });
  const rl = createInterface({ input: process.stdin, output, terminal: true });
  rl.question(question, (answer) => { rl.close(); process.stdout.write("\n"); resolve(answer); });
  muted = true;
});

// The signing key's password goes to the build process only.
let password = process.env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD;
if (password === undefined) {
  if (!process.stdin.isTTY) {
    console.error("no terminal to ask for the key password: set TAURI_SIGNING_PRIVATE_KEY_PASSWORD");
    process.exit(1);
  }
  password = await askHidden("Updater key password: ");
}
const signing = { TAURI_SIGNING_PRIVATE_KEY: keyFile, TAURI_SIGNING_PRIVATE_KEY_PASSWORD: password };

const run = (cmd, cwd, env = {}) => execSync(cmd, { cwd: `${root}${cwd}`, stdio: "inherit", env: { ...process.env, ...env } });
run("cargo update --workspace", "");
run("npm run tauri build", "app", signing);

// Everything the release carries sits in the bundle folder.
const bundle = `${root}target/release/bundle/nsis/`;
const installer = `FOQA-Sim_${version}_x64-setup.exe`;
const stable = "FOQA-Sim_x64-setup.exe";
for (const file of [installer, `${installer}.sig`]) {
  if (!existsSync(bundle + file)) throw new Error(`the build did not write ${bundle + file}`);
}
copyFileSync(bundle + installer, bundle + stable);

// The notes are the devlog without its front matter: in latest.json (shown by the app) and in the release.
const body = readFileSync(notes, "utf8").replace(/^﻿/, "").replace(/^---\r?\n[\s\S]*?\r?\n---\r?\n?/, "").trim();
writeFileSync(`${bundle}latest.json`, `${JSON.stringify({
  version,
  notes: body,
  pub_date: new Date().toISOString(),
  platforms: { "windows-x86_64": {
    signature: readFileSync(`${bundle}${installer}.sig`, "utf8").trim(),
    url: `https://github.com/FEgebilge/foqa-sim/releases/download/v${version}/${installer}`,
  } },
}, null, 2)}\n`);

const sha256 = createHash("sha256").update(readFileSync(bundle + installer)).digest("hex");
const releaseNotes = join(mkdtempSync(join(tmpdir(), "foqa-release-")), "notes.md");
writeFileSync(releaseNotes, `${body}\n\nSHA-256 of ${stable}: \`${sha256}\`\n`);

// Always a draft: only publishing it on GitHub makes /releases/latest (and so the updater) see it.
const create = ["release", "create", `v${version}`, "--draft", "--title", `FOQA-Sim ${version}`, "--notes-file", releaseNotes,
  ...[installer, `${installer}.sig`, stable, "latest.json"].map((file) => bundle + file)];
const quote = (arg) => (/[\s"]/.test(arg) ? `"${arg}"` : arg);
console.log(`\nFOQA-Sim ${version}: ${bundle}${installer}`);
console.log(`SHA-256: ${sha256}`);
console.log(`Release notes: ${notes}`);
console.log(`\ngh ${create.map(quote).join(" ")}`);
if (!publish) console.log("\nNot created: add --publish to run the command above (it makes a draft).");
else if (spawnSync("gh", create, { stdio: "inherit" }).status !== 0) process.exit(1);
else console.log(`\nDraft v${version} created: check its assets on GitHub, then publish it there.`);
