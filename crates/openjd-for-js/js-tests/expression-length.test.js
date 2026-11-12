// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

// Long and deeply nested expressions. wasm32 can't spawn threads, so
// openjd-expr parses every expression on the caller's stack here, and caps
// expression source at 512 bytes in wasm builds (`MAX_PARSE_INPUT_LEN`) so
// that the parser's recursion fits the engine's call stack. An input that
// overflows that stack throws a RangeError out of the middle of the parse and
// leaves the module inconsistent, so every input here must parse or throw an
// ordinary error.

import { describe, it, expect, beforeAll } from "vitest";
import { getModule } from "./helpers.js";

/** openjd-expr's `MAX_PARSE_INPUT_LEN` in a wasm32 build. */
const CAP = 512;

let mod;

beforeAll(async () => {
  mod = await getModule();
});

/** The largest `n` for which `shape(n)` is at most `cap` bytes. */
function fit(shape, cap) {
  let n = 0;
  while (shape(n + 1).length <= cap) n++;
  return n;
}

/**
 * Parse `src`, returning `"OK"` or the error. Rethrows a RangeError, which
 * means the engine's call stack overflowed.
 */
function parseOutcome(src) {
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

// The shapes that recurse deepest per input byte: nested and unclosed
// brackets, prefix operators, f-strings, and long left-nested chains.
// Each maps to the outcome expected at the cap: "OK", or a pattern the error
// message matches.
const SHAPES = {
  "nested parentheses": [(n) => "(".repeat(n) + "1" + ")".repeat(n), "OK"],
  "nested lists": [(n) => "[".repeat(n) + "1" + "]".repeat(n), /nesting depth/],
  "nested calls": [(n) => "f(".repeat(n) + "1" + ")".repeat(n), /nesting depth/],
  "unclosed parentheses": [(n) => "(".repeat(n) + "1", /unexpected EOF/],
  "unclosed lists": [(n) => "[".repeat(n) + "1", /unexpected EOF/],
  "unclosed calls": [(n) => "f(".repeat(n) + "1", /unexpected EOF/],
  "unclosed calls on juxtaposed names": [(n) => "[i f(".repeat(n) + "x", /Syntax error/],
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

describe("long expressions", () => {
  it("parses and evaluates an expression longer than 200 bytes", () => {
    // Inputs over 200 bytes used to need a parser thread, which wasm32
    // can't spawn: "Failed to spawn parser thread".
    const src = "1" + " + 1".repeat(62);
    expect(src.length).toBeGreaterThan(200);
    const st = new mod.SymbolTable();
    const result = mod.evaluateExpression(src, st);
    expect(result.toString()).toBe("63");
    result.free();
    st.free();
  });

  it("accepts an expression of exactly the cap", () => {
    const src = "'" + "a".repeat(CAP - 2) + "'";
    expect(src.length).toBe(CAP);
    const st = new mod.SymbolTable();
    const result = mod.evaluateExpression(src, st);
    expect(result.toString()).toBe("a".repeat(CAP - 2));
    result.free();
    st.free();
  });

  it("rejects an expression one byte over the cap", () => {
    const src = "'" + "a".repeat(CAP - 1) + "'";
    expect(() => mod.parseExpression(src)).toThrow(
      `Expression source length (${CAP + 1} bytes) exceeds maximum allowed ` +
        `(${CAP} bytes in WebAssembly builds)`,
    );
  });

  it("measures the cap after trimming white space", () => {
    const src = "   " + "'" + "a".repeat(CAP - 2) + "'" + "\n ";
    expect(parseOutcome(src)).toBe("OK");
  });
});

describe("deeply nested expressions at the cap", () => {
  for (const [name, [shape, expected]] of Object.entries(SHAPES)) {
    it(`${name}: parses or throws an ordinary error`, () => {
      const src = shape(fit(shape, CAP));
      expect(src.length).toBeGreaterThan(CAP - 12);
      const outcome = parseOutcome(src);
      if (expected === "OK") {
        expect(outcome).toBe("OK");
      } else {
        expect(outcome).toBeInstanceOf(Error);
        expect(outcome.message).toMatch(expected);
      }
    });
  }

  it("rejects the same shapes at 64 KB by length, before parsing", () => {
    for (const [shape] of Object.values(SHAPES)) {
      const outcome = parseOutcome(shape(fit(shape, 64 * 1024)));
      expect(outcome).toBeInstanceOf(Error);
      expect(outcome.message).toMatch(/exceeds maximum allowed \(512 bytes/);
    }
  });

  it("leaves the module usable afterwards", () => {
    const st = new mod.SymbolTable();
    const result = mod.evaluateExpression("1 + 2", st);
    expect(result.toString()).toBe("3");
    result.free();
    st.free();
  });
});

describe("format strings", () => {
  it("applies the cap to each interpolation, not the whole string", () => {
    // Twenty 40-byte interpolations: 900 bytes in all, each under the cap.
    const part = "{{ " + "1" + " + 1".repeat(9) + " }}";
    const fs = new mod.FormatString(part.repeat(20));
    const st = new mod.SymbolTable();
    expect(fs.resolve(st)).toBe("10".repeat(20));
    st.free();
    fs.free();
  });
});
