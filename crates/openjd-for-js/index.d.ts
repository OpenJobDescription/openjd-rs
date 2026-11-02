// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

// Types for index.js. The generated pkg/openjd_for_js.d.ts already declares
// the public `evaluate` and `evaluateExpression` signatures (from a
// typescript_custom_section in src/expr.rs) and hides the internal entry
// points that index.js calls.
export * from "./pkg/openjd_for_js.js";
export { default } from "./pkg/openjd_for_js.js";
