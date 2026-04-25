// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Template parsing: YAML/JSON decoding and dispatch by specificationVersion.
//!
//! Mirrors Python `_parse.py`.

use std::str::FromStr;

use crate::error::{ModelError, ValidationErrors};
use crate::template::validate_v2023_09 as validate;
use crate::template::{EnvironmentTemplate, JobTemplate};
use crate::types::{
    CallerLimits, Extensions, KnownExtension, SpecificationRevision, TemplateSpecificationVersion,
    ValidationContext,
};

/// Document format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentType {
    Json,
    Yaml,
}

/// Parse a string into a generic YAML/JSON object.
///
/// When `caller_limits.max_template_size` is set, the document is rejected
/// if its byte length exceeds the limit (checked before parsing).
pub fn document_string_to_object(
    document: &str,
    doc_type: DocumentType,
    caller_limits: &CallerLimits,
) -> Result<serde_json::Value, ModelError> {
    if let Some(max) = caller_limits.max_template_size {
        if document.len() > max {
            return Err(ModelError::ModelValidation(ValidationErrors::single(
                format!(
                    "Template document size ({} bytes) exceeds caller limit of {max} bytes.",
                    document.len()
                ),
            )));
        }
    }

    let parsed: serde_json::Value = match doc_type {
        DocumentType::Json => serde_json::from_str(document).map_err(|e| {
            ModelError::DecodeValidation(format!(
                "The document is not a valid JSON document consisting of key-value pairs. {e}"
            ))
        })?,
        DocumentType::Yaml => {
            let options = serde_saphyr::options! {
                strict_booleans: true,
            };
            serde_saphyr::from_str_with_options(document, options).map_err(|e| {
                ModelError::DecodeValidation(format!(
                    "The document is not a valid YAML document consisting of key-value pairs. {e}"
                ))
            })?
        }
    };

    if !parsed.is_object() {
        return Err(ModelError::DecodeValidation(format!(
            "The document is not a valid {doc_type:?} document consisting of key-value pairs."
        )));
    }

    Ok(parsed)
}

/// Decode and validate a job template from a YAML value.
pub fn decode_job_template(
    template: serde_json::Value,
    supported_extensions: Option<&[&str]>,
    caller_limits: &CallerLimits,
) -> Result<JobTemplate, ModelError> {
    // Extract specificationVersion
    let version_str = template
        .get("specificationVersion")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| {
            ModelError::DecodeValidation(
                "Template is missing Open Job Description schema version key: specificationVersion"
                    .to_string(),
            )
        })?;

    let version = TemplateSpecificationVersion::from_str(&version_str)
        .map_err(|_| {
            let allowed = TemplateSpecificationVersion::JobTemplate2023_09.as_str();
            ModelError::DecodeValidation(format!(
                "Unknown template version: {version_str}. Values allowed for 'specificationVersion' in Job Templates are: {allowed}"
            ))
        })?;

    if !version.is_job_template() {
        let allowed = TemplateSpecificationVersion::JobTemplate2023_09.as_str();
        return Err(ModelError::DecodeValidation(format!(
            "Specification version '{version_str}' is not a Job Template version. \
             Values allowed for 'specificationVersion' in Job Templates are: {allowed}"
        )));
    }

    let jt: JobTemplate = serde_json::from_value(template)
        .map_err(|e| ModelError::DecodeValidation(format!("'{version_str}' failed checks: {e}")))?;

    // Build extension set: intersection of template-requested and supported
    let mut extensions = Extensions::new();
    if let Some(template_exts) = &jt.extensions {
        if template_exts.is_empty() {
            return Err(ModelError::DecodeValidation(
                "extensions, if provided, must be a non-empty list.".to_string(),
            ));
        }
        // Check for duplicate extension names
        let mut seen = std::collections::HashSet::new();
        for ext in template_exts {
            if !seen.insert(ext.as_str()) {
                return Err(ModelError::DecodeValidation(format!(
                    "Duplicate values for extension name are not allowed. Duplicate values: {}",
                    ext.as_str()
                )));
            }
        }
        let supported: std::collections::HashSet<&str> = supported_extensions
            .unwrap_or(&[])
            .iter()
            .copied()
            .collect();
        for ext in template_exts {
            let ext_str = ext.as_str();
            if !supported.contains(ext_str) {
                return Err(ModelError::DecodeValidation(format!(
                    "Unknown or unsupported extension: {ext_str}"
                )));
            }
            match KnownExtension::from_str(ext_str) {
                Ok(known) => {
                    extensions.insert(known);
                }
                Err(_) => {
                    return Err(ModelError::DecodeValidation(format!(
                        "Unknown or unsupported extension: {ext_str}"
                    )));
                }
            }
        }
    }

    let ctx = ValidationContext::with_extensions(SpecificationRevision::V2023_09, extensions)
        .with_caller_limits(caller_limits.clone());
    validate::validate_job_template(&jt, &ctx)?;

    Ok(jt)
}

/// Decode and validate an environment template from a YAML value.
pub fn decode_environment_template(
    template: serde_json::Value,
    supported_extensions: Option<&[&str]>,
) -> Result<EnvironmentTemplate, ModelError> {
    let version_str = template
        .get("specificationVersion")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| {
            ModelError::DecodeValidation(
                "Template is missing Open Job Description schema version key: specificationVersion"
                    .to_string(),
            )
        })?;

    let version = TemplateSpecificationVersion::from_str(&version_str).map_err(|_| {
        let allowed = TemplateSpecificationVersion::Environment2023_09.as_str();
        ModelError::DecodeValidation(format!(
            "Unknown template version: {version_str}. Allowed values are: {allowed}"
        ))
    })?;

    if !version.is_environment_template() {
        let allowed = TemplateSpecificationVersion::Environment2023_09.as_str();
        return Err(ModelError::DecodeValidation(format!(
            "Specification version '{version_str}' is not an Environment Template version. \
             Allowed values for 'specificationVersion' are: {allowed}"
        )));
    }

    let et: EnvironmentTemplate = serde_json::from_value(template)
        .map_err(|e| ModelError::DecodeValidation(format!("'{version_str}' failed checks: {e}")))?;

    // Build extension set: intersection of template-requested and supported
    let mut extensions = Extensions::new();
    if let Some(template_exts) = &et.extensions {
        if template_exts.is_empty() {
            return Err(ModelError::DecodeValidation(
                "extensions, if provided, must be a non-empty list.".to_string(),
            ));
        }
        // Check for duplicate extension names
        let mut seen = std::collections::HashSet::new();
        for ext in template_exts {
            if !seen.insert(ext.as_str()) {
                return Err(ModelError::DecodeValidation(format!(
                    "Duplicate values for extension name are not allowed. Duplicate values: {}",
                    ext.as_str()
                )));
            }
        }
        let supported: std::collections::HashSet<&str> = supported_extensions
            .unwrap_or(&[])
            .iter()
            .copied()
            .collect();
        for ext in template_exts {
            let ext_str = ext.as_str();
            if !supported.contains(ext_str) {
                return Err(ModelError::DecodeValidation(format!(
                    "Unknown or unsupported extension: {ext_str}"
                )));
            }
            match KnownExtension::from_str(ext_str) {
                Ok(known) => {
                    extensions.insert(known);
                }
                Err(_) => {
                    return Err(ModelError::DecodeValidation(format!(
                        "Unknown or unsupported extension: {ext_str}"
                    )));
                }
            }
        }
    }

    let ctx = ValidationContext::with_extensions(SpecificationRevision::V2023_09, extensions);
    validate::validate_environment_template(&et, &ctx)?;

    Ok(et)
}

/// Auto-detect template type and decode.
// Both variants are large structs only used as return values, not stored in collections.
#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
pub enum DecodedTemplate {
    Job(JobTemplate),
    Environment(EnvironmentTemplate),
}

/// Auto-detect whether a template is a job or environment template and decode it.
pub fn decode_template(
    template: serde_json::Value,
    supported_extensions: Option<&[&str]>,
    caller_limits: &CallerLimits,
) -> Result<DecodedTemplate, ModelError> {
    let version_str = template
        .get("specificationVersion")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| {
            ModelError::DecodeValidation(
                "Template is missing Open Job Description schema version key: specificationVersion"
                    .to_string(),
            )
        })?;

    let version = version_str
        .parse::<TemplateSpecificationVersion>()
        .map_err(|_| {
            ModelError::DecodeValidation(format!("Unknown template version: {version_str}"))
        })?;

    if version.is_job_template() {
        decode_job_template(template, supported_extensions, caller_limits).map(DecodedTemplate::Job)
    } else {
        decode_environment_template(template, supported_extensions)
            .map(DecodedTemplate::Environment)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn yaml_val(s: &str) -> serde_json::Value {
        serde_saphyr::from_str(s).unwrap()
    }

    // -- document_string_to_object --

    #[test]
    fn test_doc_string_to_object_json() {
        let result = document_string_to_object(
            r#"{"key": "value"}"#,
            DocumentType::Json,
            &CallerLimits::default(),
        )
        .unwrap();
        assert_eq!(result["key"].as_str().unwrap(), "value");
    }

    #[test]
    fn test_doc_string_to_object_yaml() {
        let result =
            document_string_to_object("key: value\n", DocumentType::Yaml, &CallerLimits::default())
                .unwrap();
        assert_eq!(result["key"].as_str().unwrap(), "value");
    }

    #[test]
    fn test_doc_string_not_a_dict_json() {
        assert!(document_string_to_object(
            "[1, 2, 3]",
            DocumentType::Json,
            &CallerLimits::default()
        )
        .is_err());
    }

    #[test]
    fn test_doc_string_not_a_dict_yaml() {
        assert!(document_string_to_object(
            "- 1\n- 2\n",
            DocumentType::Yaml,
            &CallerLimits::default()
        )
        .is_err());
    }

    #[test]
    fn test_doc_string_bad_parse_json() {
        assert!(
            document_string_to_object("{", DocumentType::Json, &CallerLimits::default()).is_err()
        );
    }

    #[test]
    fn test_doc_string_bad_parse_yaml() {
        assert!(
            document_string_to_object("-", DocumentType::Yaml, &CallerLimits::default()).is_err()
        );
    }

    // -- decode_job_template --

    #[test]
    fn test_decode_job_template_missing_spec_version() {
        let v = yaml_val(r#"{"notspecversion": "badvalue"}"#);
        assert!(decode_job_template(v, None, &CallerLimits::default()).is_err());
    }

    #[test]
    fn test_decode_job_template_unknown_version() {
        let v = yaml_val(r#"{"specificationVersion": "badvalue"}"#);
        assert!(decode_job_template(v, None, &CallerLimits::default()).is_err());
    }

    #[test]
    fn test_decode_job_template_not_job_version() {
        let v = yaml_val(r#"{"specificationVersion": "environment-2023-09"}"#);
        assert!(decode_job_template(v, None, &CallerLimits::default()).is_err());
    }

    #[test]
    fn test_decode_job_template_success() {
        let v = yaml_val(
            r#"{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "name",
            "steps": [{"name": "step", "script": {"actions": {"onRun": {"command": "do thing"}}}}]
        }"#,
        );
        let jt = decode_job_template(v, None, &CallerLimits::default()).unwrap();
        assert_eq!(jt.specification_version, "jobtemplate-2023-09");
    }

    // -- decode_environment_template --

    #[test]
    fn test_decode_env_template_missing_spec_version() {
        let v = yaml_val(r#"{"notspecversion": "badvalue"}"#);
        assert!(decode_environment_template(v, None).is_err());
    }

    #[test]
    fn test_decode_env_template_unknown_version() {
        let v = yaml_val(r#"{"specificationVersion": "badvalue"}"#);
        assert!(decode_environment_template(v, None).is_err());
    }

    #[test]
    fn test_decode_env_template_not_env_version() {
        let v = yaml_val(r#"{"specificationVersion": "jobtemplate-2023-09"}"#);
        assert!(decode_environment_template(v, None).is_err());
    }

    #[test]
    fn test_decode_env_template_success() {
        let v = yaml_val(
            r#"{
            "specificationVersion": "environment-2023-09",
            "environment": {
                "name": "FooEnv",
                "description": "A description",
                "script": {"actions": {"onEnter": {"command": "echo", "args": ["Hello", "World"]}}}
            }
        }"#,
        );
        let et = decode_environment_template(v, None).unwrap();
        assert_eq!(et.specification_version, "environment-2023-09");
    }

    // -- decode_template (auto-detect) --

    #[test]
    fn test_decode_template_auto_detect_job() {
        let v = yaml_val(
            r#"{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "name",
            "steps": [{"name": "step", "script": {"actions": {"onRun": {"command": "do thing"}}}}]
        }"#,
        );
        assert!(matches!(
            decode_template(v, None, &CallerLimits::default()).unwrap(),
            DecodedTemplate::Job(_)
        ));
    }

    #[test]
    fn test_decode_template_auto_detect_env() {
        let v = yaml_val(
            r#"{
            "specificationVersion": "environment-2023-09",
            "environment": {
                "name": "FooEnv",
                "description": "A description",
                "script": {"actions": {"onEnter": {"command": "echo", "args": ["Hello", "World"]}}}
            }
        }"#,
        );
        assert!(matches!(
            decode_template(v, None, &CallerLimits::default()).unwrap(),
            DecodedTemplate::Environment(_)
        ));
    }

    #[test]
    fn test_decode_template_missing_version() {
        let v = yaml_val(r#"{"name": "test"}"#);
        let err = decode_template(v, None, &CallerLimits::default()).unwrap_err();
        assert!(err.to_string().contains("specificationVersion"));
    }

    #[test]
    fn test_decode_template_unknown_version() {
        let v = yaml_val(r#"{"specificationVersion": "badvalue"}"#);
        let err = decode_template(v, None, &CallerLimits::default()).unwrap_err();
        assert!(err.to_string().contains("Unknown template version"));
    }

    // ══════════════════════════════════════════════════════════════
    // ══════════════════════════════════════════════════════════════
    // ModelValidation structured errors via decode_job_template
    // ══════════════════════════════════════════════════════════════
    #[test]
    fn validation_error_has_structured_paths() {
        // Step name exceeds 64 chars — triggers ModelValidation
        let long_name = "a".repeat(128);
        let v = yaml_val(&format!(
            r#"{{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "test",
            "steps": [{{"name": "{long_name}", "script": {{"actions": {{"onRun": {{"command": "echo"}}}}}}}}]
        }}"#,
        ));
        let err = decode_job_template(v, None, &Default::default()).unwrap_err();
        let errors = match &err {
            crate::error::ModelError::ModelValidation(e) => e,
            other => panic!("expected ModelValidation, got: {other}"),
        };
        assert_eq!(errors.len(), 1);
        let e = &errors.errors[0];
        assert_eq!(
            e.path,
            vec![
                crate::error::PathElement::Field("steps".into()),
                crate::error::PathElement::Index(0),
                crate::error::PathElement::Field("name".into()),
            ]
        );
        assert!(
            e.message.contains("64"),
            "expected message about 64-char limit, got: {}",
            e.message
        );
        // Display output matches the Pydantic-compatible format
        assert_eq!(
            err.to_string(),
            format!(
                "Model validation error: 1 validation error for JobTemplate\nsteps[0] -> name:\n\t{}",
                e.message
            )
        );
    }

    #[test]
    fn validation_error_paths_contain_steps() {
        // Missing 'script' — step has no actions
        let v = yaml_val(
            r#"{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "test",
            "steps": [{"name": "s"}]
        }"#,
        );
        let err = decode_job_template(v, None, &Default::default()).unwrap_err();
        let errors = match &err {
            crate::error::ModelError::ModelValidation(e) => e,
            other => panic!("expected ModelValidation, got: {other}"),
        };
        assert!(!errors.is_empty());
        // Every error should reference steps[0]
        for e in &errors.errors {
            assert!(
                e.path.len() >= 2,
                "expected path with at least 2 elements, got: {:?}",
                e.path
            );
            assert_eq!(e.path[0], crate::error::PathElement::Field("steps".into()),);
            assert_eq!(e.path[1], crate::error::PathElement::Index(0),);
        }
    }

    #[test]
    fn validation_error_detail_populated_for_format_string_errors() {
        // Template with undefined variable in format string
        let v = yaml_val(
            r#"{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "{{Param.Undefined}}",
            "steps": [{"name": "s", "script": {"actions": {"onRun": {"command": "echo"}}}}]
        }"#,
        );
        let err = decode_job_template(v, None, &Default::default()).unwrap_err();
        let errors = match &err {
            crate::error::ModelError::ModelValidation(e) => e,
            other => panic!("expected ModelValidation, got: {other}"),
        };
        assert_eq!(errors.len(), 1);
        let e = &errors.errors[0];
        assert_eq!(
            e.path,
            vec![crate::error::PathElement::Field("name".into())]
        );
        // message contains the full formatted text
        assert!(e.message.contains("Undefined variable"));
        // detail is populated with structured diagnostic data
        let detail = e
            .detail
            .as_ref()
            .expect("detail should be Some for format string errors");
        assert!(detail.summary.contains("Undefined variable"));
        assert_eq!(detail.spans.len(), 1);
        let span = &detail.spans[0];
        assert!(span.summary.contains("Undefined variable"));
        assert_eq!(span.source, "{{Param.Undefined}}");
        assert!(span.end > span.start);
    }
}
