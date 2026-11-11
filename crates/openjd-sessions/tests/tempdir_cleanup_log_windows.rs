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
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use log::{Level, Log, Metadata, Record};
use openjd_sessions::session::{Session, SessionConfig};

#[derive(Debug, Clone, PartialEq)]
struct Captured {
    level: Level,
    target: String,
    content: Option<u64>,
    session_id: Option<String>,
    message: String,
}

static CAPTURED: Mutex<Vec<Captured>> = Mutex::new(Vec::new());

struct CapturingLogger;

impl Log for CapturingLogger {
    fn enabled(&self, _: &Metadata) -> bool {
        true
    }

    fn log(&self, record: &Record) {
        struct Visitor {
            content: Option<u64>,
            session_id: Option<String>,
        }
        impl<'kvs> log::kv::VisitSource<'kvs> for Visitor {
            fn visit_pair(
                &mut self,
                key: log::kv::Key<'kvs>,
                value: log::kv::Value<'kvs>,
            ) -> Result<(), log::kv::Error> {
                match key.as_str() {
                    "openjd_log_content" => self.content = value.to_u64(),
                    "session_id" => self.session_id = Some(value.to_string()),
                    _ => {}
                }
                Ok(())
            }
        }
        let mut v = Visitor {
            content: None,
            session_id: None,
        };
        let _ = record.key_values().visit(&mut v);
        CAPTURED.lock().unwrap().push(Captured {
            level: record.level(),
            target: record.target().to_string(),
            content: v.content,
            session_id: v.session_id,
            message: record.args().to_string(),
        });
    }

    fn flush(&self) {}
}

#[test]
fn failed_attempt_warning_carries_the_cleanup_content_flags() {
    log::set_logger(&CapturingLogger).unwrap();
    log::set_max_level(log::LevelFilter::Trace);

    // 1. TempDir::cleanup (no session id): a briefly held file, deleted on retry.
    let mut td = openjd_sessions::TempDir::new(None, None, None).unwrap();
    let locked = td.path().join("locked.bin");
    let held = hold(&locked);
    let release = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(200));
        drop(held);
    });
    let result = td.cleanup();
    release.join().unwrap();
    result.unwrap();
    assert!(!td.path().exists());

    let warnings = captured_warnings();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    let w = &warnings[0];
    assert_eq!(w.level, Level::Warn);
    assert_eq!(w.target, "openjd.sessions");
    // FILE_PATH (2) | EXCEPTION_INFO (16) | PROCESS_CONTROL (32), as a literal.
    assert_eq!(w.content, Some(50));
    assert_eq!(w.session_id, None, "TempDir::cleanup logs no session id");
    assert!(w.message.ends_with("Retrying in 1 s."), "{}", w.message);

    // 2. Session::cleanup with a file locked in the EMBEDDED FILES directory,
    //    held for the whole cleanup. This drives the session-tagged log path,
    //    and confirms the lock is waited out once (one working-dir warning per
    //    pass, not two), then the give-up error.
    CAPTURED.lock().unwrap().clear();
    let root = openjd_sessions::TempDir::new(None, None, None).unwrap();
    let mut session = Session::with_config(session_config(root.path(), "sess-audit")).unwrap();
    let files_dir = session.files_directory().to_path_buf();
    assert!(
        files_dir.starts_with(session.working_directory()),
        "embedded files dir is inside the working dir: {files_dir:?}"
    );
    let held = hold(&files_dir.join("locked.bin"));

    session.cleanup();
    drop(held);

    let warnings = captured_warnings();
    // One warning per pass (6 passes: the first plus one per retry delay), each
    // tagged with the session id. Two per pass would mean the embedded files
    // directory was cleaned separately and the lock waited out twice.
    assert_eq!(warnings.len(), 6, "{warnings:?}");
    for w in &warnings {
        assert_eq!(w.level, Level::Warn);
        assert_eq!(w.content, Some(50), "{}", w.message);
        assert_eq!(w.session_id.as_deref(), Some("sess-audit"), "{}", w.message);
        assert!(w.message.contains("locked.bin"), "{}", w.message);
    }
    assert!(warnings.last().unwrap().message.ends_with("Giving up."));

    // The give-up error from Session::cleanup, at error level, session-tagged.
    let errors: Vec<Captured> = CAPTURED
        .lock()
        .unwrap()
        .iter()
        .filter(|c| c.level == Level::Error && c.message.contains("could not be deleted"))
        .cloned()
        .collect();
    assert_eq!(errors.len(), 1, "{errors:?}");
    // FILE_PATH (2) | EXCEPTION_INFO (16), as a literal.
    assert_eq!(errors[0].content, Some(18));
    assert_eq!(errors[0].session_id.as_deref(), Some("sess-audit"));
    assert!(
        errors[0]
            .message
            .starts_with("Files within temporary directory"),
        "{}",
        errors[0].message
    );
}

fn hold(path: &Path) -> std::fs::File {
    std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .share_mode(0)
        .open(path)
        .unwrap()
}

fn captured_warnings() -> Vec<Captured> {
    CAPTURED
        .lock()
        .unwrap()
        .iter()
        .filter(|c| {
            c.message
                .starts_with("Could not delete temporary directory")
        })
        .cloned()
        .collect()
}

fn session_config(root: &Path, id: &str) -> SessionConfig {
    SessionConfig {
        limits: Default::default(),
        session_id: id.into(),
        job_parameter_values: Default::default(),
        session_root_directory: Some(root.to_path_buf()),
        path_mapping_rules: None,
        retain_working_dir: false,
        callback: None,
        os_env_vars: None,
        user: None,
        profile: None,
        cancel_token: None,
        debug_collect_stdout: false,
        echo_openjd_directives: true,
        sticky_bit_policy: openjd_sessions::StickyBitPolicy::Disabled,
    }
}
