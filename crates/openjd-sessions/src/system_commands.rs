// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.

//! Resolution of system command names to absolute paths, without consulting `PATH`.
//!
//! This addresses CWE-426 (Untrusted Search Path). Be precise about the exposure
//! in *this* implementation, because an earlier revision of this comment was not:
//!
//! In the Python reference implementation the hole is directly exploitable. There,
//! the job's environment is merged over the parent's and handed to the `Popen`
//! call that launches `sudo`, so a bare `sudo` is resolved through a `PATH` the job
//! wrote, and a job that drops an executable named `sudo` early on `PATH` gets it
//! run at the session's privilege level. That is the reported vulnerability.
//!
//! **This crate is not in that position today.** `CrossUserHelper::spawn` sets no
//! environment on its `Command`, so the child inherits the session process's own
//! environment, and `Command::new` resolves the program against the *parent's*
//! `PATH`. The job's environment reaches only the job's own command, in
//! `subprocess.rs`, and reaches it after `env_clear()`. So no job-supplied variable
//! influences how `sudo` is located.
//!
//! What remains is a latent hazard rather than a live exploit, which is still worth
//! closing:
//!
//! * The safety of the bare name rests on an invariant stated nowhere and enforced
//!   by nothing -- that no caller ever sets an environment on the helper's
//!   `Command`. Adding `.env()` or `.envs()` there, for any reason, would silently
//!   make it the Python bug.
//! * It presumes the session process's own `PATH` is trustworthy. That is an
//!   assumption about how the agent is launched, not a property of this crate.
//! * Parity with the reference implementation: the same audit should reach the same
//!   conclusion in both, without a reader having to re-derive the difference.
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
    // Paired with the entry above. /run/wrappers/bin holds only the setuid/setcap
    // wrappers, so on NixOS it resolves `sudo` and nothing else: /usr/bin holds just
    // `env`, /bin just `sh`, and the sbin directories are absent. `rm` lives in this
    // symlink farm, which nixos-rebuild manages and root owns, making it
    // trust-equivalent to /usr/bin there. Without it the ordering above would
    // resolve `sudo` and then skip the cross-user cleanup for want of `rm`.
    "/run/current-system/sw/bin",
    "/usr/bin",
    "/bin",
    // FreeBSD and the other BSDs install sudo from ports into /usr/local/bin and
    // have no /usr/bin/sudo at all, so omitting this made cross-user sessions
    // impossible to start there -- a regression from the bare `Command::new("sudo")`
    // this module replaced, which the login PATH would have resolved.
    "/usr/local/bin",
    "/usr/local/sbin",
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

    /// This module's own source, minus its tests, for [`never_reads_the_environment`].
    fn production_source() -> &'static str {
        let source = include_str!("system_commands.rs");
        // Split off the test module, whose own assertions mention the very tokens
        // being searched for. Without this the test fails on itself.
        source
            .split_once("#[cfg(test)]")
            .map(|(production, _tests)| production)
            .expect("this file contains a #[cfg(test)] module")
    }

    #[test]
    fn never_reads_the_environment() {
        // Pins "PATH is never read", which is the property the whole module exists
        // for and the one a `which`-style rewrite would silently undo.
        //
        // Asserted against the source rather than by setting PATH and observing the
        // result. An earlier revision did the latter, and it was wrong twice over:
        // `std::env::set_var` mutates process-global state while other tests in this
        // same binary call `std::env::vars()` concurrently (`subprocess.rs`), which
        // is a data race -- the reason the function is `unsafe` from edition 2024 --
        // and it could flake unrelated tests that spawn bare commands.
        //
        // This form is deterministic, needs no isolation, and still fails on the
        // realistic mutation: a PATH fallback has to read the environment to work.
        let production = production_source();

        assert!(
            !production.contains("std::env"),
            "system_commands must not read process environment state; PATH resolution \
             is exactly what this module exists to avoid"
        );
        assert!(
            !production.contains("var_os") && !production.contains("env::var"),
            "environment lookup found in production source"
        );
    }

    #[test]
    fn a_command_present_only_outside_the_trusted_list_is_not_found() {
        // The behavioural companion to the assertion above: a real executable that
        // exists, is executable, and is simply not in a searched directory must not
        // resolve by any route.
        let dir = dir_with_executable("openjd-test-path-cmd");
        assert!(
            dir.path().join("openjd-test-path-cmd").exists(),
            "precondition: the executable really exists"
        );

        assert_eq!(find_system_command("openjd-test-path-cmd"), None);
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
    fn the_two_nixos_directories_are_present_as_a_pair() {
        // /run/wrappers/bin alone supports no complete code path: it holds only the
        // setuid wrappers, so on NixOS it resolves `sudo` and nothing else. `rm`
        // lives in the sw/bin symlink farm, so without that entry the ordering
        // resolves sudo and then skips the cleanup for want of rm.
        assert!(TRUSTED_SYSTEM_DIRECTORIES.contains(&"/run/wrappers/bin"));
        assert!(TRUSTED_SYSTEM_DIRECTORIES.contains(&"/run/current-system/sw/bin"));
    }

    #[test]
    fn bsd_local_directories_are_searched() {
        // FreeBSD and the other BSDs install sudo from ports into /usr/local/bin and
        // have no /usr/bin/sudo, so omitting this made cross-user sessions
        // impossible to start there -- a regression from the bare name this module
        // replaced, which the login PATH resolved.
        assert!(TRUSTED_SYSTEM_DIRECTORIES.contains(&"/usr/local/bin"));
        assert!(TRUSTED_SYSTEM_DIRECTORIES.contains(&"/usr/local/sbin"));
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
