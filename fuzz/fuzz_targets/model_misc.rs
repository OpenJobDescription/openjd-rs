// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| openjd_fuzz::targets::model::misc(data));
