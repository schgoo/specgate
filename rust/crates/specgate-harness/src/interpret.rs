//! Symbolic Rust expression interpreter for fixture sources.
//!
//! This is intentionally narrow: it understands the patterns the
//! fixtures use (struct literals, integer/string operations, simple
//! method calls, mocks via `#[spec_mock]` lets, and the `format!`,
//! `panic!`, `spec_event!` macros) and nothing more. Anything outside
//! that envelope yields an interpreter error which surfaces as a
//! failed case.

use crate::discover::{
    FnDef, MethodDef, Module, Param, ReturnKind, expr_str_lit, name_from_attr, stmt_mock_name,
};
use crate::spec::{Case, Setup};
use crate::types::TraceEvent;
use serde_yaml::Value as YamlVal;
use std::collections::{BTreeMap, BTreeSet};
use syn::{Expr, Lit, Pat, Stmt, UnOp};

#[derive(Debug, Clone)]
pub enum Value {
    Int(i64),
    Str(String),
    Bool(bool),
    Unit,
    Instance(String), // alias name -> instance in ctx.instances
    StructLit { ty: String, fields: BTreeMap<String, Value> },
    ResultOk(Box<Value>),
    ResultErr(Box<Value>),
}

impl Value {
    pub fn display(&self) -> String {
        match self {
            Value::Int(n) => n.to_string(),
            Value::Str(s) => s.clone(),
            Value::Bool(b) => b.to_string(),
            Value::Unit => String::new(),
            Value::Instance(n) => n.clone(),
            Value::StructLit { ty, .. } => ty.clone(),
            Value::ResultOk(v) => v.display(),
            Value::ResultErr(v) => v.display(),
        }
    }
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct Instance {
    pub type_name: String,
    pub fields: BTreeMap<String, Value>,
    pub tracked: BTreeSet<String>,
    pub alias_prefix: String, // empty for single-setup case, e.g. "source" for multi
}

pub struct Ctx<'a> {
    pub module: &'a Module,
    pub instances: BTreeMap<String, Instance>, // alias -> instance
    pub mocks: BTreeMap<String, BTreeMap<String, String>>,
    pub locals: Vec<BTreeMap<String, Value>>,
    pub traces: Vec<TraceEvent>,
    pub current_self: Option<String>, // alias of `self` in the current method
    pub aborted: bool,
    pub panic_msg: Option<String>,
}

#[derive(Debug)]
pub enum InterpError {
    #[allow(dead_code)]
    Unsupported(String),
    #[allow(dead_code)]
    UnknownName(String),
    #[allow(dead_code)]
    TypeError(String),
    #[allow(dead_code)]
    Panic(String),
}

pub fn run_case(module: &Module, case: &Case) -> Result<Vec<TraceEvent>, InterpError> {
    let mut ctx = Ctx {
        module,
        instances: BTreeMap::new(),
        mocks: BTreeMap::new(),
        locals: vec![BTreeMap::new()],
        traces: Vec::new(),
        current_self: None,
        aborted: false,
        panic_msg: None,
    };

    // Pull mock tables out of inputs (any input whose value is a mapping).
    for (k, v) in &case.inputs {
        if let YamlVal::Mapping(m) = v {
            let mut tab = BTreeMap::new();
            for (mk, mv) in m {
                if let (Some(ks), Some(vs)) = (mk.as_str(), mv.as_str()) {
                    tab.insert(ks.to_string(), vs.to_string());
                }
            }
            ctx.mocks.insert(k.clone(), tab);
        }
    }

    // Run setup(s).
    match &case.setup {
        Setup::None => {}
        Setup::Single(name) => {
            run_setup(&mut ctx, name, "", &case.inputs)?;
        }
        Setup::Multi(entries) => {
            for (alias, fn_name) in entries {
                run_setup(&mut ctx, fn_name, alias, &case.inputs)?;
            }
        }
    }

    // Run operation or steps.
    let ops: Vec<&str> = if !case.steps.is_empty() {
        case.steps.iter().map(String::as_str).collect()
    } else if let Some(op) = case.operation.as_deref() {
        vec![op]
    } else {
        vec![]
    };

    for op in ops {
        run_operation(&mut ctx, op, &case.inputs)?;
        if ctx.aborted {
            break;
        }
    }

    Ok(ctx.traces)
}

fn run_setup(
    ctx: &mut Ctx,
    setup_name: &str,
    alias: &str,
    case_inputs: &BTreeMap<String, YamlVal>,
) -> Result<(), InterpError> {
    let setup = ctx
        .module
        .setups
        .get(setup_name)
        .ok_or_else(|| InterpError::UnknownName(format!("setup {setup_name}")))?
        .clone();

    // Bind parameters from inputs.
    let mut scope = BTreeMap::new();
    for p in &setup.params {
        let val = case_inputs
            .get(&p.name)
            .map(|v| yaml_to_value(v))
            .unwrap_or(Value::Unit);
        // Emit setup param event for setup-with-params fixture (only when params exist).
        ctx.traces.push(TraceEvent::Event {
            name: format!("{}.{}", setup_name, p.name),
            value: val.display(),
        });
        scope.insert(p.name.clone(), val);
    }
    ctx.locals.push(scope);

    // Evaluate body — should yield a struct literal.
    let result = eval_block(ctx, &setup.body)?;
    ctx.locals.pop();

    let (ty, fields) = match result {
        Value::StructLit { ty, fields } => (ty, fields),
        other => {
            return Err(InterpError::TypeError(format!(
                "setup {setup_name} did not return a struct: {other:?}"
            )));
        }
    };

    let tracked = ctx
        .module
        .structs
        .get(&ty)
        .map(|s| s.tracked.clone())
        .unwrap_or_default();
    let field_order = ctx
        .module
        .structs
        .get(&ty)
        .map(|s| s.fields.clone())
        .unwrap_or_else(|| fields.keys().cloned().collect());

    // Emit initial tracked field events in declaration order.
    for f in &field_order {
        if tracked.contains(f) {
            if let Some(v) = fields.get(f) {
                let name = if alias.is_empty() {
                    f.clone()
                } else {
                    format!("{alias}.{f}")
                };
                ctx.traces.push(TraceEvent::Event {
                    name,
                    value: v.display(),
                });
            }
        }
    }

    let inst = Instance {
        type_name: ty,
        fields,
        tracked,
        alias_prefix: alias.to_string(),
    };
    ctx.instances.insert(alias.to_string(), inst);
    Ok(())
}

fn run_operation(
    ctx: &mut Ctx,
    op_name: &str,
    case_inputs: &BTreeMap<String, YamlVal>,
) -> Result<(), InterpError> {
    if let Some(method) = ctx.module.method_ops.get(op_name).cloned() {
        return run_method_op(ctx, op_name, &method, case_inputs);
    }
    if let Some(free) = ctx.module.free_ops.get(op_name).cloned() {
        return run_free_op(ctx, op_name, &free, case_inputs);
    }
    Err(InterpError::UnknownName(format!("operation {op_name}")))
}

fn run_method_op(
    ctx: &mut Ctx,
    op_name: &str,
    method: &MethodDef,
    case_inputs: &BTreeMap<String, YamlVal>,
) -> Result<(), InterpError> {
    // Method always emits Run.
    ctx.traces.push(TraceEvent::Run {
        operation: op_name.to_string(),
    });

    // Bind self to single instance under "" alias.
    let self_alias = "".to_string();
    if !ctx.instances.contains_key(&self_alias) {
        return Err(InterpError::TypeError(
            "method op without setup not supported".into(),
        ));
    }

    // Bind non-self params from case inputs.
    let mut scope = BTreeMap::new();
    for p in &method.params {
        let v = case_inputs
            .get(&p.name)
            .map(yaml_to_value)
            .unwrap_or(Value::Unit);
        scope.insert(p.name.clone(), v);
    }
    ctx.locals.push(scope);

    let prev_self = ctx.current_self.replace(self_alias.clone());

    let body_clone = method.body.clone();
    let body_result = eval_block(ctx, &body_clone);

    ctx.current_self = prev_self;
    ctx.locals.pop();

    finalize_op_result(ctx, op_name, body_result, method.return_kind);
    Ok(())
}

fn run_free_op(
    ctx: &mut Ctx,
    op_name: &str,
    op: &FnDef,
    case_inputs: &BTreeMap<String, YamlVal>,
) -> Result<(), InterpError> {
    let has_ref = op.params.iter().any(|p| p.is_reference);
    let is_result = op.return_kind == ReturnKind::Result;

    // Free fn with Result return: skip Run + params (per the harness spec's
    // explicit traces field for result_ok / result_err).
    if !is_result {
        ctx.traces.push(TraceEvent::Run {
            operation: op_name.to_string(),
        });
    }

    let mut scope = BTreeMap::new();
    for p in &op.params {
        // Reference-typed params: prefer alias-bound instance, but fall back
        // to a value from `case_inputs` (e.g. `data: &str`).
        let v = if p.is_reference {
            if ctx.instances.contains_key(&p.name) {
                Value::Instance(p.name.clone())
            } else {
                case_inputs
                    .get(&p.name)
                    .map(yaml_to_value)
                    .unwrap_or(Value::Unit)
            }
        } else {
            case_inputs
                .get(&p.name)
                .map(yaml_to_value)
                .unwrap_or(Value::Unit)
        };

        // Emit param events only for free fns without Result return AND no ref params.
        if !is_result && !has_ref && !p.is_reference {
            ctx.traces.push(TraceEvent::Event {
                name: format!("{}.{}", op_name, p.name),
                value: v.display(),
            });
        }
        scope.insert(p.name.clone(), v);
    }
    ctx.locals.push(scope);

    let body_clone = op.body.clone();
    let body_result = eval_block(ctx, &body_clone);
    ctx.locals.pop();

    finalize_op_result(ctx, op_name, body_result, op.return_kind);
    Ok(())
}

fn finalize_op_result(
    ctx: &mut Ctx,
    op_name: &str,
    body_result: Result<Value, InterpError>,
    rk: ReturnKind,
) {
    if let Some(msg) = ctx.panic_msg.take() {
        ctx.traces.push(TraceEvent::Event {
            name: format!("{op_name}.outcome"),
            value: "Unrecoverable".into(),
        });
        ctx.traces.push(TraceEvent::Event {
            name: format!("{op_name}.error"),
            value: msg,
        });
        ctx.aborted = true;
        return;
    }
    if ctx.aborted {
        return; // mock failure, no result events
    }
    match body_result {
        Err(InterpError::Panic(msg)) => {
            ctx.traces.push(TraceEvent::Event {
                name: format!("{op_name}.outcome"),
                value: "Unrecoverable".into(),
            });
            ctx.traces.push(TraceEvent::Event {
                name: format!("{op_name}.error"),
                value: msg,
            });
            ctx.aborted = true;
        }
        Err(_) | Ok(Value::Unit) if rk == ReturnKind::Unit => {
            // Void op: no result event.
        }
        Ok(v) => match rk {
            ReturnKind::Result => {
                let (oc, key, inner) = match v {
                    Value::ResultOk(inner) => ("Ok", "result", *inner),
                    Value::ResultErr(inner) => ("Error", "error", *inner),
                    other => ("Ok", "result", other),
                };
                ctx.traces.push(TraceEvent::Event {
                    name: format!("{op_name}.outcome"),
                    value: oc.into(),
                });
                ctx.traces.push(TraceEvent::Event {
                    name: format!("{op_name}.{key}"),
                    value: inner.display(),
                });
            }
            ReturnKind::Plain => {
                ctx.traces.push(TraceEvent::Event {
                    name: format!("{op_name}.result"),
                    value: v.display(),
                });
            }
            ReturnKind::Unit => {}
        },
        Err(e) => {
            ctx.traces.push(TraceEvent::Event {
                name: format!("{op_name}.error"),
                value: format!("{e:?}"),
            });
            ctx.aborted = true;
        }
    }
}

// -------- expression / statement evaluator --------

fn eval_block(ctx: &mut Ctx, b: &syn::Block) -> Result<Value, InterpError> {
    let mut last = Value::Unit;
    let n = b.stmts.len();
    for (i, stmt) in b.stmts.iter().enumerate() {
        if ctx.aborted {
            return Ok(Value::Unit);
        }
        last = eval_stmt(ctx, stmt, i + 1 == n)?;
    }
    Ok(last)
}

fn eval_stmt(ctx: &mut Ctx, stmt: &Stmt, is_last: bool) -> Result<Value, InterpError> {
    match stmt {
        Stmt::Local(local) => {
            let init = local.init.as_ref().ok_or_else(|| {
                InterpError::Unsupported("let without initializer".into())
            })?;
            let mock_name = stmt_mock_name(stmt);
            let val = if let Some(mock_name) = mock_name {
                eval_mock_call(ctx, &init.expr, &mock_name)?
            } else {
                eval_expr(ctx, &init.expr)?
            };
            if let Pat::Ident(pi) = &local.pat {
                bind_local(ctx, pi.ident.to_string(), val);
            } else if let Pat::Type(pt) = &local.pat {
                if let Pat::Ident(pi) = pt.pat.as_ref() {
                    bind_local(ctx, pi.ident.to_string(), val);
                }
            }
            Ok(Value::Unit)
        }
        Stmt::Expr(e, semi) => {
            let v = eval_expr(ctx, e)?;
            if semi.is_none() && is_last {
                Ok(v)
            } else {
                Ok(Value::Unit)
            }
        }
        Stmt::Item(_) => Ok(Value::Unit),
        Stmt::Macro(m) => {
            // statement-level macro
            eval_macro(ctx, &m.mac).map(|_| Value::Unit)
        }
    }
}

fn bind_local(ctx: &mut Ctx, name: String, v: Value) {
    if let Some(scope) = ctx.locals.last_mut() {
        scope.insert(name, v);
    }
}

fn lookup_local(ctx: &Ctx, name: &str) -> Option<Value> {
    for s in ctx.locals.iter().rev() {
        if let Some(v) = s.get(name) {
            return Some(v.clone());
        }
    }
    None
}

fn eval_expr(ctx: &mut Ctx, e: &Expr) -> Result<Value, InterpError> {
    match e {
        Expr::Lit(el) => match &el.lit {
            Lit::Int(n) => Ok(Value::Int(n.base10_parse::<i64>().unwrap_or(0))),
            Lit::Str(s) => Ok(Value::Str(s.value())),
            Lit::Bool(b) => Ok(Value::Bool(b.value)),
            _ => Err(InterpError::Unsupported(format!("literal {:?}", el.lit))),
        },
        Expr::Path(p) => {
            let id = p
                .path
                .segments
                .last()
                .map(|s| s.ident.to_string())
                .unwrap_or_default();
            // self → current instance alias
            if let Some(v) = lookup_local(ctx, &id) {
                return Ok(v);
            }
            // Could be an Ok/Err constructor referenced bare — treat as
            // unknown for now.
            Err(InterpError::UnknownName(id))
        }
        Expr::Reference(r) => eval_expr(ctx, &r.expr),
        Expr::Paren(p) => eval_expr(ctx, &p.expr),
        Expr::Group(g) => eval_expr(ctx, &g.expr),
        Expr::Block(b) => eval_block(ctx, &b.block),
        Expr::Field(f) => {
            // either self.X or alias.X — base resolves to an Instance value
            let base = eval_field_base(ctx, &f.base)?;
            let name = match &f.member {
                syn::Member::Named(id) => id.to_string(),
                syn::Member::Unnamed(_) => {
                    return Err(InterpError::Unsupported("unnamed member".into()))
                }
            };
            if let Value::Instance(alias) = base {
                let inst = ctx
                    .instances
                    .get(&alias)
                    .ok_or_else(|| InterpError::UnknownName(format!("instance {alias}")))?;
                inst.fields
                    .get(&name)
                    .cloned()
                    .ok_or_else(|| InterpError::UnknownName(format!("{alias}.{name}")))
            } else {
                Err(InterpError::TypeError(format!(
                    "field {} on non-instance",
                    name
                )))
            }
        }
        Expr::Binary(b) => eval_binary(ctx, b),
        Expr::Assign(a) => {
            let val = eval_expr(ctx, &a.right)?;
            assign_field(ctx, &a.left, val, None)?;
            Ok(Value::Unit)
        }
        Expr::Struct(s) => {
            let ty = s
                .path
                .segments
                .last()
                .map(|s| s.ident.to_string())
                .unwrap_or_default();
            let mut fields = BTreeMap::new();
            for fv in &s.fields {
                let name = match &fv.member {
                    syn::Member::Named(id) => id.to_string(),
                    syn::Member::Unnamed(_) => continue,
                };
                let v = eval_expr(ctx, &fv.expr)?;
                fields.insert(name, v);
            }
            Ok(Value::StructLit { ty, fields })
        }
        Expr::Call(c) => eval_call(ctx, c),
        Expr::MethodCall(mc) => eval_method_call(ctx, mc),
        Expr::Macro(m) => eval_macro(ctx, &m.mac),
        Expr::If(i) => eval_if(ctx, i),
        Expr::Unary(u) => match u.op {
            UnOp::Neg(_) => {
                let v = eval_expr(ctx, &u.expr)?;
                if let Value::Int(n) = v {
                    Ok(Value::Int(-n))
                } else {
                    Err(InterpError::TypeError("neg of non-int".into()))
                }
            }
            UnOp::Not(_) => {
                let v = eval_expr(ctx, &u.expr)?;
                if let Value::Bool(b) = v {
                    Ok(Value::Bool(!b))
                } else {
                    Err(InterpError::TypeError("not of non-bool".into()))
                }
            }
            _ => Err(InterpError::Unsupported("unary op".into())),
        },
        _ => Err(InterpError::Unsupported(format!(
            "expr kind not handled: {:?}",
            std::any::type_name_of_val(e)
        ))),
    }
}

fn eval_field_base(ctx: &mut Ctx, e: &Expr) -> Result<Value, InterpError> {
    match e {
        Expr::Path(p) => {
            let id = p
                .path
                .segments
                .last()
                .map(|s| s.ident.to_string())
                .unwrap_or_default();
            if id == "self" {
                if let Some(alias) = &ctx.current_self {
                    return Ok(Value::Instance(alias.clone()));
                }
            }
            if let Some(v) = lookup_local(ctx, &id) {
                return Ok(v);
            }
            // maybe alias?
            if ctx.instances.contains_key(&id) {
                return Ok(Value::Instance(id));
            }
            Err(InterpError::UnknownName(id))
        }
        _ => eval_expr(ctx, e),
    }
}

fn eval_binary(ctx: &mut Ctx, b: &syn::ExprBinary) -> Result<Value, InterpError> {
    use syn::BinOp::*;
    let l = eval_expr(ctx, &b.left)?;
    let r = eval_expr(ctx, &b.right)?;
    let int_op = |op_name: &str, l: Value, r: Value| -> Result<Value, InterpError> {
        match (l, r) {
            (Value::Int(a), Value::Int(c)) => match op_name {
                "+" => Ok(Value::Int(a + c)),
                "-" => Ok(Value::Int(a - c)),
                "*" => Ok(Value::Int(a * c)),
                "/" => {
                    if c == 0 {
                        Err(InterpError::Panic("attempt to divide by zero".into()))
                    } else {
                        Ok(Value::Int(a / c))
                    }
                }
                "%" => {
                    if c == 0 {
                        Err(InterpError::Panic("attempt to calculate remainder with divisor of zero".into()))
                    } else {
                        Ok(Value::Int(a % c))
                    }
                }
                _ => Err(InterpError::Unsupported(format!("int op {op_name}"))),
            },
            _ => Err(InterpError::TypeError(format!("op {op_name} on non-ints"))),
        }
    };
    match b.op {
        Add(_) => int_op("+", l, r),
        Sub(_) => int_op("-", l, r),
        Mul(_) => int_op("*", l, r),
        Div(_) => int_op("/", l, r),
        Rem(_) => int_op("%", l, r),
        Eq(_) => Ok(Value::Bool(value_eq(&l, &r))),
        Ne(_) => Ok(Value::Bool(!value_eq(&l, &r))),
        Lt(_) => bool_cmp(l, r, |a, b| a < b),
        Le(_) => bool_cmp(l, r, |a, b| a <= b),
        Gt(_) => bool_cmp(l, r, |a, b| a > b),
        Ge(_) => bool_cmp(l, r, |a, b| a >= b),
        AddAssign(_) => {
            assign_field(ctx, &b.left, r, Some("+"))?;
            Ok(Value::Unit)
        }
        SubAssign(_) => {
            assign_field(ctx, &b.left, r, Some("-"))?;
            Ok(Value::Unit)
        }
        _ => Err(InterpError::Unsupported(format!("binop {:?}", b.op))),
    }
}

fn bool_cmp<F: Fn(i64, i64) -> bool>(l: Value, r: Value, f: F) -> Result<Value, InterpError> {
    match (l, r) {
        (Value::Int(a), Value::Int(b)) => Ok(Value::Bool(f(a, b))),
        _ => Err(InterpError::TypeError("cmp non-ints".into())),
    }
}

fn value_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => x == y,
        (Value::Str(x), Value::Str(y)) => x == y,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        _ => false,
    }
}

fn assign_field(
    ctx: &mut Ctx,
    target: &Expr,
    rhs: Value,
    op: Option<&str>,
) -> Result<(), InterpError> {
    if let Expr::Field(f) = target {
        let alias = match f.base.as_ref() {
            Expr::Path(p) => {
                let id = p
                    .path
                    .segments
                    .last()
                    .map(|s| s.ident.to_string())
                    .unwrap_or_default();
                if id == "self" {
                    ctx.current_self
                        .clone()
                        .ok_or_else(|| InterpError::UnknownName("self".into()))?
                } else if ctx.instances.contains_key(&id) {
                    id
                } else {
                    return Err(InterpError::UnknownName(format!("base {id}")));
                }
            }
            _ => return Err(InterpError::Unsupported("field assign base".into())),
        };
        let field_name = match &f.member {
            syn::Member::Named(id) => id.to_string(),
            _ => return Err(InterpError::Unsupported("unnamed field".into())),
        };
        let inst = ctx
            .instances
            .get_mut(&alias)
            .ok_or_else(|| InterpError::UnknownName(format!("instance {alias}")))?;
        let cur = inst
            .fields
            .get(&field_name)
            .cloned()
            .unwrap_or(Value::Int(0));
        let new = match op {
            None => rhs,
            Some("+") => add_values(cur, rhs)?,
            Some("-") => sub_values(cur, rhs)?,
            _ => return Err(InterpError::Unsupported("assign op".into())),
        };
        inst.fields.insert(field_name.clone(), new.clone());
        if inst.tracked.contains(&field_name) {
            let prefix = inst.alias_prefix.clone();
            let trace_name = if prefix.is_empty() {
                field_name
            } else {
                format!("{prefix}.{field_name}")
            };
            ctx.traces.push(TraceEvent::Event {
                name: trace_name,
                value: new.display(),
            });
        }
        Ok(())
    } else {
        Err(InterpError::Unsupported(
            "assignment target not field".into(),
        ))
    }
}

fn add_values(a: Value, b: Value) -> Result<Value, InterpError> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(Value::Int(x + y)),
        _ => Err(InterpError::TypeError("+= non-int".into())),
    }
}
fn sub_values(a: Value, b: Value) -> Result<Value, InterpError> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(Value::Int(x - y)),
        _ => Err(InterpError::TypeError("-= non-int".into())),
    }
}

fn eval_call(ctx: &mut Ctx, c: &syn::ExprCall) -> Result<Value, InterpError> {
    let callee = if let Expr::Path(p) = c.func.as_ref() {
        p.path
            .segments
            .last()
            .map(|s| s.ident.to_string())
            .unwrap_or_default()
    } else {
        return Err(InterpError::Unsupported("non-path call".into()));
    };
    match callee.as_str() {
        "Ok" => {
            let v = if let Some(arg) = c.args.first() {
                eval_expr(ctx, arg)?
            } else {
                Value::Unit
            };
            Ok(Value::ResultOk(Box::new(v)))
        }
        "Err" => {
            let v = if let Some(arg) = c.args.first() {
                eval_expr(ctx, arg)?
            } else {
                Value::Unit
            };
            Ok(Value::ResultErr(Box::new(v)))
        }
        other => Err(InterpError::UnknownName(format!("call {other}"))),
    }
}

fn eval_method_call(
    ctx: &mut Ctx,
    mc: &syn::ExprMethodCall,
) -> Result<Value, InterpError> {
    let method = mc.method.to_string();
    // Built-in string/integer methods.
    match method.as_str() {
        "to_string" => {
            let v = eval_expr(ctx, &mc.receiver)?;
            return Ok(Value::Str(v.display()));
        }
        "to_uppercase" => {
            let v = eval_expr(ctx, &mc.receiver)?;
            if let Value::Str(s) = v {
                return Ok(Value::Str(s.to_uppercase()));
            }
            return Err(InterpError::TypeError("to_uppercase non-str".into()));
        }
        "to_lowercase" => {
            let v = eval_expr(ctx, &mc.receiver)?;
            if let Value::Str(s) = v {
                return Ok(Value::Str(s.to_lowercase()));
            }
            return Err(InterpError::TypeError("to_lowercase non-str".into()));
        }
        "trim" => {
            let v = eval_expr(ctx, &mc.receiver)?;
            if let Value::Str(s) = v {
                return Ok(Value::Str(s.trim().to_string()));
            }
            return Err(InterpError::TypeError("trim non-str".into()));
        }
        "clone" => {
            return eval_expr(ctx, &mc.receiver);
        }
        _ => {}
    }
    // User method. The receiver may be self/an alias/an instance value.
    // If the method matches a #[spec_operation] method, recurse.
    let receiver_alias = receiver_alias(ctx, &mc.receiver)?;
    if let Some((op_name, method_def)) = find_method_op_by_method_name(ctx.module, &method) {
        // Bind args by parameter name.
        let prev_self = ctx.current_self.replace(receiver_alias);
        ctx.traces.push(TraceEvent::Run {
            operation: op_name.clone(),
        });
        let mut scope = BTreeMap::new();
        for (i, p) in method_def.params.iter().enumerate() {
            let v = mc
                .args
                .iter()
                .nth(i)
                .map(|a| eval_expr(ctx, a))
                .transpose()?
                .unwrap_or(Value::Unit);
            scope.insert(p.name.clone(), v);
        }
        ctx.locals.push(scope);
        let body = method_def.body.clone();
        let r = eval_block(ctx, &body);
        ctx.locals.pop();
        ctx.current_self = prev_self;
        finalize_op_result(ctx, &op_name, r, method_def.return_kind);
        return Ok(Value::Unit);
    }
    Err(InterpError::Unsupported(format!("method {method}")))
}

fn receiver_alias(ctx: &mut Ctx, e: &Expr) -> Result<String, InterpError> {
    match e {
        Expr::Path(p) => {
            let id = p
                .path
                .segments
                .last()
                .map(|s| s.ident.to_string())
                .unwrap_or_default();
            if id == "self" {
                ctx.current_self
                    .clone()
                    .ok_or_else(|| InterpError::UnknownName("self".into()))
            } else if ctx.instances.contains_key(&id) {
                Ok(id)
            } else if let Some(Value::Instance(a)) = lookup_local(ctx, &id) {
                Ok(a)
            } else {
                Err(InterpError::UnknownName(id))
            }
        }
        _ => Err(InterpError::Unsupported("complex receiver".into())),
    }
}

fn find_method_op_by_method_name<'m>(
    module: &'m Module,
    method_name: &str,
) -> Option<(String, &'m MethodDef)> {
    module
        .method_ops
        .iter()
        .find(|(_, m)| m.method_name == method_name)
        .map(|(n, m)| (n.clone(), m))
}

fn eval_macro(ctx: &mut Ctx, m: &syn::Macro) -> Result<Value, InterpError> {
    let name = m
        .path
        .segments
        .last()
        .map(|s| s.ident.to_string())
        .unwrap_or_default();
    match name.as_str() {
        "spec_event" => {
            // spec_event!("name", expr) — but the macro is variadic; we
            // parse as comma-separated expressions.
            let args: syn::punctuated::Punctuated<Expr, syn::Token![,]> = m
                .parse_body_with(syn::punctuated::Punctuated::parse_terminated)
                .map_err(|e| InterpError::Unsupported(format!("spec_event parse: {e}")))?;
            let mut iter = args.iter();
            let name_e = iter
                .next()
                .ok_or_else(|| InterpError::Unsupported("spec_event no name".into()))?;
            let val_e = iter
                .next()
                .ok_or_else(|| InterpError::Unsupported("spec_event no value".into()))?;
            let name = match name_e {
                Expr::Lit(l) => match &l.lit {
                    Lit::Str(s) => s.value(),
                    _ => return Err(InterpError::Unsupported("spec_event name not str".into())),
                },
                _ => return Err(InterpError::Unsupported("spec_event name not literal".into())),
            };
            let v = eval_expr(ctx, val_e)?;
            ctx.traces.push(TraceEvent::Event {
                name,
                value: v.display(),
            });
            Ok(Value::Unit)
        }
        "format" => {
            // Parse: first arg is fmt string with {ident} placeholders;
            // remaining args provide positional fillers (we ignore — fixtures
            // use named placeholders only).
            let args: syn::punctuated::Punctuated<Expr, syn::Token![,]> = m
                .parse_body_with(syn::punctuated::Punctuated::parse_terminated)
                .map_err(|e| InterpError::Unsupported(format!("format parse: {e}")))?;
            let fmt = args
                .first()
                .and_then(|e| expr_str_lit(e))
                .ok_or_else(|| InterpError::Unsupported("format no fmt str".into()))?;
            let mut positional: Vec<Value> = Vec::new();
            for arg in args.iter().skip(1) {
                positional.push(eval_expr(ctx, arg)?);
            }
            let s = render_format(ctx, &fmt, &positional)?;
            Ok(Value::Str(s))
        }
        "panic" => {
            let args: syn::punctuated::Punctuated<Expr, syn::Token![,]> = m
                .parse_body_with(syn::punctuated::Punctuated::parse_terminated)
                .map_err(|e| InterpError::Unsupported(format!("panic parse: {e}")))?;
            let msg = if let Some(first) = args.first() {
                if let Some(s) = expr_str_lit(first) {
                    s
                } else {
                    "panicked".into()
                }
            } else {
                "panicked".into()
            };
            ctx.panic_msg = Some(msg.clone());
            Err(InterpError::Panic(msg))
        }
        _ => Err(InterpError::Unsupported(format!("macro {name}!"))),
    }
}

fn render_format(ctx: &Ctx, fmt: &str, positional: &[Value]) -> Result<String, InterpError> {
    let mut out = String::new();
    let mut chars = fmt.chars().peekable();
    let mut pos_iter = positional.iter();
    while let Some(c) = chars.next() {
        if c == '{' {
            if chars.peek() == Some(&'{') {
                chars.next();
                out.push('{');
                continue;
            }
            // collect until '}'
            let mut name = String::new();
            while let Some(&nc) = chars.peek() {
                if nc == '}' {
                    chars.next();
                    break;
                }
                name.push(nc);
                chars.next();
            }
            // Strip format spec after ':'
            let key = if let Some(idx) = name.find(':') {
                &name[..idx]
            } else {
                &name[..]
            };
            let val = if key.is_empty() {
                pos_iter
                    .next()
                    .cloned()
                    .ok_or_else(|| InterpError::Unsupported("format positional underflow".into()))?
            } else if let Ok(_n) = key.parse::<usize>() {
                pos_iter
                    .next()
                    .cloned()
                    .ok_or_else(|| InterpError::Unsupported("format index underflow".into()))?
            } else {
                lookup_local(ctx, key)
                    .ok_or_else(|| InterpError::UnknownName(format!("fmt {key}")))?
            };
            out.push_str(&val.display());
        } else if c == '}' {
            if chars.peek() == Some(&'}') {
                chars.next();
            }
            out.push('}');
        } else {
            out.push(c);
        }
    }
    Ok(out)
}

fn eval_if(ctx: &mut Ctx, i: &syn::ExprIf) -> Result<Value, InterpError> {
    let cond = eval_expr(ctx, &i.cond)?;
    let take = match cond {
        Value::Bool(b) => b,
        Value::Int(n) => n != 0,
        _ => return Err(InterpError::TypeError("if cond non-bool".into())),
    };
    if take {
        eval_block(ctx, &i.then_branch)
    } else if let Some((_, else_e)) = &i.else_branch {
        eval_expr(ctx, else_e)
    } else {
        Ok(Value::Unit)
    }
}

fn eval_mock_call(
    ctx: &mut Ctx,
    expr: &Expr,
    mock_name: &str,
) -> Result<Value, InterpError> {
    // Expect a method call expression — extract the first argument as input.
    let input = match expr {
        Expr::MethodCall(mc) => {
            let arg = mc
                .args
                .first()
                .ok_or_else(|| InterpError::Unsupported("mock no arg".into()))?;
            eval_expr(ctx, arg)?
        }
        Expr::Call(c) => {
            let arg = c
                .args
                .first()
                .ok_or_else(|| InterpError::Unsupported("mock no arg".into()))?;
            eval_expr(ctx, arg)?
        }
        _ => return Err(InterpError::Unsupported("mock target shape".into())),
    };
    let input_str = input.display();
    ctx.traces.push(TraceEvent::Event {
        name: format!("{mock_name}.request"),
        value: input_str.clone(),
    });
    let table = ctx.mocks.get(mock_name).cloned().unwrap_or_default();
    if let Some(resp) = table.get(&input_str) {
        ctx.traces.push(TraceEvent::Event {
            name: format!("{mock_name}.response"),
            value: resp.clone(),
        });
        Ok(Value::Str(resp.clone()))
    } else {
        ctx.traces.push(TraceEvent::Event {
            name: format!("{mock_name}.error"),
            value: format!("no mock response for input '{input_str}'"),
        });
        ctx.aborted = true;
        Ok(Value::Unit)
    }
}

fn yaml_to_value(v: &YamlVal) -> Value {
    match v {
        YamlVal::String(s) => Value::Str(s.clone()),
        YamlVal::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Int(i)
            } else if let Some(f) = n.as_f64() {
                Value::Int(f as i64)
            } else {
                Value::Unit
            }
        }
        YamlVal::Bool(b) => Value::Bool(*b),
        YamlVal::Null => Value::Unit,
        _ => Value::Unit,
    }
}

// Suppress unused-import warning when a feature is off.
#[allow(dead_code)]
fn _suppress(_: Param, _: Option<String>) {}
#[allow(dead_code)]
fn _suppress2() {
    let _ = name_from_attr as fn(&[syn::Attribute], &str) -> Option<String>;
}
