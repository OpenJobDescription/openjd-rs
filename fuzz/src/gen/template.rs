// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Job and environment template generator.
//!
//! Produces a `serde_json::Value` shaped like a 2023-09 template: every job
//! parameter type with defaults and constraints, task parameter spaces with
//! range expressions, chunking, and combination expressions, `let` bindings,
//! embedded files, environments with wrap hooks, host requirements, step
//! dependencies, and SimpleAction sugar. Format-string fields interpolate
//! well-typed expressions from [`super::expr::ExprGen`] over the symbols
//! actually in scope, so most generated templates get through decode and
//! exercise job creation. A minority are perturbed (wrong types, missing
//! fields, out-of-range values) to keep the validator's reject paths covered.

use super::expr::{py_quote, ExprGen, Symbol};
use super::value::range_text;
use super::{flip, float, ident, int, one_in, path_string, pick, small_int, string, upto};
use arbitrary::Unstructured;
use openjd_expr::ExprType;
use serde_json::{json, Map, Value};

/// A job parameter the generator declared, for building input values.
#[derive(Clone, Debug)]
pub struct ParamInfo {
    pub name: String,
    /// The spec `type` string (`INT`, `LIST[PATH]`, ...).
    pub spec_type: &'static str,
    pub ty: ExprType,
}

/// Generator output: the template document plus what it declared.
pub struct Generated {
    pub doc: Value,
    pub params: Vec<ParamInfo>,
}

/// `(spec type string, constructor for its expression type)`.
type ParamType = (&'static str, fn() -> ExprType);

const PARAM_TYPES: &[ParamType] = &[
    ("STRING", || ExprType::STRING),
    ("INT", || ExprType::INT),
    ("FLOAT", || ExprType::FLOAT),
    ("PATH", || ExprType::PATH),
    ("BOOL", || ExprType::BOOL),
    ("RANGE_EXPR", || ExprType::RANGE_EXPR),
    ("LIST[STRING]", || ExprType::list(ExprType::STRING)),
    ("LIST[INT]", || ExprType::list(ExprType::INT)),
    ("LIST[FLOAT]", || ExprType::list(ExprType::FLOAT)),
    ("LIST[PATH]", || ExprType::list(ExprType::PATH)),
    ("LIST[BOOL]", || ExprType::list(ExprType::BOOL)),
    ("LIST[LIST[INT]]", || {
        ExprType::list(ExprType::list(ExprType::INT))
    }),
];

const EXTENSIONS: &[&str] = &[
    "EXPR",
    "TASK_CHUNKING",
    "REDACTED_ENV_VARS",
    "FEATURE_BUNDLE_1",
    "WRAP_ACTIONS",
];

/// A JSON value for a default / input of type `ty` (well-typed most of the
/// time, deliberately off-type occasionally).
pub fn json_value_of(u: &mut Unstructured<'_>, ty: &ExprType) -> Value {
    if one_in(u, 20) {
        return pick(
            u,
            &[
                json!(null),
                json!("x"),
                json!(1),
                json!([]),
                json!({}),
                json!(1.5),
            ],
        )
        .clone();
    }
    if let Some(elem) = ty.list_element_type() {
        let n = upto(u, 4);
        return Value::Array((0..n).map(|_| json_value_of(u, elem)).collect());
    }
    match ty.to_string().as_str() {
        "int" => {
            if one_in(u, 4) {
                json!(int(u).to_string())
            } else {
                json!(small_int(u))
            }
        }
        "float" => {
            let f = float(u);
            if one_in(u, 4) {
                json!(f.to_string())
            } else {
                json!(f)
            }
        }
        "bool" => json!(flip(u)),
        "path" => json!(path_string(u)),
        "range_expr" => json!(range_text(u)),
        _ => json!(string(u)),
    }
}

struct Ctx<'a, 'b> {
    u: &'a mut Unstructured<'b>,
    exts: Vec<&'static str>,
    symbols: Vec<Symbol>,
}

impl Ctx<'_, '_> {
    fn has(&self, ext: &str) -> bool {
        self.exts.contains(&ext)
    }

    /// A format string over the current scope.
    fn fmt(&mut self, want: &ExprType) -> String {
        let n = upto(self.u, 3);
        if n == 0 {
            return string(self.u).replace("{{", "{").replace("}}", "}");
        }
        let mut out = String::new();
        for i in 0..n {
            if i > 0 || one_in(self.u, 2) {
                out.push_str(pick(self.u, &["", "-", " ", "/", "x", "é"]));
            }
            let expr = if !self.has("EXPR") || one_in(self.u, 3) {
                // Base-spec interpolation: a bare symbol reference.
                if self.symbols.is_empty() {
                    // Nothing in scope: a literal (an undefined reference
                    // would only test that rejection, which the text
                    // targets already cover).
                    "'x'".to_string()
                } else {
                    pick(self.u, &self.symbols).name.clone()
                }
            } else {
                let syms = self.symbols.clone();
                let mut g = ExprGen::new(self.u, &syms);
                if n == 1 {
                    g.of_type(want, 3)
                } else {
                    g.any()
                }
            };
            out.push_str("{{ ");
            out.push_str(&expr);
            out.push_str(" }}");
        }
        out
    }

    fn param_def(&mut self, name: &str) -> (Value, ParamInfo) {
        let allowed: Vec<_> = PARAM_TYPES
            .iter()
            .filter(|(t, _)| self.has("EXPR") || matches!(*t, "STRING" | "INT" | "FLOAT" | "PATH"))
            .collect();
        let (spec_type, mk) = **pick(self.u, &allowed);
        let ty = mk();
        let mut m = Map::new();
        m.insert("name".into(), json!(name));
        m.insert("type".into(), json!(spec_type));
        if !one_in(self.u, 4) {
            let v = json_value_of(self.u, &ty);
            m.insert("default".into(), v);
        }
        match spec_type {
            "INT" | "FLOAT" | "LIST[INT]" | "LIST[FLOAT]" => {
                if one_in(self.u, 3) {
                    m.insert("minValue".into(), json!(small_int(self.u)));
                }
                if one_in(self.u, 3) {
                    m.insert("maxValue".into(), json!(small_int(self.u)));
                }
            }
            _ => {}
        }
        if matches!(spec_type, "STRING" | "INT" | "FLOAT") && one_in(self.u, 4) {
            let n = upto(self.u, 3) + 1;
            let vals: Vec<Value> = (0..n).map(|_| json_value_of(self.u, &ty)).collect();
            m.insert("allowedValues".into(), Value::Array(vals));
        }
        if spec_type.starts_with("LIST") || spec_type == "STRING" {
            if one_in(self.u, 4) {
                m.insert("minLength".into(), json!(upto(self.u, 3)));
            }
            if one_in(self.u, 4) {
                m.insert("maxLength".into(), json!(upto(self.u, 6)));
            }
        }
        if spec_type == "PATH" && one_in(self.u, 3) {
            m.insert(
                "objectType".into(),
                json!(*pick(self.u, &["FILE", "DIRECTORY"])),
            );
            m.insert(
                "dataFlow".into(),
                json!(*pick(self.u, &["NONE", "IN", "OUT", "INOUT"])),
            );
        }
        if one_in(self.u, 5) {
            m.insert(
                "userInterface".into(),
                json!({"control": *pick(self.u, &["LINE_EDIT", "HIDDEN", "DROPDOWN_LIST", "CHECK_BOX"]), "label": "L"}),
            );
        }
        (
            Value::Object(m),
            ParamInfo {
                name: name.to_string(),
                spec_type,
                ty,
            },
        )
    }

    fn action(&mut self) -> Value {
        let mut m = Map::new();
        let cmd = self.fmt(&ExprType::STRING);
        m.insert(
            "command".into(),
            json!(if cmd.is_empty() {
                "echo".to_string()
            } else {
                cmd
            }),
        );
        if !one_in(self.u, 3) {
            let n = upto(self.u, 3);
            let args: Vec<Value> = (0..n)
                .map(|_| {
                    let t = if one_in(self.u, 3) {
                        ExprType::list(ExprType::STRING)
                    } else {
                        ExprType::STRING
                    };
                    json!(self.fmt(&t))
                })
                .collect();
            // `args: []` is invalid ("if provided, must not be empty").
            if !args.is_empty() {
                m.insert("args".into(), Value::Array(args));
            }
        }
        if one_in(self.u, 4) {
            let t = if one_in(self.u, 2) {
                json!(upto(self.u, 600) as i64 - 1)
            } else {
                json!(self.fmt(&ExprType::INT))
            };
            m.insert("timeout".into(), t);
        }
        if one_in(self.u, 4) {
            let c = if flip(self.u) {
                json!({"mode": "TERMINATE"})
            } else {
                json!({"mode": "NOTIFY_THEN_TERMINATE", "notifyPeriodInSeconds": small_int(self.u)})
            };
            m.insert("cancelation".into(), c);
        }
        Value::Object(m)
    }

    fn embedded_files(&mut self, scope: &str) -> Option<Value> {
        if !one_in(self.u, 3) {
            return None;
        }
        let n = upto(self.u, 2) + 1;
        let mut files = Vec::new();
        for _ in 0..n {
            let name = ident(self.u);
            let mut m = Map::new();
            m.insert("name".into(), json!(name));
            m.insert("type".into(), json!("TEXT"));
            if !one_in(self.u, 3) {
                let f = if one_in(self.u, 3) {
                    string(self.u)
                } else {
                    format!("{}.{}", ident(self.u), pick(self.u, &["sh", "py", "txt"]))
                };
                m.insert("filename".into(), json!(f));
            }
            let data = self.fmt(&ExprType::STRING);
            m.insert("data".into(), json!(data));
            if one_in(self.u, 3) {
                m.insert("runnable".into(), json!(flip(self.u)));
            }
            if self.has("FEATURE_BUNDLE_1") && one_in(self.u, 3) {
                m.insert(
                    "endOfLine".into(),
                    json!(*pick(self.u, &["LF", "CRLF", "AUTO"])),
                );
            }
            files.push(Value::Object(m));
            self.symbols.push(Symbol {
                name: format!("{scope}.File.{name}"),
                ty: ExprType::PATH,
            });
        }
        Some(Value::Array(files))
    }

    fn lets(&mut self) -> Option<Value> {
        if !self.has("EXPR") || !one_in(self.u, 3) {
            return None;
        }
        let n = upto(self.u, 2) + 1;
        let mut out = Vec::new();
        for _ in 0..n {
            let name = format!("l{}", ident(self.u).to_lowercase());
            let ty = super::value::concrete_type(self.u);
            let syms = self.symbols.clone();
            let mut g = ExprGen::new(self.u, &syms);
            let e = g.of_type(&ty, 3);
            out.push(json!(format!("{name} = {e}")));
            self.symbols.push(Symbol { name, ty });
        }
        Some(Value::Array(out))
    }

    fn environment(&mut self, name: &str) -> Value {
        let mark = self.symbols.len();
        let mut m = Map::new();
        m.insert("name".into(), json!(name));
        if one_in(self.u, 2) {
            let n = upto(self.u, 3);
            let mut vars = Map::new();
            for _ in 0..n {
                let k = format!("{}_{}", ident(self.u).to_uppercase(), upto(self.u, 9));
                vars.insert(k, json!(self.fmt(&ExprType::STRING)));
            }
            m.insert("variables".into(), Value::Object(vars));
        }
        if !one_in(self.u, 3) || !m.contains_key("variables") {
            let mut script = Map::new();
            if let Some(l) = self.lets() {
                script.insert("let".into(), l);
            }
            if let Some(f) = self.embedded_files("Env") {
                script.insert("embeddedFiles".into(), f);
            }
            let mut actions = Map::new();
            if !one_in(self.u, 4) {
                actions.insert("onEnter".into(), self.action());
            }
            if one_in(self.u, 2) {
                actions.insert("onExit".into(), self.action());
            }
            if self.has("WRAP_ACTIONS") && one_in(self.u, 2) {
                let hook = *pick(
                    self.u,
                    &["onWrapEnvEnter", "onWrapTaskRun", "onWrapEnvExit"],
                );
                self.symbols.push(Symbol {
                    name: "WrappedAction.Command".into(),
                    ty: ExprType::STRING,
                });
                actions.insert(hook.into(), self.action());
            }
            if actions.is_empty() {
                actions.insert("onEnter".into(), self.action());
            }
            script.insert("actions".into(), Value::Object(actions));
            m.insert("script".into(), Value::Object(script));
        }
        self.symbols.truncate(mark);
        Value::Object(m)
    }

    fn param_space(&mut self) -> Option<Value> {
        if one_in(self.u, 3) {
            return None;
        }
        let n = upto(self.u, 2) + 1;
        let mut defs = Vec::new();
        let mut names = Vec::new();
        for i in 0..n {
            let name = format!("{}{i}", ident(self.u));
            let kinds: &[&str] = if self.has("TASK_CHUNKING") && i == 0 {
                &["INT", "FLOAT", "STRING", "PATH", "CHUNK[INT]"]
            } else {
                &["INT", "FLOAT", "STRING", "PATH"]
            };
            let kind = *pick(self.u, kinds);
            let (range, ty) = match kind {
                "INT" | "CHUNK[INT]" => {
                    let r = match upto(self.u, 2) {
                        0 => json!(range_text(self.u)),
                        1 => {
                            let k = upto(self.u, 4) + 1;
                            json!((0..k).map(|_| small_int(self.u)).collect::<Vec<_>>())
                        }
                        _ => json!(self.fmt(&ExprType::RANGE_EXPR)),
                    };
                    let ty = if kind == "INT" {
                        ExprType::INT
                    } else {
                        ExprType::RANGE_EXPR
                    };
                    (r, ty)
                }
                "FLOAT" => {
                    let k = upto(self.u, 3) + 1;
                    (
                        json!((0..k).map(|_| float(self.u)).collect::<Vec<_>>()),
                        ExprType::FLOAT,
                    )
                }
                "PATH" => {
                    let k = upto(self.u, 3) + 1;
                    let r: Vec<Value> = (0..k).map(|_| json!(path_string(self.u))).collect();
                    (Value::Array(r), ExprType::PATH)
                }
                _ => {
                    let k = upto(self.u, 3) + 1;
                    let r: Vec<Value> =
                        (0..k).map(|_| json!(self.fmt(&ExprType::STRING))).collect();
                    (Value::Array(r), ExprType::STRING)
                }
            };
            let mut d = json!({"name": name, "type": kind, "range": range});
            if kind == "CHUNK[INT]" {
                let mut chunks = json!({
                    "defaultTaskCount": if one_in(self.u, 3) { json!(self.fmt(&ExprType::INT)) } else { json!(small_int(self.u)) },
                    "rangeConstraint": *pick(self.u, &["CONTIGUOUS", "NONCONTIGUOUS"]),
                });
                if one_in(self.u, 3) {
                    chunks["targetRuntimeSeconds"] = json!(small_int(self.u));
                }
                d["chunks"] = chunks;
            }
            defs.push(d);
            names.push(name.clone());
            self.symbols.push(Symbol {
                name: format!("Task.Param.{name}"),
                ty,
            });
            self.symbols.push(Symbol {
                name: format!("Task.RawParam.{name}"),
                ty: ExprType::STRING,
            });
        }
        let mut space = json!({"taskParameterDefinitions": defs});
        if names.len() > 1 && !one_in(self.u, 3) {
            let combo = if flip(self.u) {
                names.join(" * ")
            } else {
                format!("({})", names.join(", "))
            };
            space["combination"] = json!(combo);
        }
        Some(space)
    }

    fn host_requirements(&mut self) -> Option<Value> {
        if !one_in(self.u, 4) {
            return None;
        }
        let amount = *pick(
            self.u,
            &[
                "amount.worker.vcpu",
                "amount.worker.memory",
                "amount.worker.gpu",
                "amount.custom.x",
                "amount.bad",
            ],
        );
        let attr = *pick(
            self.u,
            &[
                "attr.worker.os.family",
                "attr.worker.cpu.arch",
                "attr.custom.y",
            ],
        );
        let val = *pick(
            self.u,
            &["linux", "windows", "macos", "x86_64", "arm64", "Other", ""],
        );
        // A non-negative number (a format string is not accepted here).
        let min = json!(small_int(self.u).unsigned_abs());
        Some(json!({
            "amounts": [{"name": amount, "min": min}],
            "attributes": [{"name": attr, "anyOf": [val]}],
        }))
    }

    fn step(&mut self, name: &str, prior: &[String]) -> Value {
        let mark = self.symbols.len();
        let mut m = Map::new();
        m.insert("name".into(), json!(name));
        self.symbols.push(Symbol {
            name: "Step.Name".into(),
            ty: ExprType::STRING,
        });
        if let Some(ps) = self.param_space() {
            m.insert("parameterSpace".into(), ps);
        }
        if !prior.is_empty() && one_in(self.u, 2) {
            let dep = pick(self.u, prior).clone();
            m.insert("dependencies".into(), json!([{"dependsOn": dep}]));
        }
        if let Some(h) = self.host_requirements() {
            m.insert("hostRequirements".into(), h);
        }
        if one_in(self.u, 4) {
            let env_name = format!("StepEnv{}", ident(self.u));
            let e = self.environment(&env_name);
            m.insert("stepEnvironments".into(), json!([e]));
        }
        if self.has("FEATURE_BUNDLE_1") && one_in(self.u, 3) {
            let kind = *pick(self.u, &["bash", "python", "cmd", "powershell", "node"]);
            let mut sa = json!({"script": self.fmt(&ExprType::STRING)});
            if one_in(self.u, 2) {
                sa["args"] = json!([self.fmt(&ExprType::STRING)]);
            }
            m.insert(kind.into(), sa);
        } else {
            let mut script = Map::new();
            if let Some(l) = self.lets() {
                script.insert("let".into(), l);
            }
            if let Some(f) = self.embedded_files("Task") {
                script.insert("embeddedFiles".into(), f);
            }
            script.insert("actions".into(), json!({"onRun": self.action()}));
            m.insert("script".into(), Value::Object(script));
        }
        self.symbols.truncate(mark);
        Value::Object(m)
    }
}

/// Generate a job template.
pub fn job_template(u: &mut Unstructured<'_>) -> Generated {
    let mut exts: Vec<&'static str> = EXTENSIONS
        .iter()
        .copied()
        .filter(|_| !one_in(u, 3))
        .collect();
    if one_in(u, 6) {
        exts.clear();
    }
    let mut ctx = Ctx {
        u,
        exts,
        symbols: vec![Symbol {
            name: "Session.WorkingDirectory".into(),
            ty: ExprType::PATH,
        }],
    };
    let mut doc = Map::new();
    doc.insert("specificationVersion".into(), json!("jobtemplate-2023-09"));
    if !ctx.exts.is_empty() {
        doc.insert("extensions".into(), json!(ctx.exts.clone()));
    }
    let n_params = upto(ctx.u, 4);
    let mut defs = Vec::new();
    let mut params = Vec::new();
    for i in 0..n_params {
        let name = format!(
            "{}{}",
            ident(ctx.u),
            if one_in(ctx.u, 2) {
                i.to_string()
            } else {
                String::new()
            }
        );
        let (d, info) = ctx.param_def(&name);
        ctx.symbols.push(Symbol {
            name: format!("Param.{name}"),
            ty: info.ty.clone(),
        });
        ctx.symbols.push(Symbol {
            name: format!("RawParam.{name}"),
            ty: ExprType::STRING,
        });
        defs.push(d);
        params.push(info);
    }
    if !defs.is_empty() {
        doc.insert("parameterDefinitions".into(), Value::Array(defs));
    }
    // The job name is template scope: no Session.* symbols. Control
    // characters and names over 128 characters are invalid, so they would
    // only test that rejection.
    let session_syms = std::mem::take(&mut ctx.symbols);
    ctx.symbols = session_syms
        .iter()
        .filter(|s| !s.name.starts_with("Session."))
        .cloned()
        .collect();
    let name: String = ctx
        .fmt(&ExprType::STRING)
        .chars()
        .filter(|c| !c.is_control())
        .collect();
    ctx.symbols = session_syms;
    doc.insert(
        "name".into(),
        json!(if name.is_empty() || name.chars().count() > 128 {
            "Job".to_string()
        } else {
            name
        }),
    );
    ctx.symbols.push(Symbol {
        name: "Job.Name".into(),
        ty: ExprType::STRING,
    });
    if one_in(ctx.u, 3) {
        let n = upto(ctx.u, 1) + 1;
        let envs: Vec<Value> = (0..n)
            .map(|i| ctx.environment(&format!("JobEnv{i}")))
            .collect();
        doc.insert("jobEnvironments".into(), Value::Array(envs));
    }
    let n_steps = upto(ctx.u, 2) + 1;
    let mut steps = Vec::new();
    let mut names: Vec<String> = Vec::new();
    for i in 0..n_steps {
        let sname = if one_in(ctx.u, 8) && !names.is_empty() {
            names[0].clone()
        } else {
            format!("Step{i}")
        };
        steps.push(ctx.step(&sname, &names));
        names.push(sname);
    }
    doc.insert("steps".into(), Value::Array(steps));
    let mut doc = Value::Object(doc);
    if one_in(ctx.u, 10) {
        perturb(ctx.u, &mut doc);
    }
    // A description for about a third of templates, derived from the
    // document instead of drawn from the input, so it consumes no bytes and
    // leaves every later draw (and the generated reproducers in
    // regressions/) unchanged.
    let h = {
        use std::hash::{Hash, Hasher};
        let mut s = std::collections::hash_map::DefaultHasher::new();
        doc.to_string().hash(&mut s);
        s.finish()
    };
    if h % 3 == 0 {
        if let Value::Object(o) = &mut doc {
            o.insert("description".into(), json!(format!("Job {h:x}")));
        }
    }
    Generated { doc, params }
}

/// Generate an environment template.
pub fn environment_template(u: &mut Unstructured<'_>) -> Generated {
    let exts: Vec<&'static str> = EXTENSIONS.iter().copied().filter(|_| flip(u)).collect();
    let mut ctx = Ctx {
        u,
        exts,
        symbols: vec![Symbol {
            name: "Session.WorkingDirectory".into(),
            ty: ExprType::PATH,
        }],
    };
    let mut params = Vec::new();
    let mut defs = Vec::new();
    for _ in 0..upto(ctx.u, 2) {
        let name = ident(ctx.u);
        let (d, info) = ctx.param_def(&name);
        ctx.symbols.push(Symbol {
            name: format!("Param.{name}"),
            ty: info.ty.clone(),
        });
        defs.push(d);
        params.push(info);
    }
    let env = ctx.environment("TemplateEnv");
    let mut doc = json!({
        "specificationVersion": "environment-2023-09",
        "environment": env,
    });
    if !ctx.exts.is_empty() {
        doc["extensions"] = json!(ctx.exts.clone());
    }
    if !defs.is_empty() {
        doc["parameterDefinitions"] = Value::Array(defs);
    }
    Generated { doc, params }
}

/// Replace one random node of the document with a wrong-typed value.
fn perturb(u: &mut Unstructured<'_>, v: &mut Value) {
    let replacement = pick(
        u,
        &[
            json!(null),
            json!(""),
            json!(-1),
            json!(1e300),
            json!([]),
            json!({}),
            json!("{{"),
            json!("{{ Param.Nope }}"),
            json!(py_quote("x")),
        ],
    )
    .clone();
    replace_somewhere(u, v, replacement);
}

fn replace_somewhere(u: &mut Unstructured<'_>, cur: &mut Value, replacement: Value) {
    let descend = !one_in(u, 3);
    match cur {
        Value::Object(m) if descend && !m.is_empty() => {
            let keys: Vec<String> = m.keys().cloned().collect();
            let k = pick(u, &keys).clone();
            if let Some(n) = m.get_mut(&k) {
                return replace_somewhere(u, n, replacement);
            }
        }
        Value::Array(a) if descend && !a.is_empty() => {
            let i = u.choose_index(a.len()).unwrap_or(0);
            return replace_somewhere(u, &mut a[i], replacement);
        }
        _ => {}
    }
    *cur = replacement;
}
