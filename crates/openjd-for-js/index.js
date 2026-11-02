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
// same reason, so it is given copies.
//
// Remove this file once wasm-bindgen supports `Option<&T>` for exported
// structs (https://github.com/wasm-bindgen/wasm-bindgen/pull/4873).

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
  const copies = Array.from(rules, (rule) =>
    rule instanceof PathMappingRule ? rule.__clone() : rule,
  );
  return withPathMappingRules.call(FunctionLibrary, copies);
};
