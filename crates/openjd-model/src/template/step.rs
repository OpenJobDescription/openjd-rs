// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Step types per spec §3.

use super::actions::{Action, CancelationMode, StepActions};
use super::constrained_strings::Description;
use super::environment::{EmbeddedFile, Environment};
use super::host_requirements::HostRequirements;
use super::task_parameters::StepParameterSpaceDefinition;
use crate::format_string::FormatString;
use serde::Deserialize;

/// SimpleAction syntax sugar (FEATURE_BUNDLE_1).
/// Allows specifying a script interpreter directly instead of a full StepScript.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SimpleAction {
    /// Let bindings evaluated once per task (requires EXPR extension).
    #[serde(rename = "let")]
    pub let_bindings: Option<Vec<String>>,
    /// The script content to execute. Required.
    pub script: String,
    /// Additional arguments to pass to the interpreter.
    pub args: Option<Vec<FormatString>>,
    /// Maximum allowed runtime in seconds.
    pub timeout: Option<FormatString>,
    /// How to cancel the action.
    pub cancelation: Option<CancelationMode>,
}

/// §3 StepTemplate
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StepTemplate {
    pub name: String,
    pub description: Option<Description>,
    #[serde(rename = "let")]
    pub let_bindings: Option<Vec<String>>,
    pub dependencies: Option<Vec<StepDependency>>,
    pub step_environments: Option<Vec<Environment>>,
    pub host_requirements: Option<HostRequirements>,
    pub parameter_space: Option<StepParameterSpaceDefinition>,
    pub script: Option<StepScript>,
    // SimpleAction syntax sugar (§3.5, FEATURE_BUNDLE_1)
    pub bash: Option<SimpleAction>,
    pub python: Option<SimpleAction>,
    pub cmd: Option<SimpleAction>,
    pub powershell: Option<SimpleAction>,
    pub node: Option<SimpleAction>,
}

impl StepTemplate {
    /// De-sugar SimpleAction syntax into equivalent StepScript.
    /// If the step already has a `script` field, returns `Ok(Some(clone))`.
    /// If it uses a SimpleAction (bash/python/cmd/powershell/node), transforms
    /// it into a StepScript with an embedded file and onRun action.
    /// Returns `Err` if the SimpleAction script contains malformed format string syntax.
    pub fn resolve_syntax_sugar(&self) -> Result<Option<StepScript>, crate::ModelError> {
        if let Some(script) = &self.script {
            return Ok(Some(script.clone()));
        }

        let interpreters: &[(&str, &str, &[&str], Option<&SimpleAction>)] = &[
            ("python", ".py", &[], self.python.as_ref()),
            ("bash", ".sh", &[], self.bash.as_ref()),
            ("cmd", ".bat", &["/C"], self.cmd.as_ref()),
            ("powershell", ".ps1", &["-File"], self.powershell.as_ref()),
            ("node", ".js", &[], self.node.as_ref()),
        ];

        for &(command, ext, arg_prefix, sa_opt) in interpreters {
            let Some(sa) = sa_opt else { continue };

            let safe_name: String = self
                .name
                .chars()
                .map(|c| if c.is_alphanumeric() { c } else { '_' })
                .take(200)
                .collect();
            let safe_name = if safe_name.starts_with(|c: char| c.is_ascii_digit()) {
                format!("_{safe_name}")
            } else {
                safe_name
            };
            let embedded_name = format!("{safe_name}_script");
            let filename = format!("{embedded_name}{ext}");
            let file_ref = format!("{{{{Task.File.{embedded_name}}}}}");

            let mut args = Vec::new();
            for prefix_arg in arg_prefix {
                args.push(FormatString::new(prefix_arg).unwrap());
            }
            args.push(FormatString::new(&file_ref).unwrap());
            if let Some(user_args) = &sa.args {
                args.extend(user_args.iter().cloned());
            }

            return Ok(Some(StepScript {
                let_bindings: sa.let_bindings.clone(),
                actions: StepActions {
                    on_run: Action {
                        command: FormatString::new(command).unwrap(),
                        args: Some(args),
                        cancelation: sa.cancelation.clone(),
                        timeout: sa.timeout.clone(),
                    },
                },
                embedded_files: Some(vec![EmbeddedFile {
                    name: embedded_name,
                    file_type: crate::types::FileType::Text,
                    filename: Some(filename),
                    data: Some(FormatString::new(&sa.script).map_err(|e| {
                        crate::ModelError::DecodeValidation(format!(
                            "SimpleAction script format string error: {e}"
                        ))
                    })?),
                    runnable: Some(true),
                    end_of_line: None,
                }]),
            }));
        }

        Ok(None)
    }
}

/// §3.2 StepDependency
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StepDependency {
    pub depends_on: String,
}

/// §3.5 StepScript
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StepScript {
    #[serde(rename = "let")]
    pub let_bindings: Option<Vec<String>>,
    pub actions: StepActions,
    pub embedded_files: Option<Vec<EmbeddedFile>>,
}

#[cfg(test)]
mod tests {
    use super::StepTemplate;

    #[test]
    fn resolve_syntax_sugar_returns_error_for_malformed_format_string() {
        let step: StepTemplate = serde_saphyr::from_str(
            r#"
            name: TestStep
            bash:
              script: "echo '{{broken'"
            "#,
        )
        .unwrap();

        let result = step.resolve_syntax_sugar();
        assert!(
            result.is_err(),
            "resolve_syntax_sugar should return Err for malformed format string"
        );
    }

    #[test]
    fn resolve_syntax_sugar_ok_for_valid_script() {
        let step: StepTemplate = serde_saphyr::from_str(
            r#"
            name: TestStep
            bash:
              script: "echo hello"
            "#,
        )
        .unwrap();

        let result = step.resolve_syntax_sugar();
        assert!(result.is_ok(), "valid script should succeed");
        assert!(
            result.unwrap().is_some(),
            "bash step should produce a StepScript"
        );
    }
}
