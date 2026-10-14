// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Structured logging with content classification.
//!
//! Mirrors Python's `openjd.sessions._logging.LogContent(Flag)`.
//! The worker agent uses `openjd_log_content` to route log records:
//! only EXCEPTION_INFO | PROCESS_CONTROL | HOST_INFO go to the worker log;
//! all records go to CloudWatch via the session log stream.

use bitflags::bitflags;

bitflags! {
    /// Describes the content of a log record, used by consumers to filter/route logs.
    ///
    /// ```
    /// use openjd_sessions::LogContent;
    ///
    /// let content = LogContent::COMMAND_OUTPUT | LogContent::PROCESS_CONTROL;
    /// assert!(content.contains(LogContent::COMMAND_OUTPUT));
    /// assert!(!content.contains(LogContent::BANNER));
    /// ```
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct LogContent: u32 {
        /// A banner, such as one that visually separates log sections.
        const BANNER = 1 << 0;
        /// A file path.
        const FILE_PATH = 1 << 1;
        /// The contents of a file.
        const FILE_CONTENTS = 1 << 2;
        /// Output from a command that was run.
        const COMMAND_OUTPUT = 1 << 3;
        /// An error openjd encountered. It may include sensitive information,
        /// such as file paths or host information.
        const EXCEPTION_INFO = 1 << 4;
        /// Starting, killing, or signaling processes.
        const PROCESS_CONTROL = 1 << 5;
        /// Parameters, and their values, of the running action.
        const PARAMETER_INFO = 1 << 6;
        /// The system environment, such as dependency versions, OS name, and
        /// CPU architecture.
        const HOST_INFO = 1 << 7;
    }
}

impl log::kv::ToValue for LogContent {
    fn to_value(&self) -> log::kv::Value<'_> {
        log::kv::Value::from(self.bits())
    }
}

/// Emit a structured log record with session_id, openjd_log_content, and
/// a precise timestamp captured at the point of the log call.
///
/// Usage:
///   session_log!(info, session_id, LogContent::HOST_INFO, "message {}", arg);
#[macro_export]
macro_rules! session_log {
    ($level:ident, $session_id:expr, $content:expr, $($arg:tt)+) => {
        log::$level!(
            target: "openjd.sessions",
            session_id = $session_id,
            openjd_log_content = $crate::logging::LogContent::bits(&$content),
            openjd_timestamp_usec = $crate::logging::timestamp_usec();
            $($arg)+
        )
    };
}

/// Return the current time as microseconds since the Unix epoch (u64).
/// Used by `session_log!` to attach a precise timestamp to each log record.
pub fn timestamp_usec() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64
}

/// Log a section banner (major section separator).
pub fn log_section_banner(session_id: &str, title: &str) {
    session_log!(info, session_id, LogContent::BANNER, "");
    session_log!(
        info,
        session_id,
        LogContent::BANNER,
        "=============================================="
    );
    session_log!(info, session_id, LogContent::BANNER, "--------- {}", title);
    session_log!(
        info,
        session_id,
        LogContent::BANNER,
        "=============================================="
    );
}

/// Log a subsection banner (minor section separator).
pub fn log_subsection_banner(session_id: &str, title: &str) {
    session_log!(
        info,
        session_id,
        LogContent::BANNER,
        "----------------------------------------------"
    );
    session_log!(info, session_id, LogContent::BANNER, "{}", title);
    session_log!(
        info,
        session_id,
        LogContent::BANNER,
        "----------------------------------------------"
    );
}
