// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Typing at every stage a template sees: template validation (every
//! symbol unresolved), job creation (`Param.*` concrete, `Task.*`
//! unresolved), and run time (everything concrete). No stage rejects an
//! expression that a later stage's values let succeed.

mod common;
mod conditions;
mod list_joins;
mod listcomp_loop_variable;
mod listcomp_result;
mod union_list_forms;
mod unknown_types;
