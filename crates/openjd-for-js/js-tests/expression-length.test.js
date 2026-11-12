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
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { getModule } from "./helpers.js";
import { SHAPES, fit, moduleCap, parseOutcome, warmUp } from "./deep-shapes.js";

/** openjd-expr's `MAX_PARSE_INPUT_LEN` in a wasm32 build. */
const CAP = 512;

/**
 * Engine stack, in KB, that the deep shapes at the cap must fit in after a
 * warm-up. Node's default is 984 KB. A warmed module needed at most about
 * 600 KB in Node.js 24, so this leaves some room for variation between Node
 * versions while still failing if the parser's frames grow by about a fifth,
 * or if the cap is raised much above 512 bytes.
 */
const REDUCED_STACK_KB = 720;

let mod;

beforeAll(async () => {
  mod = await getModule();
});

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
    expect(moduleCap(mod)).toBe(CAP);
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
    expect(parseOutcome(mod, src)).toBe("OK");
  });
});

describe("deeply nested expressions at the cap", () => {
  for (const [name, [shape, expected]] of Object.entries(SHAPES)) {
    it(`${name}: parses or throws an ordinary error`, () => {
      const src = shape(fit(shape, CAP));
      expect(src.length).toBeGreaterThan(CAP - 12);
      const outcome = parseOutcome(mod, src);
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
      const outcome = parseOutcome(mod, shape(fit(shape, 64 * 1024)));
      expect(outcome).toBeInstanceOf(Error);
      expect(outcome.message).toMatch(/exceeds maximum allowed \(512 bytes/);
    }
  });

  it("parses them after a warm-up, too", () => {
    // The engine's optimized frames are larger than its baseline ones.
    warmUp(mod);
    for (const [shape] of Object.values(SHAPES)) {
      parseOutcome(mod, shape(fit(shape, CAP)));
    }
  });

  it(`parses them in ${REDUCED_STACK_KB} KB of engine stack`, () => {
    // Guards the margin below Node's default 984 KB stack, which the shapes
    // above, run at the default, can't see. The child process reads the cap
    // from the module, so it also fails if the cap is raised.
    const child = join(dirname(fileURLToPath(import.meta.url)), "deep-shapes-child.js");
    const out = execFileSync(process.execPath, [`--stack-size=${REDUCED_STACK_KB}`, child], {
      encoding: "utf8",
    });
    const { cap, results } = JSON.parse(out.trim().split("\n").pop());
    expect(cap).toBe(CAP);
    expect(results).toEqual(Object.fromEntries(Object.keys(SHAPES).map((k) => [k, "ok"])));
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
    // Twenty 43-byte interpolations, 860 bytes in all, each expression 37
    // bytes.
    const part = "{{ " + "1" + " + 1".repeat(9) + " }}";
    expect(part.repeat(20).length).toBe(860);
    const fs = new mod.FormatString(part.repeat(20));
    const st = new mod.SymbolTable();
    expect(fs.resolve(st)).toBe("10".repeat(20));
    st.free();
    fs.free();
  });

  it("rejects an interpolation over the cap", () => {
    const src = "{{ '" + "a".repeat(CAP) + "' }}";
    expect(() => new mod.FormatString(src)).toThrow(/maximum allowed \(512 bytes in WebAssembly builds\)/);
  });
});
