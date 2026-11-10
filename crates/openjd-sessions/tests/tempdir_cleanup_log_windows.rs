// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! The warning `TempDir::cleanup` logs for a failed attempt reaches the logger
//! with the content flags that decide where it is shown.
//!
//! This is its own test binary because it installs a process-wide logger that
//! records key-value fields, which the unit tests' `testing_logger` does not.

#![cfg(windows)]

use std::os::windows::fs::OpenOptionsExt;
use std::sync::Mutex;
use std::time::Duration;

use log::{Level, Log, Metadata, Record};

#[derive(Debug, Clone, PartialEq)]
struct Captured {
    level: Level,
    target: String,
    content: Option<u64>,
    message: String,
}

static CAPTURED: Mutex<Vec<Captured>> = Mutex::new(Vec::new());

struct CapturingLogger;

impl Log for CapturingLogger {
    fn enabled(&self, _: &Metadata) -> bool {
        true
    }

    fn log(&self, record: &Record) {
        struct Visitor(Option<u64>);
        impl<'kvs> log::kv::VisitSource<'kvs> for Visitor {
            fn visit_pair(
                &mut self,
                key: log::kv::Key<'kvs>,
                value: log::kv::Value<'kvs>,
            ) -> Result<(), log::kv::Error> {
                if key.as_str() == "openjd_log_content" {
                    self.0 = value.to_u64();
                }
                Ok(())
            }
        }
        let mut v = Visitor(None);
        let _ = record.key_values().visit(&mut v);
        CAPTURED.lock().unwrap().push(Captured {
            level: record.level(),
            target: record.target().to_string(),
            content: v.0,
            message: record.args().to_string(),
        });
    }

    fn flush(&self) {}
}

#[test]
fn failed_attempt_warning_carries_the_cleanup_content_flags() {
    log::set_logger(&CapturingLogger).unwrap();
    log::set_max_level(log::LevelFilter::Trace);

    let mut td = openjd_sessions::TempDir::new(None, None, None).unwrap();
    let locked = td.path().join("locked.bin");
    let held = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .share_mode(0)
        .open(&locked)
        .unwrap();
    let release = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(200));
        drop(held);
    });

    let result = td.cleanup();
    release.join().unwrap();

    result.unwrap();
    assert!(!td.path().exists());
    let warnings: Vec<Captured> = CAPTURED
        .lock()
        .unwrap()
        .iter()
        .filter(|c| {
            c.message
                .starts_with("Could not delete temporary directory")
        })
        .cloned()
        .collect();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    let w = &warnings[0];
    assert_eq!(w.level, Level::Warn);
    assert_eq!(w.target, "openjd.sessions");
    // FILE_PATH (2) | EXCEPTION_INFO (16) | PROCESS_CONTROL (32), as a literal.
    assert_eq!(w.content, Some(50));
    assert!(w.message.ends_with("Retrying in 1 s."), "{}", w.message);
}
