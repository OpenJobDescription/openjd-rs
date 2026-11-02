// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

// Package entry point. Re-exports the wasm-bindgen output in pkg/ and
// defines the functions that take an optional FunctionLibrary.
//
// wasm-bindgen can take an exported Rust struct as an optional argument only
// by value, which zeroes the caller's JS handle, so a library passed to
// `evaluate` would be used up after one call. A required `&FunctionLibrary`
// argument is borrowed instead. So Rust exports one entry point without a
// library and one that borrows a required library, and the functions below
// pick between them. `withPathMappingRules` takes its rules by value for the
// same reason, so it is given copies. It also checks its argument first,
// because wasm-bindgen's own check of a wrong-typed array element corrupts
// the wasm heap after repeated failures.
//
// The library wrappers can go once wasm-bindgen supports `Option<&T>` for
// exported structs (https://github.com/wasm-bindgen/wasm-bindgen/pull/4873);
// the argument check in withPathMappingRules has to stay or move to Rust.

import {
  FunctionLibrary,
  ParsedExpression,
  PathMappingRule,
  __evaluateExpression,
  __evaluateExpressionWithLibrary,
} from "./pkg/openjd_for_js.js";

export * from "./pkg/openjd_for_js.js";
export { default } from "./pkg/openjd_for_js.js";

ParsedExpression.prototype.evaluate = function evaluate(symbols, lib, options) {
  return lib == null
    ? this.__evaluate(symbols, options)
    : this.__evaluateWithLibrary(symbols, lib, options);
};

export function evaluateExpression(expr, symbols, lib, options) {
  return lib == null
    ? __evaluateExpression(expr, symbols, options)
    : __evaluateExpressionWithLibrary(expr, symbols, lib, options);
}

const withPathMappingRules = FunctionLibrary.withPathMappingRules;
FunctionLibrary.withPathMappingRules = function (rules) {
  // Check every element here. wasm-bindgen's own check for a wrong-typed
  // element in a `Vec<PathMappingRule>` throws without freeing what it had
  // already converted, and a few thousand such calls corrupt the wasm heap.
  // It also accepts a non-iterable as an empty list.
  if (rules == null || typeof rules[Symbol.iterator] !== "function") {
    throw new TypeError(
      "FunctionLibrary.withPathMappingRules: rules must be an array (or other iterable) of PathMappingRule",
    );
  }
  const checked = [];
  let i = 0;
  for (const rule of rules) {
    if (!(rule instanceof PathMappingRule)) {
      throw new TypeError(
        `FunctionLibrary.withPathMappingRules: rules[${i}] is not a PathMappingRule`,
      );
    }
    checked.push(rule);
    i++;
  }
  // Copy only after every element passed, so a rejected call leaves the
  // caller's rules as they were. A freed rule fails in `__clone`; free the
  // copies already made before rethrowing.
  const copies = [];
  try {
    for (const rule of checked) copies.push(rule.__clone());
  } catch (e) {
    copies.forEach((copy) => copy.free());
    throw e;
  }
  return withPathMappingRules.call(FunctionLibrary, copies);
};
