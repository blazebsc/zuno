// Runs every `*.check.ts` under landing/src, one process each (same approach as the app's scripts/run-checks.mjs).
import { build } from "esbuild";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readdirSync, rmSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, relative, resolve } from "node:path";

const ROOT = resolve(import.meta.dirname, "..");

function findChecks(dir) {
  return readdirSync(dir).flatMap((entry) => {
    const path = join(dir, entry);
    if (statSync(path).isDirectory()) return findChecks(path);
    return entry.endsWith(".check.ts") ? [path] : [];
  });
}

const checks = findChecks(join(ROOT, "src")).sort();
const outDir = mkdtempSync(join(tmpdir(), "zuno-landing-checks-"));
let failed = 0;

try {
  for (const check of checks) {
    const name = relative(ROOT, check);
    const bundle = join(outDir, `${name.replace(/[\\/]/g, "_")}.mjs`);
    try {
      await build({ entryPoints: [check], bundle: true, platform: "node", format: "esm", outfile: bundle, logLevel: "silent" });
    } catch (error) {
      failed += 1;
      console.error(`FAILED (bundle) ${name}\n${error?.message ?? error}`);
      continue;
    }
    const ran = spawnSync(process.execPath, [bundle], { encoding: "utf8", timeout: 30_000 });
    if (ran.status === 0) {
      console.log(`  ok  ${name}`);
    } else {
      failed += 1;
      console.error(`FAILED ${name}\n${(ran.stderr || ran.stdout || "timed out").trim()}`);
    }
  }
} finally {
  rmSync(outDir, { recursive: true, force: true });
}

if (checks.length === 0 || failed > 0) process.exit(1);
console.log(`\n${checks.length} checks passed.`);
