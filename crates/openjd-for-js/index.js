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
// because a wasm-bindgen error thrown from inside wasm leaves the heap
// inconsistent and repeated ones corrupt it.
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
  // Check every element here, before any call into wasm. A wasm-bindgen
  // error thrown from inside wasm, such as its "array contains a value of
  // the wrong type" for a wrong-typed `Vec<PathMappingRule>` element or its
  // "null pointer passed to rust" for a freed object, leaves the wasm heap
  // inconsistent, and some thousands to tens of thousands of them corrupt
  // it. A non-iterable would also be read as an empty list.
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
  // Check the pointers only after iteration has finished: an iterator could
  // free a rule it already yielded. After this point no code the caller
  // passed in runs, except from hostile objects (getters, Proxies) or
  // replaced built-ins, which can misuse the raw wasm-bindgen glue anyway.
  // A freed rule, or an object made with Object.create(PathMappingRule.prototype),
  // has no live wasm pointer. `__wbg_ptr` is wasm-bindgen's handle field, a
  // positive 32-bit integer for a live object.
  checked.forEach((rule, index) => {
    const ptr = rule.__wbg_ptr;
    if (!(Number.isInteger(ptr) && ptr > 0 && ptr <= 0xffffffff)) {
      throw new TypeError(
        `FunctionLibrary.withPathMappingRules: rules[${index}] has been freed`,
      );
    }
  });
  // Copy only after every element passed, so a rejected call allocates
  // nothing and leaves the caller's rules as they were.
  return withPathMappingRules.call(
    FunctionLibrary,
    checked.map((rule) => rule.__clone()),
  );
};
