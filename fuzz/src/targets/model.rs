// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! openjd-model targets: decode → preprocess → create_job → parameter-space
//! iteration → dependency graph → run-time format-string resolution, the
//! path `openjd check` / `openjd run` and the worker agent take on an
//! untrusted template.
//!
//! Oracles beyond "never panics":
//! - **Validation implies runnable** (#349, #397, #421, #315, #361): a
//!   template that decodes and instantiates must not fail at run time with
//!   an undefined-symbol / unknown-function / syntax error when its actions,
//!   `let` bindings, and embedded files are resolved the way the session
//!   resolves them.
//! - **Preprocess idempotence** (#384, #388): feeding
//!   `preprocess_job_parameters` its own output yields the same values.
//! - **Determinism and Eq/Hash agreement** on created jobs.
//! - **Parameter-space consistency** (#398, 20c7876): `len` matches
//!   iteration, `get(i)` equals the i-th iterated set, and every iterated set
//!   passes `contains` / `validate_containment`.
//! - **Topological order** respects every declared dependency.

use crate::gen::template::{environment_template, job_template, ParamInfo};
use crate::gen::{self, one_in, pick, upto};
use crate::targets::text_and_control;
use crate::{known_issue, oracle};
use arbitrary::Unstructured;
use openjd_expr::{
    ExprValue, ExpressionError, ExpressionErrorKind, FormatString, FormatStringOptions,
    FunctionLibrary, HostContext, PathFormat, SymbolTable,
};
use openjd_model::job::{Job, Step};
use openjd_model::template::{EnvironmentTemplate, JobTemplate};
use openjd_model::{
    capabilities, create_job, decode_environment_template, decode_job_template, decode_template,
    evaluate_let_bindings, merge_job_parameter_definitions, preprocess_job_parameters,
    template::parse::document_string_to_object, CallerLimits, DocumentType,
    JobParameterInputValues, JobParameterType, JobParameterValues, ModelError, ModelExtension,
    PathParameterOptions, StepDependencyGraph, StepParameterSpaceIterator, TaskParameterSet,
    TemplateSpecificationVersion,
};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::str::FromStr;

fn all_extensions() -> Vec<&'static str> {
    ModelExtension::ALL.iter().map(|e| e.as_str()).collect()
}

fn limits() -> CallerLimits {
    CallerLimits {
        max_eval_memory_bytes: Some(8 << 20),
        max_eval_operations: Some(200_000),
        max_task_count: Some(100_000),
        ..Default::default()
    }
}

fn hash_of<T: Hash>(v: &T) -> u64 {
    let mut h = DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

fn is_static_error(e: &ExpressionError) -> bool {
    matches!(
        e.kind(),
        ExpressionErrorKind::UndefinedVariable { .. }
            | ExpressionErrorKind::UnknownFunction { .. }
            | ExpressionErrorKind::UnsupportedSyntax { .. }
    )
}

fn model_static_error(e: &ModelError) -> Option<&ExpressionError> {
    match e {
        ModelError::Expression(x) if is_static_error(x) => Some(x),
        ModelError::FormatStringError {
            source: Some(x), ..
        } if is_static_error(x) => Some(x),
        _ => None,
    }
}

/// Input values for a template's parameters: CLI-style strings, typed
/// values, or absent.
fn inputs(u: &mut Unstructured<'_>, params: &[ParamInfo]) -> JobParameterInputValues {
    let mut out = JobParameterInputValues::new();
    for p in params {
        match upto(u, 3) {
            0 => {}
            1 => {
                let v = crate::gen::template::json_value_of(u, &p.ty);
                let s = match v {
                    serde_json::Value::String(s) => s,
                    other => other.to_string(),
                };
                out.insert(p.name.clone(), ExprValue::String(s));
            }
            _ => {
                let v = crate::gen::value::value_of(u, &p.ty, PathFormat::Posix);
                out.insert(p.name.clone(), v);
            }
        }
    }
    if one_in(u, 8) {
        out.insert("Unknown".into(), ExprValue::Int(1));
    }
    out
}

/// Run the whole instantiation pipeline and its oracles.
/// Does a template's source contain a construct the static pass evaluates
/// speculatively (`if`/`else`, `and`, `or`)? See known issue
/// `expr-boolop-static-type`.
fn speculative(doc_text: &str) -> bool {
    doc_text.contains(" if ") || doc_text.contains(" and ") || doc_text.contains(" or ")
}

pub fn pipeline(
    template: &JobTemplate,
    env_templates: &[EnvironmentTemplate],
    input: &JobParameterInputValues,
    u: &mut Unstructured<'_>,
    source: &str,
) {
    let spec = speculative(source);
    let _ = template.profile();
    let _ = template.description();
    let _ = template.name().raw();
    for d in template.parameter_definitions_list() {
        let _ = (
            d.default_value(),
            d.min_length(),
            d.allowed_values_strings(),
        );
        let _ = (
            d.description(),
            d.type_name(),
            d.path_properties(),
            d.max_length(),
        );
        let _ = (
            d.min_value_i64(),
            d.max_value_i64(),
            d.min_value_f64(),
            d.max_value_f64(),
        );
        let _ = (d.allowed_values_i64(), d.allowed_values_f64());
        // Constraint checks on a value of the parameter's own type.
        let v = crate::gen::value::value_of(u, &d.job_param_type().expr_type(), PathFormat::Posix);
        let _ = d.check_constraints(&v);
    }
    for env in template.job_environments.iter().flatten() {
        if let Some(s) = &env.script {
            let _ = s.actions.iter_named().count();
        }
    }
    let _ = openjd_model::ValidationContext::new(openjd_model::SpecificationRevision::V2023_09);
    let _ = PathParameterOptions::new("/tmpl", "/cwd");
    if let Ok(merged) = merge_job_parameter_definitions(template, env_templates) {
        for m in &merged {
            let _ = m.validate_satisfiable();
        }
    }

    let fmt = *pick(u, &[PathFormat::Posix, PathFormat::Windows]);
    let (tdir, cwd) = match fmt {
        PathFormat::Windows => ("C:\\tmpl", "C:\\cwd"),
        _ => ("/tmpl", "/cwd"),
    };
    let path_options = PathParameterOptions {
        job_template_dir: tdir,
        current_working_dir: cwd,
        path_format: fmt,
        allow_template_dir_walk_up: one_in(u, 2),
        allow_uri_path_values: one_in(u, 2),
    };
    let Ok(values) = preprocess_job_parameters(template, input, env_templates, &path_options)
    else {
        return;
    };

    // Idempotence: preprocessing its own output changes nothing.
    let again_input: JobParameterInputValues = values
        .iter()
        .map(|(k, v)| (k.clone(), v.value.clone()))
        .collect();
    match preprocess_job_parameters(template, &again_input, env_templates, &path_options) {
        Ok(again) => {
            for (k, v) in &values {
                let a = again.get(k);
                let same = a.is_some_and(|a| a.value.equals(&v.value));
                let path_typed = v.value.expr_type().to_string().contains("path")
                    || matches!(
                        v.param_type,
                        JobParameterType::Path | JobParameterType::ListPath
                    );
                if path_typed {
                    known_issue!(
                        "model-preprocess-path-idempotence",
                        same,
                        "preprocess not idempotent for {k:?}: {:?} -> {:?}",
                        v.value,
                        a.map(|a| &a.value)
                    );
                } else {
                    oracle!(
                        same,
                        "preprocess not idempotent for {k:?}: {:?} -> {:?}",
                        v.value,
                        a.map(|a| &a.value)
                    );
                }
            }
        }
        Err(e) => oracle!(
            false,
            "preprocess_job_parameters rejects its own output: {e}\n{again_input:?}"
        ),
    }

    let ctx = template
        .default_validation_context()
        .with_caller_limits(limits());
    let job = match create_job(template, &values, &ctx) {
        Ok(j) => j,
        Err(e) => {
            if let Some(x) = model_static_error(&e) {
                if spec {
                    known_issue!(
                        "expr-boolop-static-type",
                        false,
                        "template validated but create_job fails with a static error: {x}"
                    );
                } else if has_float_format_range(template) {
                    known_issue!(
                        "model-float-range-unchecked-scope",
                        false,
                        "template validated but create_job fails with a static error: {x}"
                    );
                } else {
                    oracle!(
                        false,
                        "template validated but create_job fails with a static error: {x}"
                    );
                }
            }
            return;
        }
    };
    match create_job(template, &values, &ctx) {
        Ok(j2) => {
            oracle!(j2 == job, "create_job is nondeterministic");
            oracle!(hash_of(&j2) == hash_of(&job), "equal jobs hash differently");
        }
        Err(e) => oracle!(false, "create_job succeeded once then failed: {e}"),
    }

    check_dependencies(&job);
    for step in &job.steps {
        let tasks = check_param_space(step);
        run_time_resolution(template, &job, step, &values, &tasks, u);
    }
}

fn check_dependencies(job: &Job) {
    let Ok(graph) = StepDependencyGraph::new(job) else {
        return;
    };
    let _ = (
        graph.max_indegree(),
        graph.max_outdegree(),
        graph.node_count(),
    );
    for (i, step) in job.steps.iter().enumerate() {
        let node = graph.step_node(&step.name);
        oracle!(node.is_some(), "graph has no node for step {:?}", step.name);
        if let Some(n) = graph.node(i) {
            for &e in n.in_edges.iter().chain(&n.out_edges) {
                oracle!(graph.edge(e).is_some(), "node {i} lists missing edge {e}");
            }
        }
    }
    for env in job.job_environments.iter().flatten() {
        if let Some(s) = &env.script {
            let a = &s.actions;
            let named = a.iter_named().count();
            oracle!(
                named == a.iter_actions().count() && a.has_any_action() == (named > 0),
                "job environment action helpers disagree: {named} named actions"
            );
            // The wrap hooks are a subset of the named actions.
            let hooks = a
                .wrap_hooks()
                .iter()
                .filter(|(_, h, _)| h.is_some())
                .count();
            oracle!(
                a.has_any_wrap_hook() == (hooks > 0) && hooks <= named,
                "job environment wrap hooks: {hooks} of {named} named actions"
            );
        }
    }
    if let Ok(order) = graph.topo_sorted() {
        oracle!(order.len() == job.steps.len(), "topo sort dropped steps");
        let pos = |name: &str| order.iter().position(|&i| job.steps[i].name == name);
        for step in &job.steps {
            for dep in step.dependencies.iter().flatten() {
                let (a, b) = (pos(&dep.depends_on), pos(&step.name));
                oracle!(
                    a < b,
                    "topo order puts {:?} before its dependency {:?}",
                    step.name,
                    dep.depends_on
                );
            }
        }
        let names = graph.topo_sorted_names();
        oracle!(
            names.is_ok(),
            "topo_sorted succeeded but topo_sorted_names failed"
        );
    }
}

/// Task parameter sets are equal by name, regardless of insertion order.
fn set_eq(a: &TaskParameterSet, b: &TaskParameterSet) -> bool {
    a.len() == b.len()
        && a.iter().all(|(k, va)| {
            b.get(k)
                .is_some_and(|vb| va.param_type == vb.param_type && va.value.equals(&vb.value))
        })
}

/// Does the template have a FLOAT task parameter whose range uses a format
/// string (see known issue `model-float-range-unchecked-scope`)?
fn has_float_format_range(t: &JobTemplate) -> bool {
    use openjd_model::template::{FloatRange, FloatRangeItem, TaskParameterDefinition};
    t.steps.iter().any(|s| {
        s.parameter_space.iter().any(|ps| {
            ps.task_parameter_definitions.iter().any(|d| match d {
                TaskParameterDefinition::FLOAT(f) => match &f.range {
                    FloatRange::Expression(_) => true,
                    FloatRange::List(items) => items
                        .iter()
                        .any(|i| matches!(i, FloatRangeItem::FormatString(_))),
                },
                _ => false,
            })
        })
    })
}

fn sets_eq(a: &[TaskParameterSet], b: &[TaskParameterSet]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| set_eq(x, y))
}

/// Parameter-space invariants. Returns a few task parameter sets to resolve.
fn check_param_space(step: &Step) -> Vec<TaskParameterSet> {
    let Some(space) = &step.parameter_space else {
        return vec![TaskParameterSet::new()];
    };
    let Ok(mut it) = StepParameterSpaceIterator::new(space) else {
        return vec![];
    };
    let len = it.len();
    let _ = (
        it.names(),
        it.chunks_parameter_name(),
        it.chunks_default_task_count(),
    );
    let adaptive = it.chunks_adaptive();
    // Adaptive chunking reports len 0 (the count is not known up front).
    oracle!(
        adaptive || it.is_empty() == (len == 0),
        "is_empty disagrees with len {len}"
    );
    let walked: Vec<TaskParameterSet> = it.by_ref().take(2001).collect();
    if !adaptive && len <= 2000 {
        oracle!(
            walked.len() == len,
            "step {:?}: len() = {len} but iteration yields {}",
            step.name,
            walked.len()
        );
    }
    it.reset();
    let rewalk: Vec<TaskParameterSet> = it.by_ref().take(walked.len()).collect();
    oracle!(
        sets_eq(&rewalk, &walked),
        "step {:?}: reset() does not replay the walk",
        step.name
    );
    for (i, set) in walked.iter().enumerate().take(64) {
        if !adaptive {
            let got = it.get(i);
            oracle!(
                got.as_ref().is_some_and(|g| set_eq(g, set)),
                "step {:?}: get({i}) = {got:?} but iteration gives {set:?}",
                step.name
            );
        }
        oracle!(
            it.contains(set),
            "step {:?}: contains() rejects iterated set {set:?}",
            step.name
        );
        if let Err(e) = it.validate_containment(set) {
            oracle!(
                false,
                "step {:?}: validate_containment rejects iterated set {set:?}: {e}",
                step.name
            );
        }
    }
    if !adaptive {
        oracle!(
            it.get(len).is_none(),
            "step {:?}: get(len) returned a set",
            step.name
        );
    }
    if let Ok(one) = StepParameterSpaceIterator::new_with_chunk_override(space, Some(1)) {
        let _ = one.len();
    }
    if adaptive {
        // Adaptive chunking: resize mid-walk; the walk still terminates.
        if let Ok(mut it2) = StepParameterSpaceIterator::new(space) {
            let mut n = 0;
            while it2.next().is_some() && n < 2000 {
                it2.set_chunks_default_task_count(1 + n % 7);
                n += 1;
            }
        }
    }
    walked.into_iter().take(3).collect()
}

/// Resolve a step's script the way the session does at run time and
/// require that no static-class error appears.
fn run_time_resolution(
    template: &JobTemplate,
    job: &Job,
    step: &Step,
    values: &JobParameterValues,
    tasks: &[TaskParameterSet],
    u: &mut Unstructured<'_>,
) {
    let profile = template.profile();
    let _ = (
        profile.has_extension(ModelExtension::Expr),
        openjd_model::ModelProfile::current().extensions().len(),
        openjd_model::ModelProfile::latest().revision(),
    );
    let lib =
        FunctionLibrary::for_profile(&profile.to_expr_profile(HostContext::with_rules(vec![])));
    let mut session = openjd_sessions::Session::new_for_test(PathBuf::from(if cfg!(windows) {
        "C:\\openjd-fuzz-session"
    } else {
        "/openjd-fuzz-session"
    }))
    .with_profile(profile)
    // Worker-supplied path mapping is applied to PATH parameters when the
    // session builds the run-time symbol table.
    .with_path_mapping(crate::targets::expr::path_rules(u));
    session.extend_path_mapping_rules(crate::targets::expr::path_rules(u));
    session.set_job_parameter_values_for_test(values.clone());
    let _ = (
        session.session_id(),
        session.state(),
        session.working_directory(),
        session.files_directory(),
        session.environments_entered(),
        session.job_parameter_values().len(),
        session.path_mapping_rules().len(),
        session.get_enabled_extensions(),
        session.action_status(),
    );
    let env = session.evaluate_env_vars(None);
    oracle!(
        env.contains_key("OPENJD_SESSION_WORKING_DIR"),
        "evaluate_env_vars lacks OPENJD_SESSION_WORKING_DIR"
    );
    let opts = FormatStringOptions::new()
        .with_library(&*lib)
        .with_path_format(PathFormat::host())
        .with_memory_limit(8 << 20)
        .with_operation_limit(200_000);
    let check = |what: &str, fs: &FormatString, st: &SymbolTable| {
        if let Err(e) = fs.resolve_with(st, &opts) {
            oracle!(
                !is_static_error(&e),
                "step {:?}: {what} {:?} validated but fails at run time: {e}",
                step.name,
                fs.raw()
            );
        }
    };
    for task in tasks {
        let Ok(mut st) = session.build_symbol_table(Some(task), step.resolved_symtab.as_ref())
        else {
            continue;
        };
        // Run-time symbols `Session::run_task` adds on top of
        // build_symbol_table.
        let _ = st.set("Session.HasPathMappingRules", ExprValue::Bool(false));
        let _ = st.set(
            "Session.PathMappingRulesFile",
            ExprValue::new_path("/rules.json", PathFormat::host()),
        );
        for f in step.script.embedded_files.iter().flatten() {
            let _ = st.set(
                &format!("Task.File.{}", f.name),
                ExprValue::new_path("/files/x", PathFormat::host()),
            );
        }
        if let Some(lets) = &step.script.let_bindings {
            match evaluate_let_bindings(
                lets,
                &st,
                Some(&lib),
                PathFormat::host(),
                Some(8 << 20),
                Some(200_000),
            ) {
                Ok(s) => st = s,
                Err(e) => {
                    if let Some(x) = model_static_error(&e) {
                        oracle!(
                            false,
                            "step {:?}: let bindings validated but fail at run time: {x}",
                            step.name
                        );
                    }
                    continue;
                }
            }
        }
        for f in step.script.embedded_files.iter().flatten() {
            if let Some(d) = &f.data {
                check("embedded file data", d, &st);
            }
        }
        let a = &step.script.actions.on_run;
        check("command", &a.command, &st);
        // Redaction of a resolved command never panics and is idempotent.
        if let Ok(cmd) = a.command.resolve_string_with(&st, &opts) {
            let r = session.redact(&cmd);
            oracle!(
                session.redact(&r) == r,
                "Session::redact is not idempotent on {cmd:?}"
            );
        }
        for arg in a.args.iter().flatten() {
            check("arg", arg, &st);
        }
        if let Some(t) = &a.timeout {
            check("timeout", t, &st);
        }
    }
    let _ = job;
}

fn decode_any(value: serde_json::Value, u: &mut Unstructured<'_>) {
    let exts = all_extensions();
    let supported: Option<&[&str]> = match upto(u, 2) {
        0 => Some(&exts),
        1 => None,
        _ => Some(&exts[..1]),
    };
    let lim = limits();
    let source = value.to_string();
    match decode_template(value.clone(), supported, &lim) {
        Ok(openjd_model::DecodedTemplate::Job(t)) => {
            let again = decode_job_template(value, supported, &lim);
            oracle!(
                again.is_ok(),
                "decode_template accepts a job template decode_job_template rejects"
            );
            let input = JobParameterInputValues::new();
            pipeline(&t, &[], &input, u, &source);
        }
        Ok(openjd_model::DecodedTemplate::Environment(e)) => {
            let again = decode_environment_template(value, supported, &lim);
            oracle!(again.is_ok(), "decode_template accepts an environment template decode_environment_template rejects");
            let _ = openjd_model::convert_environment(e.environment());
        }
        Err(e) => {
            let _ = e.to_string();
            if let ModelError::ModelValidation(v) = &e {
                oracle!(
                    v.is_empty() == v.errors.is_empty() && v.len() == v.errors.len(),
                    "ValidationErrors is_empty/len disagree"
                );
            }
        }
    }
}

/// `document_string_to_object` (YAML and JSON) + `decode_template` +
/// the full pipeline, on arbitrary text.
pub fn decode_text(data: &[u8]) {
    let Some((text, ctl)) = text_and_control(data) else {
        return;
    };
    if crate::known::skip_deep_interpolations(text) {
        return;
    }
    let mut u = Unstructured::new(ctl);
    let lim = limits();
    for doc_type in [DocumentType::Yaml, DocumentType::Json] {
        if let Ok(v) = document_string_to_object(text, doc_type, &lim) {
            decode_any(v, &mut u);
        }
    }
}

/// Generated job templates (plus optional environment templates) through
/// the full pipeline.
pub fn job_gen(data: &[u8]) {
    let mut u = Unstructured::new(data);
    let g = job_template(&mut u);
    let exts = all_extensions();
    let lim = limits();
    let Ok(template) = decode_job_template(g.doc.clone(), Some(&exts), &lim) else {
        return;
    };
    // The same document as YAML text must decode the same way.
    if let Ok(yaml) = serde_saphyr::to_string(&g.doc) {
        if let Ok(v) = document_string_to_object(&yaml, DocumentType::Yaml, &lim) {
            oracle!(
                decode_job_template(v, Some(&exts), &lim).is_ok(),
                "template decodes from JSON but not from its YAML rendering:\n{yaml}"
            );
        }
    }
    let mut envs = Vec::new();
    if one_in(&mut u, 3) {
        let e = environment_template(&mut u);
        if let Ok(t) = decode_environment_template(e.doc, Some(&exts), &lim) {
            envs.push(t);
        }
    }
    let input = inputs(&mut u, &g.params);
    pipeline(&template, &envs, &input, &mut u, &g.doc.to_string());
}

/// Generated environment templates: decode, merge with a job template's
/// parameters, convert with a symbol table.
pub fn env_gen(data: &[u8]) {
    let mut u = Unstructured::new(data);
    let g = environment_template(&mut u);
    let exts = all_extensions();
    let lim = limits();
    let doc_text = g.doc.to_string();
    let Ok(t) = decode_environment_template(g.doc, Some(&exts), &lim) else {
        return;
    };
    let mut st = SymbolTable::new();
    for p in &g.params {
        let v = crate::gen::value::value_of(&mut u, &p.ty, PathFormat::Posix);
        let _ = st.set(&format!("Param.{}", p.name), v);
    }
    let env = openjd_model::convert_environment_with_symtab(t.environment(), Some(&st));
    if let Some(rs) = &env.resolved_symtab {
        oracle!(
            rs.to_symtab(PathFormat::Posix).is_ok(),
            "convert_environment_with_symtab produced an undecodable resolved_symtab"
        );
    }
    let jg = job_template(&mut u);
    let jdoc = jg.doc.to_string();
    if let Ok(jt) = decode_job_template(jg.doc, Some(&exts), &lim) {
        let input = inputs(&mut u, &jg.params);
        pipeline(
            &jt,
            std::slice::from_ref(&t),
            &input,
            &mut u,
            &format!("{doc_text}{jdoc}"),
        );
    }
}

/// Small public validators and parsers that take strings.
pub fn misc(data: &[u8]) {
    let Some((text, ctl)) = text_and_control(data) else {
        return;
    };
    if crate::known::skip_deep_unclosed_brackets(text) {
        return;
    }
    let _ = capabilities::validate_amount_capability_name(text);
    let _ = capabilities::validate_attribute_capability_name(text);
    let rev = openjd_model::SpecificationRevision::V2023_09;
    let exts = openjd_model::Extensions::new();
    if let Ok(std) = capabilities::standard_attribute_capabilities(rev, &exts) {
        for (name, _) in std {
            let _ = capabilities::validate_attribute_capability_value(name, text, std);
        }
        let _ = capabilities::validate_attribute_capability_value("attr.custom", text, std);
    }
    let _ = capabilities::standard_amount_capability_names(rev, &exts);
    if let Some(t) = JobParameterType::from_spec_str(text) {
        oracle!(
            JobParameterType::from_spec_str(t.as_spec_str()) == Some(t),
            "JobParameterType {t:?} does not round-trip through as_spec_str"
        );
        let _ = t.expr_type();
    }
    if let Some(t) = openjd_model::TaskParameterType::from_spec_str(text) {
        oracle!(
            openjd_model::TaskParameterType::from_spec_str(t.as_spec_str()) == Some(t),
            "TaskParameterType {t:?} does not round-trip"
        );
    }
    if let Ok(v) = TemplateSpecificationVersion::from_str(text) {
        oracle!(
            TemplateSpecificationVersion::from_str(v.as_str()).ok() == Some(v),
            "TemplateSpecificationVersion {v:?} does not round-trip"
        );
    }

    // let bindings over arbitrary text, with a small typed base table.
    let mut u = Unstructured::new(ctl);
    let mut base = SymbolTable::new();
    for _ in 0..upto(&mut u, 3) {
        let v = crate::gen::value::value(&mut u, PathFormat::Posix);
        let _ = base.set(&format!("Param.{}", gen::ident(&mut u)), v);
    }
    let bindings: Vec<String> = text.lines().take(8).map(str::to_string).collect();
    let _ = evaluate_let_bindings(
        &bindings,
        &base,
        None,
        PathFormat::Posix,
        Some(8 << 20),
        Some(200_000),
    );
}
