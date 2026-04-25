// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Error types for the OpenJD model library.
//!
//! The primary error type is [`ModelError`], which covers all failure modes
//! during template parsing, validation, and job creation.

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ModelError {
    /// Structural deserialization failure (bad YAML/JSON, missing fields, wrong types).
    #[error("Validation error: {0}")]
    DecodeValidation(String),

    /// Semantic validation failure (template parsed but violates spec rules).
    /// Contains structured [`ValidationErrors`] with per-field paths.
    /// Use `Display` / `.to_string()` for the formatted message.
    #[error("Model validation error: {0}")]
    ModelValidation(ValidationErrors),

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

    /// Expression evaluation or symbol table error, preserving the full
    /// [`ExpressionError`](openjd_expr::ExpressionError) with its kind and
    /// source-location context.
    #[error("Expression error: {0}")]
    Expression(#[source] openjd_expr::ExpressionError),

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

impl From<openjd_expr::SymbolTableError> for ModelError {
    fn from(e: openjd_expr::SymbolTableError) -> Self {
        ModelError::Expression(openjd_expr::ExpressionError::from(e))
    }
}

impl From<openjd_expr::FormatStringValidationError> for ModelError {
    fn from(e: openjd_expr::FormatStringValidationError) -> Self {
        ModelError::FormatStringError {
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
    /// Location of the error in the template structure (e.g., which field
    /// in the JSON/YAML tree). Used by consumers to navigate to or annotate
    /// the affected node.
    pub path: Vec<PathElement>,
    /// Complete human-readable error text, suitable for direct display.
    /// Includes source pointers and span context where available.
    pub message: String,
    /// Structured diagnostic data decomposing the error into a summary
    /// and source spans. `None` for errors without source position info
    /// (e.g., duplicate names, missing required fields, limit violations).
    pub detail: Option<ErrorDetail>,
}

/// Structured diagnostic data for a validation error.
#[derive(Debug, Clone)]
pub struct ErrorDetail {
    /// Human-readable error summary without source pointers.
    pub summary: String,
    /// Diagnostic spans identifying specific character ranges in the
    /// source text where the error occurs.
    pub spans: Vec<DiagnosticSpan>,
}

/// A diagnostic span identifying a specific character range in source text.
#[derive(Debug, Clone)]
pub struct DiagnosticSpan {
    /// Human-readable description of the diagnostic at this source location.
    pub summary: String,
    /// The source text containing the error.
    pub source: String,
    /// Byte offset of the error start within source.
    pub start: usize,
    /// Byte offset of the error end within source.
    pub end: usize,
    /// Position of the caret (most relevant character) relative to start.
    pub caret: usize,
}

/// Collects multiple validation errors with structured paths.
#[derive(Debug, Default)]
pub struct ValidationErrors {
    pub errors: Vec<ValidationError>,
    /// Model name used for Display formatting (set by `into_result`).
    model_name: Option<String>,
}

impl std::fmt::Display for ValidationErrors {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = self.model_name.as_deref().unwrap_or("Template");
        write!(f, "{}", self.format(name))
    }
}

impl ValidationErrors {
    /// Create a `ValidationErrors` with a single root-level message.
    pub fn single(msg: impl Into<String>) -> Self {
        let mut ve = Self::default();
        ve.add(&[], msg);
        ve
    }

    /// Add an error at the given path.
    pub fn add(&mut self, path: &[PathElement], msg: impl Into<String>) {
        self.errors.push(ValidationError {
            path: path.to_vec(),
            message: msg.into(),
            detail: None,
        });
    }

    /// Add an error at the given path with structured diagnostic detail.
    pub fn add_with_detail(
        &mut self,
        path: &[PathElement],
        msg: impl Into<String>,
        detail: ErrorDetail,
    ) {
        self.errors.push(ValidationError {
            path: path.to_vec(),
            message: msg.into(),
            detail: Some(detail),
        });
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.errors.is_empty()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.errors.len()
    }

    pub fn into_result(self, model_name: &str) -> Result<(), ModelError> {
        if self.errors.is_empty() {
            Ok(())
        } else {
            // Store the model name in the formatted output via Display
            // The ValidationErrors struct is moved into the enum variant
            Err(ModelError::ModelValidation(
                self.with_model_name(model_name),
            ))
        }
    }

    /// Set the model name for Display formatting and return self.
    fn with_model_name(mut self, name: &str) -> Self {
        self.model_name = Some(name.to_string());
        self
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
        let e = ModelError::UnsupportedSchema("version".into());
        assert_eq!(e.to_string(), "Unsupported schema version: version");
    }

    #[test]
    fn test_model_validation_msg() {
        let mut ve = ValidationErrors::default();
        ve.add(&[PathElement::Field("name".into())], "bad template");
        let e = ModelError::ModelValidation(ve.with_model_name("JobTemplate"));
        assert_eq!(
            e.to_string(),
            "Model validation error: 1 validation error for JobTemplate\nname:\n\tbad template"
        );
    }

    #[test]
    fn test_format_string_error_with_position() {
        let e = ModelError::FormatStringError {
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
        let e = ModelError::FormatStringError {
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
        assert!(matches!(result, Err(ModelError::ModelValidation(_))));
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

    #[test]
    fn test_model_validation_structured_access() {
        let mut ve = ValidationErrors::default();
        ve.add(
            &[PathElement::Field("steps".into()), PathElement::Index(0)],
            "missing script",
        );
        ve.add(&[PathElement::Field("name".into())], "too long");
        let err = ve.into_result("JobTemplate").unwrap_err();
        let errors = match &err {
            ModelError::ModelValidation(e) => e,
            other => panic!("expected ModelValidation, got: {other}"),
        };
        assert_eq!(errors.len(), 2);
        assert_eq!(
            errors.errors[0].path,
            vec![PathElement::Field("steps".into()), PathElement::Index(0)]
        );
        assert_eq!(errors.errors[0].message, "missing script");
        assert_eq!(
            errors.errors[1].path,
            vec![PathElement::Field("name".into())]
        );
        assert_eq!(errors.errors[1].message, "too long");
        assert_eq!(
            err.to_string(),
            "Model validation error: 2 validation errors for JobTemplate\n\
             steps[0]:\n\tmissing script\n\
             name:\n\ttoo long"
        );
    }
}
