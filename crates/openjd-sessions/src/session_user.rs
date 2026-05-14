// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Session user types for cross-user execution — mirrors Python `_session_user.py`.

/// Trait for session user identity.
pub trait SessionUser: Send + Sync + std::fmt::Debug {
    fn user(&self) -> &str;
    fn group(&self) -> &str;
    fn is_process_user(&self) -> bool;
    fn as_any(&self) -> &dyn std::any::Any;
}

// ---------------------------------------------------------------------------
// POSIX
// ---------------------------------------------------------------------------

/// POSIX session user identity for cross-user execution via sudo.
#[cfg(unix)]
#[derive(Debug, Clone)]
pub struct PosixSessionUser {
    pub user: String,
    pub group: String,
}

#[cfg(unix)]
impl PosixSessionUser {
    /// Create a new PosixSessionUser.
    ///
    /// If `group` is None, defaults to the current process's effective group.
    pub fn new(user: &str, group: Option<&str>) -> Self {
        let group = match group {
            Some(g) => g.to_string(),
            None => {
                let egid = nix::unistd::getegid();
                nix::unistd::Group::from_gid(egid)
                    .ok()
                    .flatten()
                    .map(|g| g.name)
                    .unwrap_or_else(|| egid.to_string())
            }
        };
        Self {
            user: user.to_string(),
            group,
        }
    }
}

#[cfg(unix)]
impl SessionUser for PosixSessionUser {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn user(&self) -> &str {
        &self.user
    }

    fn group(&self) -> &str {
        &self.group
    }

    fn is_process_user(&self) -> bool {
        let euid = nix::unistd::geteuid();
        nix::unistd::User::from_uid(euid)
            .ok()
            .flatten()
            .map(|u| u.name == self.user)
            .unwrap_or(false)
    }
}

// ---------------------------------------------------------------------------
// Windows
// ---------------------------------------------------------------------------

/// Error for incorrect username or password.
#[cfg(windows)]
#[derive(Debug, thiserror::Error)]
pub enum BadCredentialsError {
    #[error("The username or password is incorrect.")]
    LogonFailure,
    #[error("{0}")]
    Other(String),
}

/// Windows session user identity for cross-user execution.
///
/// Mirrors Python `WindowsSessionUser`. Two authentication modes:
///
/// - **Password mode** (non-Session 0): provide `user` + `password`.
///   Credentials are validated immediately via `LogonUserW`.
/// - **Logon token mode** (Session 0 / services / SSH): provide `user` + `logon_token`.
///
/// If the user is the same as the process owner, neither password nor token is needed.
#[cfg(windows)]
#[derive(Debug)]
pub struct WindowsSessionUser {
    user: String,
    password: Option<String>,
    logon_token: Option<windows::Win32::Foundation::HANDLE>,
}

// SAFETY: `WindowsSessionUser` is Send because all of its fields can be
// sent across threads:
// - `user: String` and `password: Option<String>` are Send by virtue of
//   being owned `String`s.
// - `logon_token: Option<HANDLE>` is a Windows kernel object handle,
//   represented as a pointer-sized integer. Kernel handles are process-
//   wide and safe to use from any thread. The `HANDLE` type is marked
//   `!Send` in `windows-rs` out of caution because many Win32 APIs expect
//   the handle to remain associated with the original thread, but that is
//   not the case for the logon token here — it is only read and passed to
//   APIs that accept any thread's handle.
#[cfg(windows)]
unsafe impl Send for WindowsSessionUser {}
// SAFETY: `WindowsSessionUser` is Sync because all fields are immutable
// after construction (no interior mutability), so `&WindowsSessionUser`
// can be shared across threads without data races. The `HANDLE` is only
// read through `&self` accessors.
#[cfg(windows)]
unsafe impl Sync for WindowsSessionUser {}

#[cfg(windows)]
impl WindowsSessionUser {
    /// Create a WindowsSessionUser for the current process user (no credentials needed).
    pub fn for_process_user() -> Result<Self, String> {
        let user = crate::win32::get_process_user()
            .map_err(|e| format!("Failed to get process user: {e}"))?;
        Ok(Self {
            user,
            password: None,
            logon_token: None,
        })
    }

    /// Create a WindowsSessionUser with a password (non-Session 0 only).
    ///
    /// Validates the credentials immediately via `LogonUserW`.
    pub fn with_password(user: &str, password: &str) -> Result<Self, BadCredentialsError> {
        if crate::win32::is_session_zero() {
            return Err(BadCredentialsError::Other(
                "Must supply a logon_token rather than a password. \
                 Passwords are not supported when running in Windows Session 0."
                    .into(),
            ));
        }

        if let Ok(proc_user) = crate::win32::get_process_user() {
            if user.eq_ignore_ascii_case(&proc_user) {
                return Err(BadCredentialsError::Other(
                    "User is the process owner. Do not provide a password.".into(),
                ));
            }
        }

        Self::validate_credentials(user, password)?;

        Ok(Self {
            user: user.to_string(),
            password: Some(password.to_string()),
            logon_token: None,
        })
    }

    /// Create a WindowsSessionUser with a pre-existing logon token (Session 0 / services).
    ///
    /// The caller is responsible for the lifetime of the token handle — it must
    /// remain valid for the lifetime of this `WindowsSessionUser`.
    pub fn with_logon_token(
        user: &str,
        token: windows::Win32::Foundation::HANDLE,
    ) -> Result<Self, String> {
        if let Ok(proc_user) = crate::win32::get_process_user() {
            if user.eq_ignore_ascii_case(&proc_user) {
                return Err("User is the process owner. Do not provide a logon token.".into());
            }
        }

        Ok(Self {
            user: user.to_string(),
            password: None,
            logon_token: Some(token),
        })
    }

    /// Get the password, if this user was created with one.
    pub fn password(&self) -> Option<&str> {
        self.password.as_deref()
    }

    /// Get the logon token, if this user was created with one.
    pub fn logon_token(&self) -> Option<windows::Win32::Foundation::HANDLE> {
        self.logon_token
    }

    fn validate_credentials(user: &str, password: &str) -> Result<(), BadCredentialsError> {
        match crate::win32::logon_user(user, password) {
            Ok(_token) => Ok(()), // token dropped here, closing the handle
            Err(e) => {
                // ERROR_LOGON_FAILURE = 0x8007052E
                let code = e.code().0 as u32;
                if code == 0x8007052E {
                    Err(BadCredentialsError::LogonFailure)
                } else {
                    Err(BadCredentialsError::Other(e.to_string()))
                }
            }
        }
    }
}

#[cfg(windows)]
impl SessionUser for WindowsSessionUser {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn user(&self) -> &str {
        &self.user
    }

    fn group(&self) -> &str {
        ""
    }

    fn is_process_user(&self) -> bool {
        crate::win32::get_process_user()
            .map(|proc_user| self.user.eq_ignore_ascii_case(&proc_user))
            .unwrap_or(false)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(all(test, windows))]
mod tests_windows {
    use super::*;

    /// `WindowsSessionUser::with_password` must reject the process user with
    /// `BadCredentialsError::Other` (a structural rejection — credentials are
    /// not validated against `LogonUserW` in this case). Callers that supply
    /// the process user should use `WindowsSessionUser::for_process_user()`
    /// instead.
    ///
    /// This exercises the `Other` branch of the error mapping that the Python
    /// binding layer surfaces as `RuntimeError`. Pairs with
    /// `tests/integration/test_cross_user_windows.rs::test_with_password_logon_failure_*`
    /// which exercises the `LogonFailure` branch.
    #[test]
    fn with_password_process_owner_returns_other_variant() {
        // GIVEN the user name of the current process
        let proc_user = match crate::win32::get_process_user() {
            Ok(u) => u,
            // If we can't determine the process user we cannot drive this
            // test — but that's a separate failure mode; bail out cleanly so
            // CI doesn't false-fail on unrelated configuration issues.
            Err(_) => return,
        };

        // WHEN with_password is called for that user
        let result = WindowsSessionUser::with_password(&proc_user, "irrelevant");

        // THEN we get Other, not LogonFailure — and the message clearly
        // names the reason so callers can route it to a useful error path.
        match result {
            Err(BadCredentialsError::Other(msg)) => {
                assert!(
                    msg.contains("process owner"),
                    "expected 'process owner' in message, got: {msg}",
                );
            }
            Err(BadCredentialsError::LogonFailure) => {
                panic!(
                    "process-owner rejection should be Other, not LogonFailure. \
                     The `Other` variant carries the structural-rejection message \
                     that callers depend on to distinguish this case from a real \
                     credential mismatch."
                );
            }
            Ok(_) => panic!("with_password(process_user, ...) must reject"),
        }
    }
}
