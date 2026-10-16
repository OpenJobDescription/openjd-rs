// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Signature-driven generator for well-typed expressions.
//!
//! The grammar is not hand-maintained: productions come from the default
//! [`FunctionLibrary`]'s registered signatures. To produce an expression of
//! type `T` the generator picks an overload whose return type matches `T`,
//! binds its type variables, and recursively generates each argument at the
//! parameter type. Dunder entries render as operator syntax (`__add__` →
//! `a + b`, `__getitem__` → `a[i:j:k]`, `__property_name__` → `p.name`), and
//! ordinary functions render as either `f(a, b)` or the method form
//! `a.f(b)`. A function added to the library is fuzzed with no change here.
//!
//! Because every production follows a registered signature, the output is
//! well-typed by construction (modulo deliberate noise), which is what makes
//! the type-soundness oracle in `targets::expr` meaningful: a static type
//! error on generated text means the checker rejected a well-typed program.

use super::value::{concrete_type, SCALAR_TYPES};
use super::{float, int, one_in, pick, small_int, string, upto};
use arbitrary::Unstructured;
use openjd_expr::{ExprProfile, ExprType, FunctionLibrary, HostContext, TypeCode};
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

/// A named symbol visible to the expression, with its declared type.
#[derive(Clone, Debug)]
pub struct Symbol {
    pub name: String,
    pub ty: ExprType,
}

/// `(function name, signature)` for every overload in the validation-time
/// library (host-context signatures included, so `apply_path_mapping` is
/// generated too).
fn signatures() -> &'static [(String, ExprType)] {
    static SIGS: OnceLock<Vec<(String, ExprType)>> = OnceLock::new();
    SIGS.get_or_init(|| {
        let lib = validation_library();
        let mut names: Vec<&str> = lib.function_names().collect();
        names.sort_unstable();
        let mut out = Vec::new();
        for n in names {
            for e in lib.get_signatures(n) {
                out.push((n.to_string(), e.signature.clone()));
            }
        }
        out
    })
}

/// The validation-time library: every function, host-context functions as
/// `Unresolved` stubs.
pub fn validation_library() -> Arc<FunctionLibrary> {
    FunctionLibrary::for_profile(&ExprProfile::latest().with_host_context(HostContext::Unresolved))
}

/// Every bare function, method, and property name the library knows, for
/// dictionaries and seed text.
pub fn library_tokens() -> Vec<String> {
    let mut v: Vec<String> = signatures()
        .iter()
        .map(|(n, _)| {
            n.strip_prefix("__property_")
                .and_then(|s| s.strip_suffix("__"))
                .unwrap_or(n)
                .to_string()
        })
        .filter(|n| !n.starts_with("__"))
        .collect();
    v.sort();
    v.dedup();
    v
}

const TYPE_VARS: &[TypeCode] = &[
    TypeCode::TypeVarT,
    TypeCode::TypeVarT1,
    TypeCode::TypeVarT2,
    TypeCode::TypeVarT3,
];

/// Generator state for one expression.
pub struct ExprGen<'a, 'b> {
    u: &'a mut Unstructured<'b>,
    symbols: &'a [Symbol],
    locals: Vec<Symbol>,
    budget: usize,
    next_local: usize,
    /// Set when the output used a signature whose return type variable no
    /// parameter binds (known issue `expr-unbound-return-typevar`): such a
    /// signature does not say what it returns, so the output is not
    /// well-typed by construction.
    pub used_unbound_return: bool,
    /// Set when the output uses `and` / `or` (known issue
    /// `expr-boolop-static-type`).
    pub used_boolop: bool,
    /// Set when the output calls a signature with a union-typed parameter
    /// or return, or omits an optional argument (known issues
    /// `expr-union-arg-overload`, `expr-flatten-union-static-type`).
    pub used_union: bool,
    /// Set when the output has a conditional (`a if c else b`).
    pub used_conditional: bool,
}

impl<'a, 'b> ExprGen<'a, 'b> {
    pub fn new(u: &'a mut Unstructured<'b>, symbols: &'a [Symbol]) -> Self {
        Self {
            u,
            symbols,
            locals: Vec::new(),
            budget: 48,
            next_local: 0,
            used_unbound_return: false,
            used_boolop: false,
            used_union: false,
            used_conditional: false,
        }
    }

    /// An expression of a random concrete type.
    pub fn any(&mut self) -> String {
        let t = if one_in(self.u, 3) {
            ExprType::ANY
        } else {
            concrete_type(self.u)
        };
        self.of_type(&t, 4)
    }

    /// An expression whose value should have type `ty`.
    pub fn of_type(&mut self, ty: &ExprType, depth: u32) -> String {
        let ty = self.concretize(ty);
        if depth == 0 || self.budget == 0 {
            return self.leaf(&ty);
        }
        self.budget -= 1;
        match upto(self.u, 9) {
            0..=1 => self.leaf(&ty),
            2..=6 => self.call(&ty, depth).unwrap_or_else(|| self.leaf(&ty)),
            7 => {
                self.used_conditional = true;
                let c = self.of_type(&ExprType::BOOL, depth - 1);
                let a = self.of_type(&ty, depth - 1);
                let b = self.of_type(&ty, depth - 1);
                format!("({a} if {c} else {b})")
            }
            _ => self.special(&ty, depth),
        }
    }

    /// Replace `any` / unions / type variables with one concrete choice.
    fn concretize(&mut self, ty: &ExprType) -> ExprType {
        match ty.code() {
            TypeCode::Any
            | TypeCode::TypeVarT
            | TypeCode::TypeVarT1
            | TypeCode::TypeVarT2
            | TypeCode::TypeVarT3 => concrete_type(self.u),
            TypeCode::Union if !ty.params().is_empty() => {
                let m = pick(self.u, ty.params()).clone();
                self.concretize(&m)
            }
            TypeCode::List => {
                let e = ty.params().first().cloned().unwrap_or(ExprType::INT);
                if e.is_concrete() {
                    ty.clone()
                } else {
                    let ce = pick(self.u, &SCALAR_TYPES[..5]).clone();
                    ExprType::list(ce)
                }
            }
            _ => ty.clone(),
        }
    }

    /// Productions that are syntax rather than library calls: boolean
    /// connectives, list literals, comprehensions.
    fn special(&mut self, ty: &ExprType, depth: u32) -> String {
        match ty.code() {
            TypeCode::Bool => {
                let a = self.of_type(&ExprType::BOOL, depth - 1);
                let b = self.of_type(&ExprType::BOOL, depth - 1);
                match upto(self.u, 2) {
                    0 => {
                        self.used_boolop = true;
                        format!("({a} and {b})")
                    }
                    1 => {
                        self.used_boolop = true;
                        format!("({a} or {b})")
                    }
                    _ => format!("(not {a})"),
                }
            }
            TypeCode::List => {
                let elem = ty.params().first().cloned().unwrap_or(ExprType::INT);
                if one_in(self.u, 2) {
                    let n = upto(self.u, 3);
                    let items: Vec<String> =
                        (0..n).map(|_| self.of_type(&elem, depth - 1)).collect();
                    format!("[{}]", items.join(", "))
                } else {
                    // [body for v in src if cond], v bound at a random type.
                    let vt = pick(self.u, &SCALAR_TYPES[..5]).clone();
                    let src = self.of_type(&ExprType::list(vt.clone()), depth - 1);
                    let v = format!("v{}", self.next_local);
                    self.next_local += 1;
                    self.locals.push(Symbol {
                        name: v.clone(),
                        ty: vt,
                    });
                    let body = self.of_type(&elem, depth - 1);
                    let cond = if one_in(self.u, 2) {
                        format!(" if {}", self.of_type(&ExprType::BOOL, depth - 1))
                    } else {
                        String::new()
                    };
                    self.locals.pop();
                    format!("[{body} for {v} in {src}{cond}]")
                }
            }
            _ => self.call(ty, depth).unwrap_or_else(|| self.leaf(ty)),
        }
    }

    /// A function or operator application whose return type matches `ty`.
    fn call(&mut self, ty: &ExprType, depth: u32) -> Option<String> {
        let candidates: Vec<&(String, ExprType)> = signatures()
            .iter()
            .filter(|(name, sig)| {
                let ret = sig.sig_return();
                if *ret == ExprType::NORETURN || name == "fail" {
                    return false;
                }
                ret.match_type(ty).is_some() || ty.satisfies(ret) || ret.satisfies(ty)
            })
            .collect();
        if candidates.is_empty() {
            return None;
        }
        let (name, sig) = (*pick(self.u, &candidates)).clone();
        if has_unbound_return(&sig) {
            self.used_unbound_return = true;
        }
        if sig
            .sig_params()
            .iter()
            .chain([sig.sig_return()])
            .any(has_union)
        {
            self.used_union = true;
        }
        let mut bindings: HashMap<TypeCode, ExprType> =
            sig.sig_return().match_type(ty).unwrap_or_default();
        // Unbound type variables: share one choice so `(list[T], T)` stays
        // consistent, which is the well-typed case.
        let shared = bindings
            .values()
            .next()
            .cloned()
            .unwrap_or_else(|| concrete_type(self.u));
        for code in TYPE_VARS {
            bindings.entry(*code).or_insert_with(|| shared.clone());
        }
        let args: Vec<String> = sig
            .sig_params()
            .iter()
            .map(|p| {
                let pt = p.substitute(&bindings);
                self.optional_arg(&pt, depth)
            })
            .collect();
        Some(self.render(&name, &args))
    }

    /// An argument: optional (`T?`) parameters are sometimes omitted (`None`).
    fn optional_arg(&mut self, ty: &ExprType, depth: u32) -> String {
        if ty.code() == TypeCode::Union
            && ty.params().contains(&ExprType::NULLTYPE)
            && one_in(self.u, 3)
        {
            self.used_union = true;
            return String::new();
        }
        self.of_type(ty, depth - 1)
    }

    fn render(&mut self, name: &str, a: &[String]) -> String {
        let arg = |i: usize| a.get(i).cloned().unwrap_or_default();
        let none_or = |s: String| if s.is_empty() { "None".to_string() } else { s };
        let bin = |op: &str| format!("({} {op} {})", none_or(arg(0)), none_or(arg(1)));
        match name {
            "__add__" => bin("+"),
            "__sub__" => bin("-"),
            "__mul__" => bin("*"),
            "__truediv__" => bin("/"),
            "__floordiv__" => bin("//"),
            "__mod__" => bin("%"),
            "__pow__" => bin("**"),
            "__eq__" => bin("=="),
            "__ne__" => bin("!="),
            "__lt__" => bin("<"),
            "__le__" => bin("<="),
            "__gt__" => bin(">"),
            "__ge__" => bin(">="),
            "__contains__" => format!("({} in {})", arg(1), arg(0)),
            "__not_contains__" => format!("({} not in {})", arg(1), arg(0)),
            "__neg__" => format!("(-{})", arg(0)),
            "__pos__" => format!("(+{})", arg(0)),
            "__not__" => format!("(not {})", arg(0)),
            "__getitem__" if a.len() == 2 => format!("({})[{}]", arg(0), arg(1)),
            "__getitem__" => {
                let step = if a.len() > 3 && !arg(3).is_empty() {
                    format!(":{}", arg(3))
                } else {
                    String::new()
                };
                format!("({})[{}:{}{step}]", arg(0), arg(1), arg(2))
            }
            n if n.starts_with("__property_") => {
                let prop = &n["__property_".len()..n.len() - 2];
                format!("({}).{prop}", arg(0))
            }
            n => {
                let args: Vec<String> = a.iter().cloned().map(none_or).collect();
                if !args.is_empty() && one_in(self.u, 2) {
                    format!("({}).{n}({})", args[0], args[1..].join(", "))
                } else {
                    format!("{n}({})", args.join(", "))
                }
            }
        }
    }

    /// A literal or symbol reference of type `ty`.
    fn leaf(&mut self, ty: &ExprType) -> String {
        let matching: Vec<String> = self
            .locals
            .iter()
            .chain(self.symbols.iter())
            .filter(|s| s.ty.satisfies(ty) || *ty == ExprType::ANY)
            .map(|s| s.name.clone())
            .collect();
        if !matching.is_empty() && !one_in(self.u, 3) {
            return pick(self.u, &matching).clone();
        }
        self.literal(ty)
    }

    fn literal(&mut self, ty: &ExprType) -> String {
        match ty.code() {
            TypeCode::Bool => pick(self.u, &["True", "False"]).to_string(),
            TypeCode::Int => {
                let v = if one_in(self.u, 3) {
                    int(self.u)
                } else {
                    small_int(self.u)
                };
                if v < 0 {
                    format!("({v})")
                } else {
                    v.to_string()
                }
            }
            TypeCode::Float => {
                let f = float(self.u);
                let s = format!("{f:?}");
                if s.starts_with('-') {
                    format!("({s})")
                } else {
                    s
                }
            }
            TypeCode::String => py_quote(&string(self.u)),
            TypeCode::Path => format!("path({})", py_quote(&super::path_string(self.u))),
            TypeCode::RangeExpr => {
                format!(
                    "range_expr({})",
                    py_quote(&super::value::range_text(self.u))
                )
            }
            TypeCode::NullType => "None".to_string(),
            TypeCode::List => {
                let elem = ty.params().first().cloned().unwrap_or(ExprType::INT);
                let n = upto(self.u, 3);
                let items: Vec<String> = (0..n).map(|_| self.leaf(&elem)).collect();
                format!("[{}]", items.join(", "))
            }
            _ => {
                let t = concrete_type(self.u);
                self.literal(&t)
            }
        }
    }
}

/// Does `t` contain a union type?
fn has_union(t: &ExprType) -> bool {
    t.code() == TypeCode::Union || t.params().iter().any(has_union)
}

/// Does `sig`'s return type use a type variable that no parameter binds?
fn has_unbound_return(sig: &ExprType) -> bool {
    fn vars(t: &ExprType, out: &mut Vec<TypeCode>) {
        if TYPE_VARS.contains(&t.code()) {
            out.push(t.code());
        }
        for p in t.params() {
            vars(p, out);
        }
    }
    let mut bound = Vec::new();
    for p in sig.sig_params() {
        vars(p, &mut bound);
    }
    let mut ret = Vec::new();
    vars(sig.sig_return(), &mut ret);
    ret.iter().any(|v| !bound.contains(v))
}

/// Render `s` as a Python string literal.
pub fn py_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                out.push_str(&format!("\\x{:02x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('\'');
    out
}
