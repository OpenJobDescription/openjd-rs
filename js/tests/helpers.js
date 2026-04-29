// Helper to load the WASM module in Node.js (vitest)
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const __dirname = dirname(fileURLToPath(import.meta.url));
const PKG_DIR = join(__dirname, "..", "..", "pkg");

let initialized = false;

export async function loadWasm() {
  if (initialized) return;

  const wasmPath = join(PKG_DIR, "openjd_for_js_bg.wasm");
  const wasmBytes = await readFile(wasmPath);

  // Dynamic import of the generated JS glue
  const mod = await import(join(PKG_DIR, "openjd_for_js.js"));
  await mod.default(wasmBytes);
  initialized = true;
  return mod;
}

export async function getModule() {
  await loadWasm();
  return import(join(PKG_DIR, "openjd_for_js.js"));
}
