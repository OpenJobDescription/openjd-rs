// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Tests for embedded files — mirrors Python test_embedded_files.py

use openjd_sessions::embedded_files::*;
use std::fs;
use tempfile::TempDir;

// === TestGetSymtabEntry ===

#[test]
fn test_symtab_key_step_scope() {
    assert_eq!(symtab_key(EmbeddedFilesScope::Step, "Foo"), "Task.File.Foo");
}

#[test]
fn test_symtab_key_env_scope() {
    assert_eq!(symtab_key(EmbeddedFilesScope::Env, "Foo"), "Env.File.Foo");
}

// === TestMaterializeFilePosix ===

#[test]
fn test_writes_file() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("testfile.txt");
    let data = "some text data";

    write_embedded_file_with_options(&path, data, false, None).unwrap();

    assert!(path.exists());
    assert_eq!(fs::read_to_string(&path).unwrap(), data);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "Owner has r/w only");
    }
}

/// Mirrors Python TestMaterializeFilePosix::test_writes_file uid/gid assertions.
#[cfg(unix)]
#[test]
fn test_writes_file_posix_ownership() {
    use std::os::unix::fs::MetadataExt;
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("testfile.txt");
    write_embedded_file_with_options(&path, "data", false, None).unwrap();
    let meta = fs::metadata(&path).unwrap();
    assert_eq!(
        meta.uid(),
        nix::unistd::geteuid().as_raw(),
        "File owner is this process's owner"
    );
    assert_eq!(
        meta.gid(),
        nix::unistd::getegid().as_raw(),
        "File group is this process's group"
    );
}

/// Mirrors Python TestMaterializeFilePosix::test_writes_file — full permission check.
#[cfg(unix)]
#[test]
fn test_writes_file_posix_no_group_or_other_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("testfile.txt");
    write_embedded_file_with_options(&path, "data", false, None).unwrap();
    let mode = fs::metadata(&path).unwrap().permissions().mode();
    assert_eq!(mode & 0o070, 0, "Group has no permissions");
    assert_eq!(mode & 0o007, 0, "Others have no permissions");
}

/// Mirrors Python TestMaterializeFilePosix::test_writes_file_runnable — full permission check.
#[cfg(unix)]
#[test]
fn test_writes_file_runnable_posix_ownership_and_permissions() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("testfile.sh");
    write_embedded_file_with_options(&path, "#!/bin/bash", true, None).unwrap();
    let meta = fs::metadata(&path).unwrap();
    assert_eq!(
        meta.uid(),
        nix::unistd::geteuid().as_raw(),
        "File owner is this process's owner"
    );
    assert_eq!(
        meta.gid(),
        nix::unistd::getegid().as_raw(),
        "File group is this process's group"
    );
    let mode = meta.permissions().mode();
    assert_eq!(mode & 0o700, 0o700, "Owner has r/w/x");
    assert_eq!(mode & 0o070, 0, "Group has no permissions");
    assert_eq!(mode & 0o007, 0, "Others have no permissions");
}

#[test]
fn test_truncates_file() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("testfile.txt");
    fs::write(
        &path,
        "This needs to be longer than our test data to test truncation",
    )
    .unwrap();

    let data = "some text data";
    write_embedded_file_with_options(&path, data, false, None).unwrap();

    assert_eq!(fs::read_to_string(&path).unwrap(), data);
}

#[test]
fn test_writes_file_runnable() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("testfile.sh");
    let data = "#!/bin/bash\necho hello";

    write_embedded_file_with_options(&path, data, true, None).unwrap();

    assert!(path.exists());
    assert_eq!(fs::read_to_string(&path).unwrap(), data);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700, "Owner has r/w/x");
    }
}

// === TestEndOfLine ===

#[test]
fn test_end_of_line_lf_only() {
    let result = convert_line_endings("line1\nline2\nline3", EndOfLine::Lf);
    assert_eq!(result, b"line1\nline2\nline3");
}

#[test]
fn test_end_of_line_lf_converts_crlf() {
    let result = convert_line_endings("line1\r\nline2\r\nline3", EndOfLine::Lf);
    assert_eq!(result, b"line1\nline2\nline3");
}

#[test]
fn test_end_of_line_crlf_converts_lf() {
    let result = convert_line_endings("line1\nline2\nline3", EndOfLine::Crlf);
    assert_eq!(result, b"line1\r\nline2\r\nline3");
}

#[test]
fn test_end_of_line_crlf_preserves_crlf() {
    let result = convert_line_endings("line1\r\nline2\r\nline3", EndOfLine::Crlf);
    assert_eq!(result, b"line1\r\nline2\r\nline3");
}

#[cfg(not(windows))]
#[test]
fn test_auto_uses_lf_on_posix() {
    let result = convert_line_endings("line1\r\nline2", EndOfLine::Auto);
    assert_eq!(result, b"line1\nline2");
}

#[test]
fn test_end_of_line_written_to_file() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("eol_test.txt");

    write_embedded_file_with_options(&path, "line1\r\nline2", false, Some(EndOfLine::Lf)).unwrap();

    let bytes = fs::read(&path).unwrap();
    assert_eq!(bytes, b"line1\nline2");
}

// === TestMaterialize::test_basic ===

#[test]
fn test_materialize_basic() {
    let tmp = TempDir::new().unwrap();

    struct FileSpec {
        name: &'static str,
        data: &'static str,
        runnable: bool,
    }

    let files = vec![
        FileSpec {
            name: "Foo",
            data: "foo's data",
            runnable: false,
        },
        FileSpec {
            name: "Bar",
            data: "bar's data",
            runnable: true,
        },
        FileSpec {
            name: "Baz",
            data: "baz's data",
            runnable: false,
        },
    ];

    for f in &files {
        let path = tmp.path().join(format!("{}.txt", f.name.to_lowercase()));
        write_embedded_file_with_options(&path, f.data, f.runnable, None).unwrap();

        let key = symtab_key(EmbeddedFilesScope::Env, f.name);
        assert!(key.starts_with("Env.File."));

        assert!(path.exists());
        assert_eq!(fs::read_to_string(&path).unwrap(), f.data);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).unwrap().permissions().mode();
            if f.runnable {
                assert_eq!(mode & 0o777, 0o700);
            } else {
                assert_eq!(mode & 0o777, 0o600);
            }
        }
    }
}

// === TestMaterialize::test_resolves_symbols ===
// In Rust, format string resolution is handled by the session layer.
// This test verifies that write_embedded_file correctly writes pre-resolved data.

#[test]
fn test_materialize_resolved_data() {
    let tmp = TempDir::new().unwrap();

    let foo_path = tmp.path().join("foo.txt");
    let bar_path = tmp.path().join("bar.txt");

    // Simulate resolved data (format strings already evaluated)
    let resolved = format!("Symbol\n{}\n{}", foo_path.display(), bar_path.display());

    write_embedded_file(&foo_path, &resolved).unwrap();
    write_embedded_file(&bar_path, &resolved).unwrap();

    assert_eq!(fs::read_to_string(&foo_path).unwrap(), resolved);
    assert_eq!(fs::read_to_string(&bar_path).unwrap(), resolved);
}

// === Path traversal defense-in-depth (CWE-22) ===
//
// Per spec (§6.1.1), embedded file `filename` must be a plain basename and
// the `openjd-model` crate rejects path separators at template validation
// time. These tests exercise the sessions-layer check that re-validates
// the filename before it is joined to the target directory. This check
// catches traversal strings that could reach the session layer via any
// bypass of model validation, including implementation-level format-string
// substitution (the current model stores `filename` as a `FormatString`).

mod path_traversal {
    use super::*;
    use openjd_expr::format_string::FormatString;
    use openjd_expr::ExprValue;
    use openjd_model::job::EmbeddedFile;
    use openjd_model::symbol_table::SymbolTable;
    use openjd_model::types::FileType;
    use openjd_sessions::SessionError;

    /// Build an `EmbeddedFile` with a `filename` template that references
    /// `Param.Evil` — the tainted value is supplied via the symbol table so
    /// the traversal string bypasses any raw-template validation.
    fn file_with_tainted_filename(name: &str) -> EmbeddedFile {
        EmbeddedFile {
            name: name.to_string(),
            file_type: FileType::Text,
            filename: Some(FormatString::new("{{Param.Evil}}").unwrap()),
            data: Some(FormatString::new("echo hello").unwrap()),
            runnable: None,
            end_of_line: None,
        }
    }

    fn symtab_with_evil(value: &str) -> SymbolTable {
        let mut st = SymbolTable::new();
        st.set("Param.Evil", ExprValue::String(value.to_string()))
            .unwrap();
        st
    }

    fn allocate_with_tainted(value: &str) -> Result<(), SessionError> {
        let tmp = TempDir::new().unwrap();
        let mut ef = EmbeddedFiles::new(
            EmbeddedFilesScope::Step,
            tmp.path().to_path_buf(),
            "test-session",
        );
        let mut st = symtab_with_evil(value);
        let file = file_with_tainted_filename("evil");
        ef.allocate_file_paths(&[file], &mut st)
    }

    fn assert_rejects(tainted_value: &str, expected_reason_fragment: &str) {
        let err = allocate_with_tainted(tainted_value).expect_err(&format!(
            "expected rejection for tainted filename {tainted_value:?}"
        ));
        let msg = err.to_string();
        let expected_full = format!(
            "Embedded file 'evil' has unsafe filename '{tainted_value}': {expected_reason_fragment}"
        );
        assert_eq!(
            msg, expected_full,
            "error message mismatch for tainted value {tainted_value:?}"
        );
    }

    #[test]
    fn rejects_parent_dir_traversal() {
        assert_rejects("../evil.sh", "must not contain path separators");
    }

    #[test]
    fn rejects_absolute_posix_path() {
        assert_rejects("/etc/passwd", "must not contain path separators");
    }

    #[test]
    fn rejects_backslash_separator() {
        // Backslashes are rejected on all platforms — embedded file names
        // are single path components by spec.
        assert_rejects("..\\evil.sh", "must not contain path separators");
    }

    #[test]
    fn rejects_windows_absolute_path() {
        assert_rejects("C:\\Windows\\evil.exe", "must not contain path separators");
    }

    #[test]
    fn rejects_nested_subdirectory() {
        assert_rejects("sub/evil.sh", "must not contain path separators");
    }

    #[test]
    fn rejects_empty_filename() {
        assert_rejects("", "must not be empty");
    }

    #[test]
    fn rejects_dot() {
        assert_rejects(".", "must not be '.'");
    }

    #[test]
    fn rejects_double_dot() {
        assert_rejects("..", "must not be '..'");
    }

    #[test]
    fn rejects_null_byte() {
        assert_rejects("evil\0.sh", "must not contain null bytes");
    }

    #[test]
    fn accepts_safe_single_component_filename() {
        // Sanity check: a safe resolved filename continues to work.
        allocate_with_tainted("safe.sh").expect("safe filename should be accepted");
    }

    #[test]
    fn accepts_filename_with_dots_in_middle() {
        // Dots in the middle of a filename are not path components.
        allocate_with_tainted("my.file.sh").expect("dotted filename should be accepted");
    }
}
