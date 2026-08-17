// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.

//! Resolution of system command names to absolute paths, without consulting `PATH`.
//!
//! This module exists because of CWE-426 (Untrusted Search Path). A session runs
//! job-supplied actions, and the job influences the environment those actions run
//! with -- including `PATH`. A bare command name in the argv of one of our *own*
//! privileged helpers (`sudo`) is therefore resolved through a search path the
//! job may control, so a job that drops an executable named `sudo` early on
//! `PATH` gets it run at the session's privilege level.
//!
//! Every such name is resolved here instead, by scanning a fixed list of trusted
//! absolute directories.
//!
//! Three properties are load-bearing, and each is pinned by a test below:
//!
//! * **`PATH` is never read.** Not directly, and not indirectly via a `which`
//!   crate or `command -v`, both of which resolve through `PATH` and so would
//!   reintroduce the vulnerability while appearing to fix it.
//! * **Only paths under [`TRUSTED_SYSTEM_DIRECTORIES`] are returned**, and a name
//!   containing a path separator is rejected -- otherwise joining
//!   `"/usr/bin"` with `"../../tmp/evil"` would make this module the injection
//!   point it exists to remove.
//! * **A missing command is an error, never a fallback to the bare name.** A
//!   silent fallback would restore the vulnerability while looking fixed, which
//!   is the worst available failure mode for this class of fix.

use std::io;
use std::path::{Path, PathBuf};

/// Absolute directories searched for system commands, in order.
///
/// The order is deliberate. On NixOS the setuid `sudo` wrapper lives in
/// `/run/wrappers/bin` and the `/usr/bin` copy is either absent or not setuid, so
/// the wrapper directory must be consulted first. Everywhere else that directory
/// does not exist and costs one `stat`.
///
/// `sbin` entries are last: on non-usr-merged distributions some commands exist
/// only under `/sbin`.
pub(crate) const TRUSTED_SYSTEM_DIRECTORIES: &[&str] = &[
    "/run/wrappers/bin",
    "/usr/bin",
    "/bin",
    "/usr/sbin",
    "/sbin",
];

/// True if `name` is a bare command name, with no path component.
fn is_bare_command_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains('/') && !name.contains('\\')
}

/// True if `path` is a regular file with at least one execute bit set.
fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// Resolve `name` within an explicit list of directories.
///
/// Separated from [`find_system_command`] so that tests can supply a directory
/// they control; production callers should use the wrappers below.
pub(crate) fn find_system_command_in(name: &str, directories: &[&str]) -> Option<PathBuf> {
    if !is_bare_command_name(name) {
        return None;
    }
    directories
        .iter()
        .map(|directory| Path::new(directory).join(name))
        .find(|candidate| is_executable_file(candidate))
}

/// Resolve `name` to an absolute path, or `None` if it is not installed.
///
/// Use this when the command's absence is tolerable; use [`system_command_path`]
/// when it is required.
pub(crate) fn find_system_command(name: &str) -> Option<PathBuf> {
    find_system_command_in(name, TRUSTED_SYSTEM_DIRECTORIES)
}

/// Resolve `name` to an absolute path, or fail.
///
/// Returns [`io::ErrorKind::NotFound`] so that callers can fold this into the
/// same error they already report for a process that could not be started.
pub(crate) fn system_command_path(name: &str) -> io::Result<PathBuf> {
    find_system_command(name).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "could not find the system command {name:?} in any trusted directory ({}); \
                 PATH is deliberately not searched",
                TRUSTED_SYSTEM_DIRECTORIES.join(", ")
            ),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Create a directory containing an executable file named `name`.
    fn dir_with_executable(name: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join(name);
        fs::write(&path, "#!/bin/sh\ntrue\n").expect("write");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod");
        }
        dir
    }

    #[test]
    fn resolves_a_command_that_exists_in_a_searched_directory() {
        // The negative control for the tests below: resolution must actually
        // work, or "nothing was found" results prove nothing.
        let dir = dir_with_executable("openjd-test-cmd");
        let dir_s = dir.path().to_str().expect("utf8");

        let found = find_system_command_in("openjd-test-cmd", &[dir_s]);

        assert_eq!(found, Some(dir.path().join("openjd-test-cmd")));
    }

    #[test]
    fn does_not_resolve_a_command_outside_the_searched_directories() {
        // Pins "only the listed directories are searched". A `which`-style
        // implementation fails this: the command is not on PATH, so it would
        // return None where this must return the path.
        let dir = dir_with_executable("openjd-test-cmd");

        let found = find_system_command_in("openjd-test-cmd", &["/usr/bin", "/bin"]);

        assert_eq!(found, None, "resolved a command from an unlisted directory");
        // ...and the same name IS found once its directory is listed, so the
        // assertion above is about the directory list and not about the file.
        let dir_s = dir.path().to_str().expect("utf8");
        assert!(find_system_command_in("openjd-test-cmd", &[dir_s]).is_some());
    }

    #[test]
    fn ignores_path_even_when_it_contains_a_matching_command() {
        // Pins "PATH is never read". Without this, adding a PATH fallback for
        // commands missing from the trusted list would go unnoticed.
        //
        // PATH is process-global, so this test saves and restores it. It is the
        // only test here that touches the environment.
        let dir = dir_with_executable("openjd-test-path-cmd");
        let original = std::env::var_os("PATH");

        std::env::set_var("PATH", dir.path());
        let found = find_system_command("openjd-test-path-cmd");
        match original {
            Some(value) => std::env::set_var("PATH", value),
            None => std::env::remove_var("PATH"),
        }

        assert_eq!(found, None, "PATH was consulted");
    }

    #[test]
    fn rejects_names_containing_a_path_component() {
        // Pins the guard that stops this module becoming the injection point:
        // joining a trusted directory with "../.." escapes it entirely.
        let dir = dir_with_executable("openjd-test-cmd");
        let parent = dir.path().to_str().expect("utf8");

        for name in ["../openjd-test-cmd", "a/b", "a\\b", "", ".", ".."] {
            assert_eq!(
                find_system_command_in(name, &[parent]),
                None,
                "accepted {name:?} as a bare command name"
            );
        }
    }

    #[test]
    fn escaping_name_is_rejected_even_though_the_target_exists() {
        // The traversal guard has to be about the name, not about whether the
        // resolved file happens to exist -- so prove the target is reachable by
        // the same join before asserting the name is refused.
        let dir = dir_with_executable("openjd-test-cmd");
        let nested = dir.path().join("nested");
        fs::create_dir(&nested).expect("mkdir");
        let nested_s = nested.to_str().expect("utf8");

        assert!(
            dir.path().join("openjd-test-cmd").exists(),
            "precondition: the traversal target exists"
        );
        assert_eq!(
            find_system_command_in("../openjd-test-cmd", &[nested_s]),
            None
        );
    }

    #[test]
    fn ignores_a_non_executable_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        fs::write(dir.path().join("openjd-test-cmd"), "not executable").expect("write");
        let dir_s = dir.path().to_str().expect("utf8");

        assert_eq!(find_system_command_in("openjd-test-cmd", &[dir_s]), None);
    }

    #[test]
    fn missing_command_is_an_error_and_not_the_bare_name() {
        // The silent-fallback failure mode: returning Ok("sudo") here would look
        // fixed and behave exactly as the vulnerability did.
        let error = system_command_path("openjd-definitely-not-installed")
            .expect_err("missing command must not resolve");

        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        let message = error.to_string();
        assert!(
            message.contains("openjd-definitely-not-installed"),
            "message did not name the command: {message}"
        );
        assert!(
            message.contains("PATH is deliberately not searched"),
            "message did not explain why PATH was not used: {message}"
        );
    }

    #[test]
    fn every_trusted_directory_is_absolute() {
        // A relative entry would be resolved against the process working
        // directory, which a session changes.
        for directory in TRUSTED_SYSTEM_DIRECTORIES {
            assert!(
                Path::new(directory).is_absolute(),
                "{directory} is not absolute"
            );
        }
    }

    #[test]
    fn setuid_wrapper_directory_precedes_usr_bin() {
        // On NixOS /usr/bin/sudo is absent or not setuid; the wrapper must win.
        let wrapper = TRUSTED_SYSTEM_DIRECTORIES
            .iter()
            .position(|d| *d == "/run/wrappers/bin")
            .expect("/run/wrappers/bin is searched");
        let usr_bin = TRUSTED_SYSTEM_DIRECTORIES
            .iter()
            .position(|d| *d == "/usr/bin")
            .expect("/usr/bin is searched");

        assert!(
            wrapper < usr_bin,
            "wrapper directory must be searched first"
        );
    }
}
