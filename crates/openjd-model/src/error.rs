// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Error types for the OpenJD model library.

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum OpenJdError {
    /// Structural deserialization failure (bad YAML/JSON, missing fields, wrong types).
    #[error("Validation error: {0}")]
    DecodeValidation(String),

    /// Semantic validation failure (template parsed but violates spec rules).
    #[error("Model validation error: {0}")]
    ModelValidation(String),

    /// Format string interpolation error with optional position info.
    #[error("{}", format_string_error(.message, .input, .start, .end))]
    FormatStringError {
        message: String,
        /// The raw format string that failed, if available.
        input: Option<String>,
        /// Byte offset of the failing interpolation start.
        start: Option<usize>,
        /// Byte offset of the failing interpolation end.
        end: Option<usize>,
    },

    #[error("Expression error: {0}")]
    Expression(String),

    #[error("Compatibility error: {0}")]
    Compatibility(String),

    #[error("Unsupported schema version: {0}")]
    UnsupportedSchema(String),
}

fn format_string_error(
    message: &str,
    input: &Option<String>,
    start: &Option<usize>,
    end: &Option<usize>,
) -> String {
    match (input, start, end) {
        (Some(input), Some(s), Some(e)) => {
            format!("Failed to parse interpolation expression at [{s}, {e}]. {message}\n  {input}")
        }
        _ => format!("Format string error: {message}"),
    }
}

impl From<openjd_expr::SymbolTableError> for OpenJdError {
    fn from(e: openjd_expr::SymbolTableError) -> Self {
        OpenJdError::Expression(e.to_string())
    }
}

impl From<openjd_expr::FormatStringValidationError> for OpenJdError {
    fn from(e: openjd_expr::FormatStringValidationError) -> Self {
        OpenJdError::FormatStringError {
            message: e.message,
            input: Some(e.input),
            start: Some(e.start),
            end: Some(e.end),
        }
    }
}

/// An element in a validation error path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathElement {
    Field(String),
    Index(usize),
}

/// A single validation error with its location in the template.
#[derive(Debug, Clone)]
pub struct ValidationError {
    pub path: Vec<PathElement>,
    pub message: String,
}

/// Collects multiple validation errors with structured paths.
#[derive(Debug, Default)]
pub struct ValidationErrors {
    pub errors: Vec<ValidationError>,
}

impl ValidationErrors {
    /// Add an error at the given path.
    pub fn add(&mut self, path: &[PathElement], msg: impl Into<String>) {
        self.errors.push(ValidationError {
            path: path.to_vec(),
            message: msg.into(),
        });
    }

    pub fn is_empty(&self) -> bool {
        self.errors.is_empty()
    }

    pub fn len(&self) -> usize {
        self.errors.len()
    }

    pub fn into_result(self, model_name: &str) -> Result<(), OpenJdError> {
        if self.errors.is_empty() {
            Ok(())
        } else {
            Err(OpenJdError::ModelValidation(self.format(model_name)))
        }
    }

    /// Format errors matching the Python Pydantic output format.
    pub fn format(&self, model_name: &str) -> String {
        let n = self.errors.len();
        let word = if n == 1 { "error" } else { "errors" };
        let mut out = format!("{n} validation {word} for {model_name}");
        for err in &self.errors {
            out.push('\n');
            if err.path.is_empty() {
                // Root-level error
                out.push_str(&format!("{model_name}: {}", err.message));
            } else {
                format_path(&err.path, &mut out);
                out.push_str(":\n\t");
                out.push_str(&err.message);
            }
        }
        out
    }
}

/// Format a path as `field[index] -> nested -> leaf`.
fn format_path(path: &[PathElement], out: &mut String) {
    let mut first = true;
    for elem in path {
        match elem {
            PathElement::Field(name) => {
                if !first {
                    out.push_str(" -> ");
                }
                out.push_str(name);
                first = false;
            }
            PathElement::Index(i) => {
                out.push_str(&format!("[{i}]"));
            }
        }
    }
}

/// Helper: extend a path with a field name.
#[must_use]
pub fn path_field(base: &[PathElement], field: &str) -> Vec<PathElement> {
    let mut p = base.to_vec();
    p.push(PathElement::Field(field.to_string()));
    p
}

/// Helper: extend a path with an index.
#[must_use]
pub fn path_index(base: &[PathElement], index: usize) -> Vec<PathElement> {
    let mut p = base.to_vec();
    p.push(PathElement::Index(index));
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unsupported_schema_msg() {
        let e = OpenJdError::UnsupportedSchema("version".into());
        assert_eq!(e.to_string(), "Unsupported schema version: version");
    }

    #[test]
    fn test_model_validation_msg() {
        let e = OpenJdError::ModelValidation("bad template".into());
        assert_eq!(e.to_string(), "Model validation error: bad template");
    }

    #[test]
    fn test_format_string_error_with_position() {
        let e = OpenJdError::FormatStringError {
            message: "Undefined variable 'Param.X'".into(),
            input: Some("Hello {{Param.X}}".into()),
            start: Some(6),
            end: Some(17),
        };
        let s = e.to_string();
        assert!(s.contains("Failed to parse interpolation expression at [6, 17]"));
        assert!(s.contains("Undefined variable 'Param.X'"));
        assert!(s.contains("Hello {{Param.X}}"));
    }

    #[test]
    fn test_format_string_error_without_position() {
        let e = OpenJdError::FormatStringError {
            message: "something went wrong".into(),
            input: None,
            start: None,
            end: None,
        };
        assert_eq!(e.to_string(), "Format string error: something went wrong");
    }

    #[test]
    fn test_empty_errors_ok() {
        let ve = ValidationErrors::default();
        assert!(ve.into_result("JobTemplate").is_ok());
    }

    #[test]
    fn test_single_field_error() {
        let mut ve = ValidationErrors::default();
        ve.add(&[PathElement::Field("name".into())], "must not be empty");
        let s = ve.format("JobTemplate");
        assert_eq!(
            s,
            "1 validation error for JobTemplate\nname:\n\tmust not be empty"
        );
    }

    #[test]
    fn test_into_result_uses_model_validation() {
        let mut ve = ValidationErrors::default();
        ve.add(&[PathElement::Field("name".into())], "too long");
        let result = ve.into_result("JobTemplate");
        assert!(matches!(result, Err(OpenJdError::ModelValidation(_))));
    }

    #[test]
    fn test_nested_path_error() {
        let mut ve = ValidationErrors::default();
        ve.add(
            &[
                PathElement::Field("steps".into()),
                PathElement::Index(0),
                PathElement::Field("parameterSpace".into()),
                PathElement::Field("combination".into()),
            ],
            "missing operator",
        );
        let s = ve.format("JobTemplate");
        assert!(s.contains("steps[0] -> parameterSpace -> combination:\n\tmissing operator"));
    }

    #[test]
    fn test_root_level_error() {
        let mut ve = ValidationErrors::default();
        ve.add(&[], "must have at least one step");
        let s = ve.format("JobTemplate");
        assert!(s.contains("JobTemplate: must have at least one step"));
    }

    #[test]
    fn test_multiple_errors() {
        let mut ve = ValidationErrors::default();
        ve.add(&[PathElement::Field("name".into())], "too long");
        ve.add(
            &[
                PathElement::Field("steps".into()),
                PathElement::Index(0),
                PathElement::Field("name".into()),
            ],
            "empty",
        );
        assert_eq!(ve.len(), 2);
        let result = ve.into_result("JobTemplate");
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("2 validation errors"));
        assert!(msg.contains("name:\n\ttoo long"));
        assert!(msg.contains("steps[0] -> name:\n\tempty"));
    }

    #[test]
    fn test_path_helpers() {
        let base = vec![PathElement::Field("steps".into()), PathElement::Index(0)];
        let with_field = path_field(&base, "script");
        assert_eq!(with_field.len(), 3);
        let with_index = path_index(&base, 1);
        assert_eq!(with_index.len(), 3);
    }
}
