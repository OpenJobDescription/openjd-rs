// Helper to load the WASM module in Node.js (vitest)
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const __dirname = dirname(fileURLToPath(import.meta.url));
// pkg/ is produced by `npm run build` as a sibling of this crate's
// package.json, one directory up from js-tests/. Tests load the package
// entry point, index.js, which re-exports pkg/ and adds the functions
// that take an optional FunctionLibrary.
const PKG_DIR = join(__dirname, "..", "pkg");
const ENTRY = join(__dirname, "..", "index.js");

let initialized = false;

export async function loadWasm() {
  if (initialized) return;

  const wasmPath = join(PKG_DIR, "openjd_for_js_bg.wasm");
  const wasmBytes = await readFile(wasmPath);

  const mod = await import(ENTRY);
  await mod.default(wasmBytes);
  initialized = true;
  return mod;
}

export async function getModule() {
  await loadWasm();
  return import(ENTRY);
}
