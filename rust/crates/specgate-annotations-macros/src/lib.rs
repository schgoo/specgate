//! Procedural macros for `SpecGate` annotations.
//!
//! - `#[spec_operation("name")]` — marks a function as a spec operation; emits a
//!   `$run` event, per-parameter input events, and a `$result`/`$outcome`.
//! - `#[spec_setup("name")]` — marks a constructor/setup that builds the
//!   receiver for stateful (method) operations.
//! - `#[spec_mock(...)]` — injects a table-driven mock dependency.
//! - `#[derive(SpecEvent)]` + `#[spec_event]` — capture struct/enum fields into
//!   the trace as structured values.
//! - `#[spec_input("name")]` — give a parameter a language-neutral spec name.
//! - `spec_component!("name")` — declare the crate's component (the spec name).
//! - `spec_trace!(...)` — emit an inline trace checkpoint from within a body.
//!
//! These expand into calls into `::specgate_annotations::__rt` (which
//! re-exports `specgate-runtime`); the expanded code emits real trace events at
//! runtime.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{quote, quote_spanned};
use syn::parse::{Parse, ParseStream};
use syn::visit_mut::VisitMut;
use syn::{
    BinOp, Block, Data, DeriveInput, Expr, Fields, FnArg, Ident, ItemFn, LitStr, Pat, ReturnType, Stmt, Type, parse_macro_input,
    parse_quote,
};

struct NameArg(String);

impl Parse for NameArg {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let lit: LitStr = input.parse()?;
        Ok(NameArg(lit.value()))
    }
}

/// Arguments to `#[spec_operation("name", spec = "component")]`. The optional
/// `spec = "…"` overrides the crate-root default component for this operation.
struct OperationArg {
    op_name: String,
    spec: Option<String>,
}

impl Parse for OperationArg {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let lit: LitStr = input.parse()?;
        let mut spec = None;
        while input.peek(syn::Token![,]) {
            let _: syn::Token![,] = input.parse()?;
            if input.is_empty() {
                break;
            }
            let key: Ident = input.parse()?;
            let _: syn::Token![=] = input.parse()?;
            let val: LitStr = input.parse()?;
            if key == "spec" {
                spec = Some(val.value());
            } else {
                return Err(syn::Error::new(key.span(), "expected `spec`"));
            }
        }
        Ok(OperationArg {
            op_name: lit.value(),
            spec,
        })
    }
}

/// Arguments to `#[spec_setup("operation", fills = "param", spec = "component")]`.
/// The first positional string is the OPERATION this setup prepares. `fills`
/// pins the setup to a specific operation parameter; `spec` overrides the
/// crate-root default component. Both keys are optional and order-independent.
struct SetupArg {
    op_name: String,
    fills: Option<String>,
    spec: Option<String>,
}

impl Parse for SetupArg {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let lit: LitStr = input.parse()?;
        let mut fills = None;
        let mut spec = None;
        while input.peek(syn::Token![,]) {
            let _: syn::Token![,] = input.parse()?;
            if input.is_empty() {
                break;
            }
            let key: Ident = input.parse()?;
            let _: syn::Token![=] = input.parse()?;
            let val: LitStr = input.parse()?;
            if key == "fills" {
                fills = Some(val.value());
            } else if key == "spec" {
                spec = Some(val.value());
            } else {
                return Err(syn::Error::new(key.span(), "expected `fills` or `spec`"));
            }
        }
        Ok(SetupArg {
            op_name: lit.value(),
            fills,
            spec,
        })
    }
}

fn rt() -> TokenStream2 {
    quote! { ::specgate::__rt }
}

/// The token for an item's `component` field: an explicit `spec = "…"` literal
/// when given, else the crate-root `__SPECGATE_COMPONENT` constant declared by
/// `spec_component!`. The latter makes omitting `spec_component!` a COMPILE-TIME
/// error ("cannot find value `__SPECGATE_COMPONENT` in the crate root").
fn component_tokens(spec: Option<&str>) -> TokenStream2 {
    if let Some(s) = spec {
        quote! { #s }
    } else {
        quote! { crate::__SPECGATE_COMPONENT }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ReturnKind {
    Unit,
    Result,
    Option,
    Other,
}

fn classify_return(ty: &ReturnType) -> ReturnKind {
    match ty {
        ReturnType::Default => ReturnKind::Unit,
        ReturnType::Type(_, t) => match &**t {
            Type::Tuple(t) if t.elems.is_empty() => ReturnKind::Unit,
            Type::Path(p) => {
                let last = p.path.segments.last();
                match last.map(|s| s.ident.to_string()).as_deref() {
                    Some("Result") => ReturnKind::Result,
                    Some("Option") => ReturnKind::Option,
                    _ => ReturnKind::Other,
                }
            }
            _ => ReturnKind::Other,
        },
    }
}

fn has_receiver(f: &ItemFn) -> bool {
    f.sig.inputs.iter().any(|a| matches!(a, FnArg::Receiver(_)))
}

fn is_owned_primitive(ty: &Type) -> bool {
    if let Type::Path(p) = ty
        && let Some(s) = p.path.segments.last()
    {
        return matches!(
            s.ident.to_string().as_str(),
            "i8" | "i16"
                | "i32"
                | "i64"
                | "i128"
                | "isize"
                | "u8"
                | "u16"
                | "u32"
                | "u64"
                | "u128"
                | "usize"
                | "f32"
                | "f64"
                | "bool"
                | "char"
                | "String"
                | "str"
        );
    }
    false
}

fn is_reference(ty: &Type) -> bool {
    matches!(ty, Type::Reference(_))
}

/// True for `&mut T` parameters. These represent mutable state objects threaded
/// through an operation (their mutations are captured separately), not value
/// inputs — so they are excluded from input-echo emission.
fn is_mut_ref(ty: &Type) -> bool {
    matches!(ty, Type::Reference(r) if r.mutability.is_some())
}

/// Like `is_owned_primitive` but also accepts shared references to primitives
/// (notably `&str`) — the printed value just goes through `format!("{}", x)`.
fn is_printable_param(ty: &Type) -> bool {
    if is_owned_primitive(ty) {
        return true;
    }
    if let Type::Reference(r) = ty {
        return is_owned_primitive(&r.elem);
    }
    false
}

/// Extract `(code_ident, type, spec_name)` for each typed parameter, consuming
/// and removing any `#[spec_input("name")]` attribute. The spec name (when
/// present) is the language-neutral name the spec uses for the input; the code
/// parameter name is irrelevant to the spec.
fn extract_param_renames(f: &mut ItemFn) -> Vec<(Ident, Type, Option<String>)> {
    let mut out = Vec::new();
    for arg in &mut f.sig.inputs {
        if let FnArg::Typed(pt) = arg
            && let Pat::Ident(id) = &*pt.pat
        {
            let ident = id.ident.clone();
            let ty = (*pt.ty).clone();
            let mut spec_name = None;
            pt.attrs.retain(|a| {
                if a.path().is_ident("spec_input") {
                    if let Ok(s) = a.parse_args::<LitStr>() {
                        spec_name = Some(s.value());
                    }
                    false
                } else {
                    true
                }
            });
            out.push((ident, ty, spec_name));
        }
    }
    out
}

/// The spec-facing name of a parameter: its `#[spec_input]` override, else the
/// code identifier.
fn spec_param_name(ident: &Ident, spec_name: Option<&String>) -> String {
    spec_name.cloned().unwrap_or_else(|| ident.to_string())
}

// ---------------------------------------------------------------------------
// Body instrumentation
// ---------------------------------------------------------------------------

struct BodyInstrumenter {
    param_names: Vec<String>,
}

impl VisitMut for BodyInstrumenter {
    fn visit_block_mut(&mut self, block: &mut Block) {
        // Recurse first.
        for stmt in &mut block.stmts {
            syn::visit_mut::visit_stmt_mut(self, stmt);
        }

        let original = std::mem::take(&mut block.stmts);
        let mut new: Vec<Stmt> = Vec::with_capacity(original.len());

        for stmt in original {
            match stmt {
                Stmt::Local(local) => {
                    if let Some(mock_name) = take_mock_name(&local.attrs)
                        && let Some(stmts) = expand_mock_let(&local, &mock_name)
                    {
                        new.extend(stmts);
                        continue;
                    }
                    new.push(Stmt::Local(local));
                }
                stmt => {
                    let emit_after = field_mutation_emit(&stmt, &self.param_names);
                    new.push(stmt);
                    if let Some(after) = emit_after {
                        new.push(after);
                    }
                }
            }
        }

        block.stmts = new;
    }
}

fn take_mock_name(attrs: &[syn::Attribute]) -> Option<String> {
    for a in attrs {
        if a.path().is_ident("spec_mock")
            && let Ok(NameArg(name)) = a.parse_args::<NameArg>()
        {
            return Some(name);
        }
    }
    None
}

fn expand_mock_let(local: &syn::Local, mock_name: &str) -> Option<Vec<Stmt>> {
    let init = local.init.as_ref()?;
    let arg_expr = extract_mock_input(&init.expr)?;
    let rt = rt();
    let request_name = format!("{mock_name}.request");
    let response_name = format!("{mock_name}.response");
    let error_name = format!("{mock_name}.error");
    let pat = &local.pat;

    let block: Block = parse_quote!({
        let __sg_input = (#arg_expr).to_string();
        #rt::emit_event(#request_name, &__sg_input);
        let #pat = match #rt::mock_lookup(#mock_name, &__sg_input) {
            ::std::option::Option::Some(__sg_v) => {
                #rt::emit_event(#response_name, &__sg_v);
                __sg_v
            }
            ::std::option::Option::None => {
                #rt::emit_event(
                    #error_name,
                    &::std::format!("no mock response for input '{}'", __sg_input),
                );
                return ::std::default::Default::default();
            }
        };
    });
    Some(block.stmts)
}

fn extract_mock_input(e: &Expr) -> Option<&Expr> {
    if let Expr::MethodCall(mc) = e {
        return mc.args.last();
    }
    if let Expr::Call(c) = e {
        return c.args.last();
    }
    None
}

fn field_mutation_emit(stmt: &Stmt, param_names: &[String]) -> Option<Stmt> {
    let Stmt::Expr(expr, Some(_)) = stmt else {
        return None;
    };

    let lhs = match expr {
        Expr::Assign(a) => &*a.left,
        Expr::Binary(b) => {
            let is_compound = matches!(
                b.op,
                BinOp::AddAssign(_)
                    | BinOp::SubAssign(_)
                    | BinOp::MulAssign(_)
                    | BinOp::DivAssign(_)
                    | BinOp::RemAssign(_)
                    | BinOp::BitXorAssign(_)
                    | BinOp::BitAndAssign(_)
                    | BinOp::BitOrAssign(_)
                    | BinOp::ShlAssign(_)
                    | BinOp::ShrAssign(_)
            );
            if !is_compound {
                return None;
            }
            &*b.left
        }
        _ => return None,
    };
    field_emit_from_lhs(lhs, param_names)
}

fn field_emit_from_lhs(lhs: &Expr, param_names: &[String]) -> Option<Stmt> {
    let Expr::Field(field) = lhs else {
        return None;
    };
    let syn::Member::Named(id) = &field.member else {
        return None;
    };
    let field_name = id.to_string();
    let event_name = match &*field.base {
        Expr::Path(p) if p.path.is_ident("self") => field_name.clone(),
        Expr::Path(p) => {
            let id = p.path.get_ident()?;
            let name = id.to_string();
            if !param_names.contains(&name) {
                return None;
            }
            format!("{name}.{field_name}")
        }
        _ => return None,
    };
    let rt = rt();
    let stmt: Stmt = parse_quote! {
        #rt::emit_event_v(#event_name, #rt::ToSpecValue::to_spec_value(&(#lhs)));
    };
    Some(stmt)
}

// ---------------------------------------------------------------------------
// #[spec_operation("name")]
// ---------------------------------------------------------------------------

#[proc_macro_attribute]
pub fn spec_operation(attr: TokenStream, item: TokenStream) -> TokenStream {
    let OperationArg { op_name, spec } = parse_macro_input!(attr as OperationArg);
    let mut func = parse_macro_input!(item as ItemFn);

    let is_method = has_receiver(&func);
    let is_async = func.sig.asyncness.is_some();
    let params = extract_param_renames(&mut func);
    let param_names: Vec<String> = params.iter().map(|(i, _, _)| i.to_string()).collect();
    let has_ref_param = params.iter().any(|(_, t, _)| is_reference(t));

    let mut visitor = BodyInstrumenter {
        param_names: param_names.clone(),
    };
    visitor.visit_block_mut(&mut func.block);
    let body = &func.block;

    let pre = build_pre_stmts(&op_name, &params, is_method, has_ref_param);
    // Post-body emission of `$result` (and, for struct returns, per-field
    // events). Moving this into the macro makes an annotated operation
    // self-emit its complete trace whether it is driven by the harness runner
    // or called directly from an ordinary test — the latter is what `extract
    // --cases` records. The body is wrapped so the emission runs on
    // EVERY return path, including early `return`s and `?` short-circuits.
    let post = build_post_emit(&func.sig.output);
    let new_body: Block = if let Some(post) = post {
        let ret_ty = match &func.sig.output {
            ReturnType::Type(_, ty) => ty.clone(),
            ReturnType::Default => unreachable!("post-emit only built for a non-unit return"),
        };
        if is_async {
            parse_quote!({
                #(#pre)*
                #[allow(clippy::redundant_closure_call)]
                let __sg_ret = (async move #body).await;
                #post
                __sg_ret
            })
        } else {
            parse_quote!({
                #(#pre)*
                #[allow(clippy::redundant_closure_call)]
                let __sg_ret = (move || -> #ret_ty #body)();
                #post
                __sg_ret
            })
        }
    } else {
        parse_quote!({
            #(#pre)*
            #body
        })
    };
    *func.block = new_body;

    // Registry entry for discovery.
    // We wrap the distributed_slice static in a named const so it compiles
    // correctly whether the annotated function is a free function (module-level)
    // or a method inside an `impl` block.  A bare `static` at item level is
    // forbidden as an associated item; a named `const` containing inner items
    // is allowed in both positions.
    let rt = rt();
    let component = component_tokens(spec.as_deref());
    let fn_name = func.sig.ident.to_string();
    let const_ident = Ident::new(&format!("_SPECGATE_REG_{}", fn_name.to_uppercase()), func.sig.ident.span());
    let static_ident = Ident::new(&format!("_SPECGATE_STATIC_{}", fn_name.to_uppercase()), func.sig.ident.span());
    let param_entries: Vec<TokenStream2> = params
        .iter()
        .map(|(id, ty, spec_name)| {
            let name_str = spec_param_name(id, spec_name.as_ref());
            let ty_str = quote!(#ty).to_string();
            quote! { (#name_str, #ty_str) }
        })
        .collect();
    let ret_str = match &func.sig.output {
        ReturnType::Default => String::from("()"),
        ReturnType::Type(_, ty) => quote!(#ty).to_string(),
    };

    quote! {
        #func

        #[allow(dead_code, non_upper_case_globals)]
        const #const_ident: () = {
            #[#rt::linkme::distributed_slice(#rt::SPECGATE_OPS)]
            #[linkme(crate = #rt::linkme)]
            static #static_ident: #rt::OpMeta = #rt::OpMeta {
                name: #op_name,
                module_path: ::core::module_path!(),
                fn_name: #fn_name,
                is_setup: false,
                is_async: #is_async,
                params: &[#(#param_entries),*],
                return_type: #ret_str,
                fills: "",
                component: #component,
            };
        };
    }
    .into()
}

fn build_pre_stmts(op_name: &str, params: &[(Ident, Type, Option<String>)], _is_method: bool, _has_ref_param: bool) -> Vec<Stmt> {
    let rt = rt();
    let mut out: Vec<Stmt> = vec![parse_quote!(#rt::emit_run(#op_name);)];
    // Emit every parameter as an `op.<spec_name>` typed event via `ToSpecValue`.
    // All value-bearing params (primitives, structs, enums, collections) emit a
    // structured `Value` that round-trips correctly through the matcher. The event
    // uses the language-neutral `#[spec_input]` name when present.
    // Self receivers and `&mut T` params are excluded from input-echo emission:
    // receivers are `FnArg::Receiver` and are never present in `params`; mutable
    // reference params represent state objects threaded through an operation and
    // are skipped by the `is_mut_ref` guard below.
    for (id, ty, spec_name) in params {
        let name = spec_param_name(id, spec_name.as_ref());
        let event_name = format!("{op_name}.{name}");
        if !is_mut_ref(ty) {
            out.push(parse_quote!(
                #rt::emit_event_v(#event_name, #rt::ToSpecValue::to_spec_value(&#id));
            ));
        }
    }
    out
}

/// Build the post-body `$result`/field emission for an operation, mirroring the
/// harness runner's former `build_post_emit` (codegen.rs) so that traces stay
/// byte-identical whether an op is driven by the runner or called directly.
///
/// Returns `None` for a unit/`()` return (nothing to emit). Otherwise returns
/// statements that consume `__sg_ret` (the captured return value) and emit:
/// - `Result<T, E>` → a tagged `{Ok|Err}` map `$result`.
/// - `Option<T>`    → a tagged `{Some|None}` map `$result`.
/// - a printable scalar (`i32`/`String`/`&str`/…) → a Display-string `$result`.
/// - anything else  → the [`ReturnEmit`] autoref ladder, which emits per-field
///   events + a structured `$result` for struct returns, or just a structured
///   `$result` for enums/collections, or a Display `$result` as a last resort.
fn build_post_emit(output: &ReturnType) -> Option<TokenStream2> {
    let rt = rt();
    let ty = match output {
        ReturnType::Default => return None,
        ReturnType::Type(_, t) => {
            if matches!(&**t, Type::Tuple(tup) if tup.elems.is_empty()) {
                return None;
            }
            t
        }
    };
    Some(match classify_return(output) {
        ReturnKind::Unit => return None,
        ReturnKind::Result => quote! {
            match &__sg_ret {
                Ok(__sg_v) => {
                    let mut __sg_m = ::std::collections::BTreeMap::new();
                    __sg_m.insert("Ok".to_string(), #rt::ToSpecValue::to_spec_value(__sg_v));
                    #rt::emit_event_v("$result", #rt::Value::Map(__sg_m));
                }
                Err(__sg_e) => {
                    let mut __sg_m = ::std::collections::BTreeMap::new();
                    __sg_m.insert("Err".to_string(), #rt::Value::String(::std::format!("{}", __sg_e)));
                    #rt::emit_event_v("$result", #rt::Value::Map(__sg_m));
                }
            }
        },
        ReturnKind::Option => quote! {
            match &__sg_ret {
                Some(__sg_v) => {
                    let mut __sg_m = ::std::collections::BTreeMap::new();
                    __sg_m.insert("Some".to_string(), #rt::ToSpecValue::to_spec_value(__sg_v));
                    #rt::emit_event_v("$result", #rt::Value::Map(__sg_m));
                }
                None => {
                    let mut __sg_m = ::std::collections::BTreeMap::new();
                    __sg_m.insert("None".to_string(), #rt::Value::Map(::std::collections::BTreeMap::new()));
                    #rt::emit_event_v("$result", #rt::Value::Map(__sg_m));
                }
            }
        },
        ReturnKind::Other => {
            if is_printable_param(ty) {
                quote! {
                    #rt::emit_event_v("$result", #rt::ToSpecValue::to_spec_value(&__sg_ret));
                }
            } else {
                quote! {
                    {
                        use #rt::ReturnEmitStruct as _;
                        use #rt::ReturnEmitToSpec as _;
                        use #rt::ReturnEmitDisplay as _;
                        use #rt::ReturnEmitNone as _;
                        (&&&&#rt::ReturnEmit(&__sg_ret)).emit_result();
                    }
                }
            }
        }
    })
}

/// Build record-only echo statements prepended to a `#[spec_setup]` body.
/// Each construction parameter gets a `$setup.<spec_name>` event written to
/// the record file (via [`record_event_only`]) so `specgate extract --cases`
/// can recover the case's `setup:` map without pushing anything into the
/// in-process trace buffer. When the setup has a `fills` pin, the event name
/// is suffixed with `_<fills>` to avoid key collisions when two setups share a
/// param name.
fn build_setup_echo_stmts(fills: Option<&str>, params: &[(Ident, Type, Option<String>)]) -> Vec<Stmt> {
    let rt = rt();
    let fills_suffix = fills.filter(|s| !s.is_empty()).map(|s| format!("_{s}")).unwrap_or_default();
    let mut out: Vec<Stmt> = Vec::new();
    for (id, ty, spec_name) in params {
        if is_mut_ref(ty) {
            continue;
        }
        let name = spec_param_name(id, spec_name.as_ref());
        let event_name = format!("$setup.{name}{fills_suffix}");
        if is_printable_param(ty) {
            out.push(parse_quote!(
                #rt::record_event_only(#event_name, #rt::Value::String(::std::format!("{}", #id)));
            ));
        } else {
            out.push(parse_quote!(
                #rt::record_event_only(#event_name, #rt::ToSpecValue::to_spec_value(&#id));
            ));
        }
    }
    out
}

#[proc_macro_attribute]
pub fn spec_setup(attr: TokenStream, item: TokenStream) -> TokenStream {
    let SetupArg { op_name, fills, spec } = parse_macro_input!(attr as SetupArg);
    let mut func = parse_macro_input!(item as ItemFn);
    let params = extract_param_renames(&mut func);
    let rt = rt();
    let component = component_tokens(spec.as_deref());

    // Prepend record-only echo statements so `specgate extract --cases` can
    // recover each setup's construction inputs as the case's `setup:` map.
    // `#[spec_input]` attributes on parameters are consumed (stripped) by
    // `extract_param_renames`; the echo uses the language-neutral spec name.
    let echo_stmts = build_setup_echo_stmts(fills.as_deref(), &params);
    let original_stmts: Vec<Stmt> = std::mem::take(&mut func.block.stmts);
    func.block.stmts = echo_stmts;
    func.block.stmts.extend(original_stmts);

    // Registry entry — same const-wrapping trick as spec_operation so this
    // compiles whether the function is at module scope or inside an impl block.
    // The const/static idents include the operation + fills so that multiple
    // #[spec_setup] attributes can stack on one function without colliding.
    let fn_name = func.sig.ident.to_string();
    let is_async = func.sig.asyncness.is_some();
    let fills_str = fills.clone().unwrap_or_default();
    let suffix = sanitize_ident(&format!("{fn_name}_{op_name}_{fills_str}"));
    let const_ident = Ident::new(&format!("_SPECGATE_SETUP_REG_{suffix}"), func.sig.ident.span());
    let static_ident = Ident::new(&format!("_SPECGATE_SETUP_S_{suffix}"), func.sig.ident.span());
    let param_entries: Vec<TokenStream2> = params
        .iter()
        .map(|(id, ty, spec_name)| {
            let name_str = spec_param_name(id, spec_name.as_ref());
            let ty_str = quote!(#ty).to_string();
            quote! { (#name_str, #ty_str) }
        })
        .collect();
    let ret_str = match &func.sig.output {
        ReturnType::Default => String::from("()"),
        ReturnType::Type(_, ty) => quote!(#ty).to_string(),
    };

    quote! {
        #func

        #[allow(dead_code, non_upper_case_globals)]
        const #const_ident: () = {
            #[#rt::linkme::distributed_slice(#rt::SPECGATE_OPS)]
            #[linkme(crate = #rt::linkme)]
            static #static_ident: #rt::OpMeta = #rt::OpMeta {
                name: #op_name,
                module_path: ::core::module_path!(),
                fn_name: #fn_name,
                is_setup: true,
                is_async: #is_async,
                params: &[#(#param_entries),*],
                return_type: #ret_str,
                fills: #fills_str,
                component: #component,
            };
        };
    }
    .into()
}

/// Turn an arbitrary string into a valid uppercase identifier suffix.
fn sanitize_ident(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_uppercase() } else { '_' })
        .collect()
}

// ---------------------------------------------------------------------------
// #[spec_mock("name")] — only meaningful when used on a `let` binding inside
// a function body wrapped by #[spec_operation]. As an attribute macro at the
// item level (or unexpanded position), this is a no-op.
// ---------------------------------------------------------------------------

#[proc_macro_attribute]
pub fn spec_mock(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}

// ---------------------------------------------------------------------------
// #[derive(SpecEvent)] with helper attribute #[spec_event]
// ---------------------------------------------------------------------------

#[proc_macro_derive(SpecEvent, attributes(spec_event, spec_component))]
pub fn derive_spec_event(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let rt = rt();

    // Optional `#[spec_component("comp")]` helper attribute overrides the
    // crate-root default component for this type.
    let mut comp_override: Option<String> = None;
    for a in &input.attrs {
        if a.path().is_ident("spec_component")
            && let Ok(s) = a.parse_args::<LitStr>()
        {
            comp_override = Some(s.value());
        }
    }
    let component = component_tokens(comp_override.as_deref());

    let (impl_g, ty_g, where_c) = input.generics.split_for_impl();

    // --- Enum: match each variant and emit variant name + named fields ---
    if let Data::Enum(data_enum) = &input.data {
        let enum_name_lower = name.to_string().to_lowercase();
        let mut arms: Vec<TokenStream2> = Vec::new();
        let mut to_spec_value_arms: Vec<TokenStream2> = Vec::new();
        // Registry: one entry per variant (name + named fields; tuple/unit empty).
        let mut variant_metas: Vec<TokenStream2> = Vec::new();

        for variant in &data_enum.variants {
            let vname = &variant.ident;
            let vname_str = vname.to_string();
            match &variant.fields {
                Fields::Unit => {
                    variant_metas.push(quote! {
                        #rt::VariantMeta { name: #vname_str, fields: &[] }
                    });
                    arms.push(quote! {
                        #name::#vname => {
                            #rt::emit_event_v(
                                &__sg_base,
                                #rt::Value::String(#vname_str.to_string()),
                            );
                        }
                    });
                    to_spec_value_arms.push(quote! {
                        #name::#vname => {
                            let mut __sg_outer = ::std::collections::BTreeMap::new();
                            __sg_outer.insert(
                                #vname_str.to_string(),
                                #rt::Value::Map(::std::collections::BTreeMap::new()),
                            );
                            #rt::Value::Map(__sg_outer)
                        }
                    });
                }
                Fields::Named(named) => {
                    let field_idents: Vec<&Ident> = named.named.iter().filter_map(|f| f.ident.as_ref()).collect();
                    let field_strs: Vec<String> = field_idents.iter().map(ToString::to_string).collect();
                    let field_meta_entries: Vec<TokenStream2> = named
                        .named
                        .iter()
                        .filter_map(|f| {
                            let id = f.ident.as_ref()?;
                            let ty = &f.ty;
                            let id_str = id.to_string();
                            let ty_str = quote!(#ty).to_string();
                            Some(quote! { (#id_str, #ty_str) })
                        })
                        .collect();
                    variant_metas.push(quote! {
                        #rt::VariantMeta { name: #vname_str, fields: &[#(#field_meta_entries),*] }
                    });
                    arms.push(quote! {
                        #name::#vname { #(#field_idents),* } => {
                            #rt::emit_event_v(
                                &__sg_base,
                                #rt::Value::String(#vname_str.to_string()),
                            );
                            #(
                                #rt::emit_event_v(
                                    &::std::format!("{}.{}", __sg_base, #field_strs),
                                    #rt::ToSpecValue::to_spec_value(#field_idents),
                                );
                            )*
                        }
                    });
                    to_spec_value_arms.push(quote! {
                        #name::#vname { #(#field_idents),* } => {
                            let mut __sg_inner = ::std::collections::BTreeMap::new();
                            #(
                                __sg_inner.insert(
                                    #field_strs.to_string(),
                                    #rt::ToSpecValue::to_spec_value(#field_idents),
                                );
                            )*
                            let mut __sg_outer = ::std::collections::BTreeMap::new();
                            __sg_outer.insert(
                                #vname_str.to_string(),
                                #rt::Value::Map(__sg_inner),
                            );
                            #rt::Value::Map(__sg_outer)
                        }
                    });
                }
                Fields::Unnamed(_) => {
                    // Tuple variants: emit only the variant name.
                    variant_metas.push(quote! {
                        #rt::VariantMeta { name: #vname_str, fields: &[] }
                    });
                    arms.push(quote! {
                        #name::#vname(..) => {
                            #rt::emit_event_v(
                                &__sg_base,
                                #rt::Value::String(#vname_str.to_string()),
                            );
                        }
                    });
                    to_spec_value_arms.push(quote! {
                        #name::#vname(..) => {
                            let mut __sg_outer = ::std::collections::BTreeMap::new();
                            __sg_outer.insert(
                                #vname_str.to_string(),
                                #rt::Value::Map(::std::collections::BTreeMap::new()),
                            );
                            #rt::Value::Map(__sg_outer)
                        }
                    });
                }
            }
        }

        let type_name_str = name.to_string();
        let reg = register_type_meta(&type_name_str, name, "enum", &[], &variant_metas, &component);
        let out = quote! {
            impl #impl_g #rt::SpecEvent for #name #ty_g #where_c {
                fn emit_fields(&self, __sg_prefix: ::std::option::Option<&str>) {
                    let __sg_base: ::std::string::String = match __sg_prefix {
                        ::std::option::Option::Some(p) => p.to_string(),
                        ::std::option::Option::None => #enum_name_lower.to_string(),
                    };
                    match self {
                        #(#arms)*
                    }
                }
            }
            impl #impl_g #rt::ToSpecValue for #name #ty_g #where_c {
                fn to_spec_value(&self) -> #rt::Value {
                    match self {
                        #(#to_spec_value_arms)*
                    }
                }
            }
            #reg
        };
        return out.into();
    }

    // --- Struct: emit each field annotated with #[spec_event] ---
    // Opt-in model: a field is part of the spec surface ONLY when tagged
    // `#[spec_event]`. The same tag governs BOTH `emit_fields` (per-field
    // events) and `to_spec_value` (the structured `$result` map). Untagged
    // fields are internal and excluded from both.
    let mut emits = Vec::new();
    let mut to_spec_value_inserts = Vec::new();
    // Registry: one `(spec_name, type)` entry per tagged field, in source order.
    let mut field_metas: Vec<TokenStream2> = Vec::new();
    if let Data::Struct(s) = &input.data {
        for field in &s.fields {
            // Determine whether this field opts into the spec surface.
            let mut marked = false;
            let mut override_name: Option<String> = None;
            for a in &field.attrs {
                if !a.path().is_ident("spec_event") {
                    continue;
                }
                marked = true;
                // Optional `name = "X"` override.
                let _ = a.parse_nested_meta(|meta| {
                    if meta.path.is_ident("name") {
                        let lit: LitStr = meta.value()?.parse()?;
                        override_name = Some(lit.value());
                    }
                    Ok(())
                });
            }
            if !marked {
                continue;
            }
            let Some(id) = &field.ident else { continue };

            // The spec name: the `name = "X"` override if present, else the
            // field ident. Used as the key EVERYWHERE the field is exposed —
            // both the `to_spec_value` map key and the `emit_fields` event.
            let fname = override_name.unwrap_or_else(|| id.to_string());

            // Registry field entry (spec name + stringified declared type).
            let fty = &field.ty;
            let fty_str = quote!(#fty).to_string();
            field_metas.push(quote! { (#fname, #fty_str) });

            // ToSpecValue insert for each tagged field, keyed by spec name.
            to_spec_value_inserts.push(quote! {
                __sg_m.insert(
                    #fname.to_string(),
                    #rt::ToSpecValue::to_spec_value(&self.#id),
                );
            });

            // Per-field event for `emit_fields`, keyed by the same spec name.
            emits.push(quote! {
                let __sg_name = match __sg_prefix {
                    ::std::option::Option::Some(p) => ::std::format!("{}.{}", p, #fname),
                    ::std::option::Option::None => #fname.to_string(),
                };
                #rt::emit_event_v(
                    &__sg_name,
                    #rt::ToSpecValue::to_spec_value(&self.#id),
                );
            });
        }
    }

    let type_name_str = name.to_string();
    let reg = register_type_meta(&type_name_str, name, "struct", &field_metas, &[], &component);
    let out = quote! {
        impl #impl_g #rt::SpecEvent for #name #ty_g #where_c {
            fn emit_fields(&self, __sg_prefix: ::std::option::Option<&str>) {
                #(#emits)*
            }
        }
        impl #impl_g #rt::ToSpecValue for #name #ty_g #where_c {
            fn to_spec_value(&self) -> #rt::Value {
                let mut __sg_m = ::std::collections::BTreeMap::new();
                #(#to_spec_value_inserts)*
                #rt::Value::Map(__sg_m)
            }
        }
        impl #impl_g #rt::SpecEventStruct for #name #ty_g #where_c {}
        #reg
    };
    out.into()
}

/// Build the `SPECGATE_TYPES` registration for a `SpecEvent` type. Uses the same
/// const-wrapped `distributed_slice` static trick as `#[spec_operation]` so it
/// compiles whether the type is at module scope or nested (e.g. inside a fn).
fn register_type_meta(
    name_str: &str,
    name: &Ident,
    kind: &str,
    fields: &[TokenStream2],
    variants: &[TokenStream2],
    component: &TokenStream2,
) -> TokenStream2 {
    let rt = rt();
    let suffix = sanitize_ident(name_str);
    let const_ident = Ident::new(&format!("_SPECGATE_TYPE_REG_{suffix}"), name.span());
    let static_ident = Ident::new(&format!("_SPECGATE_TYPE_S_{suffix}"), name.span());
    quote! {
        #[allow(dead_code, non_upper_case_globals)]
        const #const_ident: () = {
            #[#rt::linkme::distributed_slice(#rt::SPECGATE_TYPES)]
            #[linkme(crate = #rt::linkme)]
            static #static_ident: #rt::TypeMeta = #rt::TypeMeta {
                name: #name_str,
                module_path: ::core::module_path!(),
                kind: #kind,
                fields: &[#(#fields),*],
                variants: &[#(#variants),*],
                component: #component,
            };
        };
    }
}

// ---------------------------------------------------------------------------
// spec_trace!("name", &expr)
// ---------------------------------------------------------------------------

struct TraceCall {
    name: LitStr,
    expr: Expr,
}

impl Parse for TraceCall {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let name: LitStr = input.parse()?;
        let _: syn::Token![,] = input.parse()?;
        let expr: Expr = input.parse()?;
        Ok(TraceCall { name, expr })
    }
}

#[proc_macro]
pub fn spec_trace(input: TokenStream) -> TokenStream {
    let TraceCall { name, expr } = parse_macro_input!(input as TraceCall);
    let rt = rt();
    let out = quote_spanned! { name.span() =>
        #rt::emit_event_v(#name, #rt::ToSpecValue::to_spec_value(&#expr))
    };
    out.into()
}

// ---------------------------------------------------------------------------
// spec_component!("dotted.name")
// ---------------------------------------------------------------------------

/// Declare the component that a crate's annotated items belong to by default.
/// Expands to a crate-root `pub(crate) const __SPECGATE_COMPONENT: &str = …`
/// that `#[spec_operation]` / `#[spec_setup]` / `#[derive(SpecEvent)]` reference
/// when no per-item `spec = "…"` override is supplied. Invoke ONCE at a crate
/// root (lib.rs / main.rs / an integration-test file root). Omitting it in a
/// crate that has annotations is a compile-time error.
#[proc_macro]
pub fn spec_component(input: TokenStream) -> TokenStream {
    let NameArg(name) = parse_macro_input!(input as NameArg);
    quote! {
        #[allow(dead_code)]
        pub(crate) const __SPECGATE_COMPONENT: &str = #name;
    }
    .into()
}
