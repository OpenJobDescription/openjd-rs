# Cross-User Testing Infrastructure

## Overview

Cross-user tests validate that the sessions crate correctly executes subprocesses as a
different OS user via the embedded cross-user helper, delivers signals across user
boundaries, and manages file ownership. These tests cannot run in a normal CI
environment because they require multiple OS users, passwordless sudo, and optionally
LDAP-based user management.

The infrastructure uses Docker containers to create isolated environments with the
required user/group/sudo configuration. Tests are gated by `#[ignore]` and only run
inside these containers via `--include-ignored`. On macOS (which has no Docker in CI),
the same tests run directly on the runner with users provisioned through Directory
Services — see [macOS](#macos) below.

This design was ported from the Python `openjd-sessions-for-python` library's Docker
test infrastructure.

## Docker Environments

Two Docker environments run the same test suite — the difference is how users are
provisioned:

| Environment | Directory | User provisioning | Purpose |
|---|---|---|---|
| localuser | `testing_containers/localuser_sudo_environment/` | `/etc/passwd` (local) | Primary cross-user testing |
| LDAP | `testing_containers/ldap_sudo_environment/` | OpenLDAP via `nslcd`/`nscd` | Validates NSS/PAM integration |

The LDAP variant ensures that `nix::unistd::User::from_name()`,
`nix::unistd::Group::from_name()`, and related calls work correctly when user resolution
goes through LDAP rather than local files. This catches bugs where code assumes
`/etc/passwd` is the source of truth.

### User Setup

Both environments create three users:

| User | Groups | Role |
|---|---|---|
| `hostuser` | `hostuser`, `sharedgroup` | Runs the test suite |
| `targetuser` | `targetuser`, `sharedgroup` | Cross-user subprocess target |
| `disjointuser` | `disjointuser`, `disjointgroup` | Tests group permission failures |

Sudoers rule: `hostuser ALL=(targetuser,hostuser) NOPASSWD: ALL`

The `sharedgroup` membership is critical — it allows `hostuser` to create directories
that `targetuser` can access (mode 0o770 with shared group ownership). The `disjointuser`
has no shared group with `hostuser`, which tests that TempDir creation correctly fails
when the session user can't share a group.

### Localuser Dockerfile

Based on `rust:1-bookworm`. Installs `psmisc` and `sudo`. Creates users via `useradd`.
Copies the workspace and builds tests as `hostuser`. The default CMD runs all tests
including ignored ones.

### LDAP Dockerfile

Same base image. Additionally installs and configures `slapd` (OpenLDAP server),
`libnss-ldapd`, and `libpam-ldapd`. Users and groups are provisioned via LDIF files:

| File | Purpose |
|---|---|
| `addUsersGroups.ldif` | Creates LDAP entries for all three users and five groups |
| `addUsersToSharedGroup.ldif` | Adds `hostuser` and `targetuser` to `sharedgroup` |
| `changePassword.ldif` | Sets the LDAP admin password |
| `start_ldap.sh` | Starts `slapd`, `nscd`, and `nslcd` services |

The CMD starts LDAP services, chowns the workspace to `hostuser`, then runs tests via
`sudo -u hostuser` with environment variables preserved.

## Test Gating

### Environment Variables

| Variable | Value in Docker | Purpose |
|---|---|---|
| `OPENJD_TEST_SUDO_TARGET_USER` | `targetuser` | Enables cross-user tests |
| `OPENJD_TEST_SUDO_SHARED_GROUP` | `sharedgroup` | Shared group for file permissions |
| `OPENJD_TEST_SUDO_DISJOINT_USER` | `disjointuser` | Tests permission denial |
| `OPENJD_TEST_SUDO_DISJOINT_GROUP` | `disjointgroup` | Tests group mismatch |

### Helper Functions

```rust
fn target_user() -> Option<Arc<PosixSessionUser>> {
    let user = std::env::var("OPENJD_TEST_SUDO_TARGET_USER").ok()?;
    let group = std::env::var("OPENJD_TEST_SUDO_SHARED_GROUP").ok()?;
    Some(Arc::new(PosixSessionUser::new(&user, Some(&group))))
}

fn require_target_user() -> Arc<PosixSessionUser> {
    target_user().expect(
        "OPENJD_TEST_SUDO_TARGET_USER and OPENJD_TEST_SUDO_SHARED_GROUP must be set",
    )
}
```

Each test is `#[ignore]` and calls `require_target_user()` or `require_disjoint_user()`
as its first line. Outside Docker, `cargo test` skips them. Inside Docker,
`cargo test -- --include-ignored` runs them.

## Test Inventory

### Subprocess Tests (4)

| Test | Validates |
|---|---|
| `test_cross_user_subprocess_basic` | `whoami` as target user returns target username |
| `test_cross_user_subprocess_notify` | SIGTERM reaches cross-user process (trap fires, "Trapped" in output) |
| `test_cross_user_subprocess_terminate` | SIGKILL kills process (trap does NOT fire) |
| `test_cross_user_subprocess_terminate_tree` | SIGKILL kills parent + all children in process tree |

These use shell scripts in `tests/support/`:
- `long_running.sh` — Traps SIGTERM, prints messages every 1s for 20 iterations
- `long_running_ignore.sh` — Same but trap handler doesn't exit (tests SIGKILL escalation)
- `spawn_child.sh` — Spawns `long_running.sh` as child, then prints its own messages

### Runner Identity Tests (3)

| Test | Validates |
|---|---|
| `test_cross_user_runner_uid` | `id -u` output matches target user's UID, not ours |
| `test_cross_user_runner_env_vars` | Explicit env vars are propagated to cross-user subprocess |
| `test_cross_user_no_env_inheritance` | Host process env vars do NOT leak into cross-user subprocess |

### Session Tests (2)

| Test | Validates |
|---|---|
| `test_cross_user_session_cleanup` | Cleanup deletes files owned by target user with 700/600 permissions |
| `test_cross_user_session_run_subprocess` | `Session::run_subprocess` runs as configured target user |

The cleanup test creates files via `sudo -u targetuser` with restrictive permissions
(owner-only), then verifies `session.cleanup()` successfully removes them. This exercises
the two-phase cleanup: `sudo rm -rf` as the session user, then `remove_dir_all` as the
process user.

### TempDir Tests (3)

| Test | Validates |
|---|---|
| `test_cross_user_tempdir_permissions` | Group ownership matches target user's group, mode is 0o770 |
| `test_cross_user_tempdir_cleanup` | Cleanup works when target user has created files inside |
| `test_cross_user_tempdir_disjoint_fails` | TempDir with disjoint user (no shared group) fails or has wrong group |

## macOS

The full POSIX cross-user suite passes on macOS with no code changes — the crate's
mechanism (`process_group(0)` at spawn, `killpg` for signalling, no `setsid(1)` binary,
no `pgrep`, no procfs) is pure POSIX syscalls, all present on macOS. The
`cross-user-macos` CI job runs the suite on `macos-latest`, where the runner has
passwordless sudo, so users are provisioned directly on the host instead of in Docker.

macOS-specific provisioning differences (encoded in the CI job):

| Concern | Linux (Docker) | macOS |
|---|---|---|
| Create users/groups | `useradd`/`groupadd` | `sysadminctl -addUser` / `dseditgroup -o create` |
| Self-named user group | Created automatically by `useradd` | Must be created explicitly (`sysadminctl` assigns primary group `staff`); the tempdir cleanup test chowns to `user:user` |
| Temp root | `/tmp` (world-writable) | The default per-user `TMPDIR` (`/var/folders/<hash>/T`) is mode `0700`, so the target user cannot traverse to the session working directory or execute the extracted helper binary. Tests set a world-traversable `TMPDIR`. |
| Resolve new users | immediate | `dscacheutil -flushcache` after provisioning |

> **Deployment note:** the `TMPDIR` constraint applies to real macOS hosts too — the
> session root must be traversable by the session user (e.g. the Deadline Cloud worker
> agent uses `/var/lib/deadline/sessions` on macOS). A session root under a `0700`
> per-user directory fails with "Helper process closed stdout unexpectedly" because
> `sudo -u <user> -i <helper_path>` cannot reach the helper binary. Whether this bites
> depends on launch context: launchd sets the private per-user `TMPDIR` for per-user
> domain services and login sessions, whereas system `LaunchDaemon`s typically get
> `/tmp`. So the same binary can work as a daemon and fail when run from a terminal —
> which is why the caller should set an explicit session root rather than relying on
> `std::env::temp_dir()`. `TempDir` does not currently pre-check ancestor traversability
> (see the `#[ignore]`d `tempdir_under_untraversable_parent_is_currently_created` test,
> which pins this).

> **Default-group sharp edge (macOS):** `PosixSessionUser::new(user, None)` defaults the
> group to the process's effective group. On macOS an ordinary local account's effective
> group is `staff`, so a cross-user `TempDir`/helper created with a `None` group is
> chmod'd `0o770`/`0o750` with group `staff` — readable/writable by *all* local users.
> This is inherited parity with the Python reference implementation
> (`grp.getgrgid(os.getegid())`), not a Rust-specific defect, but callers on macOS should
> pass an explicit dedicated group.

There is no macOS equivalent of the LDAP variant. The macOS job provisions **local**
Directory Services accounts, which is the same local-node resolution path any ordinary
macOS user takes — it does not exercise a networked directory the way the Linux LDAP
container does. `nix::unistd::User::from_name()` does go through Directory Services
rather than reading `/etc/passwd` directly, but that is not equivalent to
networked-directory (LDAP/Active Directory) coverage.

## Entry Script

`scripts/run_cross_user_tests.sh` orchestrates the full test run:

```
Usage: run_cross_user_tests.sh [--ldap] [--build-only]
```

### Execution Flow

1. Build the Docker image (localuser or LDAP variant)
2. Run all tests with `--include-ignored`

## Support Files

```
crates/openjd-sessions/tests/support/
├── long_running.sh          # 20-iteration loop, traps SIGTERM, exits on trap
├── long_running_ignore.sh   # Same but trap doesn't exit (tests SIGKILL)
└── spawn_child.sh           # Spawns long_running.sh as child process
```

These replace the Python scripts (`app_20s_run.py`, `run_app_20s_run.py`) used in the
Python test suite. Shell scripts were chosen to avoid a Python dependency in the Rust
Docker container.

## Python Parity

| Category | Python | Rust | Status |
|---|---|---|---|
| Subprocess basic | `test_basic_operation` | `test_cross_user_subprocess_basic` | ✅ |
| Subprocess notify | `test_notify_ends_process` | `test_cross_user_subprocess_notify` | ✅ |
| Subprocess terminate | `test_terminate_ends_process` | `test_cross_user_subprocess_terminate` | ✅ |
| Subprocess tree kill | `test_terminate_ends_process_tree` | `test_cross_user_subprocess_terminate_tree` | ✅ |
| Runner UID | `test_run_as_posix_user` | `test_cross_user_runner_uid` | ✅ |
| Runner env vars | `test_run_as_posix_user_with_env_vars` | `test_cross_user_runner_env_vars` | ✅ |
| No env inheritance | `test_does_not_inherit_env_vars_posix` | `test_cross_user_no_env_inheritance` | ✅ |
| Session cleanup | `test_cleanup_posix_user` | `test_cross_user_session_cleanup` | ✅ |
| Session run_subprocess | `test_user_context_posix` | `test_cross_user_session_run_subprocess` | ✅ |
| TempDir permissions | `test_defaults` | `test_cross_user_tempdir_permissions` | ✅ |
| TempDir cleanup | `test_cleanup` | `test_cross_user_tempdir_cleanup` | ✅ |
| TempDir disjoint | `test_cannot_change_to_group` | `test_cross_user_tempdir_disjoint_fails` | ✅ |
| Localuser Docker | ✅ | ✅ | ✅ |
| LDAP Docker | ✅ | ✅ | ✅ |

Windows cross-user tests from the Python library are out of scope — the Rust crate does
not yet have Windows cross-user support.
