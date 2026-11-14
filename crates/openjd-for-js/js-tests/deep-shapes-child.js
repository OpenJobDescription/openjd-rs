// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

// Run by expression-length.test.js in a child `node --stack-size=<KB>`
// process. Warms the module up, then parses every deep shape at the
// module's length cap and prints one JSON line: the cap and, per shape,
// "RangeError" if the engine's call stack overflowed or "ok" otherwise.

import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { SHAPES, fit, moduleCap, parseOutcome, warmUp } from "./deep-shapes.js";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const mod = await import(join(root, "index.js"));
await mod.default({ module_or_path: await readFile(join(root, "pkg", "openjd_for_js_bg.wasm")) });

warmUp(mod);
const cap = moduleCap(mod);
const results = {};
for (const [name, [shape]] of Object.entries(SHAPES)) {
  try {
    parseOutcome(mod, shape(fit(shape, cap)));
    results[name] = "ok";
  } catch (e) {
    if (!(e instanceof RangeError)) throw e;
    results[name] = "RangeError";
    // The module is unusable after a RangeError; stop here.
    break;
  }
}
console.log(JSON.stringify({ cap, results }));
