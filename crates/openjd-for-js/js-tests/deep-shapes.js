// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

// Expression shapes that recurse deepest per input byte, shared by
// expression-length.test.js and deep-shapes-child.js. Each maps a name to
// [shape, expected]: `shape(n)` builds the input with n repetitions, and
// `expected` is "OK" or a pattern the error message matches at the cap.

export const SHAPES = {
  "nested parentheses": [(n) => "(".repeat(n) + "1" + ")".repeat(n), "OK"],
  "nested lists": [(n) => "[".repeat(n) + "1" + "]".repeat(n), /nesting depth/],
  "nested calls": [(n) => "f(".repeat(n) + "1" + ")".repeat(n), /nesting depth/],
  // Unclosed `{` needs the most engine stack per byte of any shape found.
  "unclosed braces": [(n) => "{".repeat(n) + "1", /unexpected EOF/],
  "unclosed dict keys": [(n) => "{1:".repeat(n) + "1", /unexpected EOF/],
  "unclosed lists and braces": [(n) => "[{".repeat(n) + "1", /unexpected EOF/],
  "unclosed parentheses and braces": [(n) => "({".repeat(n) + "1", /unexpected EOF/],
  "unclosed parentheses": [(n) => "(".repeat(n) + "1", /unexpected EOF/],
  "unclosed lists": [(n) => "[".repeat(n) + "1", /unexpected EOF/],
  "unclosed calls": [(n) => "f(".repeat(n) + "1", /unexpected EOF/],
  "unclosed calls on juxtaposed names": [(n) => "[i f(".repeat(n) + "x", /Syntax error/],
  "starred": [(n) => "[" + "*".repeat(n) + "a]", /Syntax error/],
  "unary minus": [(n) => "-".repeat(n) + "1", /nesting depth/],
  "not": [(n) => "not ".repeat(n) + "1", /nesting depth/],
  "power": [(n) => "2**".repeat(n) + "1", /nesting depth/],
  "conditional": [(n) => "1 if 1 else ".repeat(n) + "1", "OK"],
  "lambda": [(n) => "lambda:".repeat(n) + "1", /Lambda/],
  "unclosed f-strings": [(n) => "f'{".repeat(n) + "x", /f-string/],
  "attribute chain": [(n) => "a" + ".b".repeat(n), /nesting depth/],
  "keyword attribute chain": [(n) => "a" + ".if".repeat(n), /nesting depth/],
  "binary operator chain": [(n) => "1" + "+1".repeat(n), /nesting depth/],
  "subscript chain": [(n) => "x" + "[0]".repeat(n), /nesting depth/],
  "nested comprehensions": [(n) => "[x for x in ".repeat(n) + "[1]" + "]".repeat(n), "OK"],
};

/** The largest `n` for which `shape(n)` is at most `cap` bytes. */
export function fit(shape, cap) {
  let n = 0;
  while (shape(n + 1).length <= cap) n++;
  return n;
}

/**
 * The module's expression length cap, read from the error for an input
 * that is certainly too long, so the tests follow the cap the module was
 * built with instead of a copy of it.
 */
export function moduleCap(mod) {
  try {
    mod.parseExpression("1".repeat(70 * 1024)).free();
  } catch (e) {
    const m = /exceeds maximum allowed \((\d+) bytes/.exec(e.message);
    if (m) return Number(m[1]);
    throw e;
  }
  throw new Error("a 70 KB expression parsed");
}

/**
 * Parse `src`, returning `"OK"` or the error. Rethrows a RangeError, which
 * means the engine's call stack overflowed.
 */
export function parseOutcome(mod, src) {
  let parsed;
  try {
    parsed = mod.parseExpression(src);
  } catch (e) {
    if (e instanceof RangeError) throw e;
    return e;
  }
  parsed.free();
  return "OK";
}

/**
 * Run ordinary and deep expressions many times, so the engine compiles the
 * parser's hot functions with its optimizing tier. Optimized frames are
 * larger: in Node.js 24 a warmed module needs about 1.5 times the stack a
 * fresh one does for the same input.
 */
export function warmUp(mod, iterations = 1000) {
  const st = new mod.SymbolTable();
  const inputs = ["1 + 2 * 3", "[x * 2 for x in range(5)]", "'a'.upper() + 'b'", "(".repeat(150) + "1" + ")".repeat(150)];
  for (let i = 0; i < iterations; i++) {
    for (const src of inputs) mod.evaluateExpression(src, st).free();
  }
  st.free();
}
