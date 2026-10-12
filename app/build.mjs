// Builds the front end into dist/ with esbuild. `node build.mjs --watch` rebuilds on change;
// `node build.mjs --test` builds the tests in tests/ and runs them with node's test runner.
import * as esbuild from "esbuild";
import { cpSync, mkdirSync, readdirSync, rmSync } from "node:fs";
import { spawnSync } from "node:child_process";

const watch = process.argv.includes("--watch");
const test = process.argv.includes("--test");

if (test) {
  rmSync("dist-test", { recursive: true, force: true });
  const entries = readdirSync("tests").filter((f) => f.endsWith(".test.ts")).map((f) => `tests/${f}`);
  await esbuild.build({
    entryPoints: entries, outdir: "dist-test", bundle: true, platform: "node", format: "esm",
    outExtension: { ".js": ".mjs" }, logLevel: "warning",
  });
  const files = readdirSync("dist-test").map((f) => `dist-test/${f}`);
  const r = spawnSync(process.execPath, ["--test", ...files], { stdio: "inherit" });
  process.exit(r.status ?? 1);
}

rmSync("dist", { recursive: true, force: true });
mkdirSync("dist", { recursive: true });
cpSync("index.html", "dist/index.html");
const options = {
  entryPoints: ["src/main.tsx"],
  outdir: "dist",
  bundle: true,
  format: "esm",
  target: ["chrome110", "safari16"],
  jsx: "automatic",
  jsxImportSource: "preact",
  loader: { ".woff2": "file", ".woff": "file" },
  assetNames: "fonts/[name]-[hash]",
  sourcemap: true,
  minify: !watch,
  logLevel: "info",
};
if (watch) {
  const ctx = await esbuild.context(options);
  await ctx.watch();
} else {
  await esbuild.build(options);
}
