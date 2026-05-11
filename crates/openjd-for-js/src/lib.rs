// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! ECMAScript/WebAssembly bindings for the OpenJD model and expression libraries.
//!
//! > **Status: experimental.** This crate is under active development and is
//! > not published to crates.io or to npm (`publish = false`). The JS surface,
//! > the Rust API, the WASM package layout, and the build pipeline may all
//! > change without notice between releases. Not recommended for production
//! > use.
//!
//! Provides spec-compliant template parsing, validation, job creation,
//! expression evaluation, and parameter space expansion for browsers
//! and VS Code extensions.

pub mod errors;
pub mod expr;
pub mod model;
