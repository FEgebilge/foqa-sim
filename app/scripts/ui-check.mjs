// Maintainer check: src/ui still matches the egebilge/ui commit in its vendor manifest. The library is looked for in
// EGEBILGE_UI, then checked out next to this repo; without it there is nothing to compare against, so the check is
// skipped: src/ui is part of this repo and the app builds without the library.
import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const app = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const parent = resolve(app, "..", "..");
const script = (dir) => join(dir, "scripts", "vendor.mjs");
const lib = [process.env.EGEBILGE_UI, join(parent, "egebilge-ui"), join(parent, "GitHub", "egebilge-ui")]
  .find((dir) => dir && existsSync(script(dir)));

if (!lib) {
  console.log("ui:check skipped: egebilge/ui is not checked out next to this repo (or set EGEBILGE_UI to its folder).");
  process.exit(0);
}
const run = spawnSync(process.execPath, [script(lib), "--to", "src/ui", "--check"], { cwd: app, stdio: "inherit" });
process.exit(run.status ?? 1);
