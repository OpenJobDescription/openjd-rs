// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.

//! Resolution of system command names to absolute paths, without consulting `PATH`.
//!
//! The problem: `Command::new("sudo")` resolves a bare name through a `PATH`,
//! which makes correctness depend on whose `PATH` that is. Be precise about the
//! answer here, because it differs from the Python implementation.
//!
//! In `openjd-sessions-for-python`, the job's environment is merged over the
//! parent's and handed to the `Popen` call that launches `sudo`. A bare name there
//! resolves through a `PATH` the job wrote, so a job that supplies its own `sudo`
//! has it run at the session's privilege level.
//!
//! This crate is not in that position, for two reasons that are worth separating
//! because only the first is about the environment. `CrossUserHelper::spawn` sets
//! no environment on its `Command`, so the helper inherits the session process's
//! own and `Command::new` resolves against the parent's `PATH`. And `sudo` is
//! spawned once when the helper starts, before any job action exists; per-action
//! environments travel to the helper over its stdin protocol afterwards, so they
//! cannot reach the argv that located `sudo`.
//!
//! Note it is *not* true that job environments are only ever applied after an
//! `env_clear()`. That holds on the same-user path (`subprocess.rs`), but the
//! helper's own runner layers the action's environment onto whatever it inherited,
//! with no `env_clear()` anywhere under `src/helper/`. The conclusion above stands
//! on ordering and channel, not on clearing.
//!
//! Resolving here is therefore about removing assumptions rather than closing a
//! reachable hole:
//!
//! * The bare name is safe only while no caller sets an environment on the helper's
//!   `Command`. Nothing states or enforces that, so adding `.env()` or `.envs()`
//!   later would change the security of a line nobody edited.
//! * It assumes the session process's own `PATH` is trustworthy, which is a
//!   property of how the agent is launched, not of this crate.
//! * It keeps the two implementations answering the same question the same way, so
//!   a reader does not have to derive the difference to audit either one.
//!
//! The solution: names are resolved here, by scanning a fixed list of trusted
//! absolute directories.
//!
//! Three properties make that work, and all three are easy to undo by accident:
//!
//! * `PATH` is never read. Not directly, and not through a `which` crate or
//!   `command -v`, which resolve via `PATH` and so would restore the original
//!   behaviour while looking like a fix.
//! * Only paths under [`TRUSTED_SYSTEM_DIRECTORIES`] are returned. A name
//!   containing a path separator is rejected, because joining `"/usr/bin"` with
//!   `"../../tmp/evil"` would otherwise escape the directory being searched.
//! * A missing command is an error, never a fallback to the bare name. Falling
//!   back would put resolution on `PATH` again while the code still read as though
//!   it did not.

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
    fn production_source_contains_no_environment_lookup() {
        // A source lint, not a behavioural pin, and worth being honest about which.
        //
        // It catches the mutation that matters in practice -- adding a PATH fallback
        // for names missing from the trusted list -- because such a fallback has to
        // read the environment to work, and the behavioural test below cannot see it
        // (a command absent from the trusted directories is also absent from PATH in
        // the test, so both return None either way).
        //
        // What it does not catch: a `which` crate, or `Command::new(bare_name)`
        // letting execvp resolve in the child. Neither reads the environment in this
        // process. Those are visible in review as a new dependency or a changed
        // spawn, which is the level they belong at.
        //
        // Asserted over the source because the obvious alternative is worse. An
        // earlier revision set PATH and observed the result, which mutates
        // process-global state while other tests in this binary call
        // `std::env::vars()` concurrently (`subprocess.rs`). That is a data race,
        // the reason the function is `unsafe` from edition 2024, and it risked
        // flaking unrelated tests that spawn bare commands.
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
