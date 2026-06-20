//! Procedural macros for SpecGate annotations.
//!
//! These expand into calls into `::specgate_annotations::__rt` (which
//! re-exports `specgate-runtime`). The expanded code emits real trace
//! events at runtime.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{quote, quote_spanned};
use syn::parse::{Parse, ParseStream};
use syn::visit_mut::VisitMut;
use syn::{
    parse_macro_input, parse_quote, BinOp, Block, Data, DeriveInput, Expr, FnArg, Fields, ItemFn,
    LitStr, Pat, ReturnType, Stmt, Type,
};

struct NameArg(String);

impl Parse for NameArg {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let lit: LitStr = input.parse()?;
        Ok(NameArg(lit.value()))
    }
}

fn rt() -> TokenStream2 {
    quote! { ::specgate_annotations::__rt }
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
    if let Type::Path(p) = ty {
        if let Some(s) = p.path.segments.last() {
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
    }
    false
}

fn is_reference(ty: &Type) -> bool {
    matches!(ty, Type::Reference(_))
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

fn typed_params(f: &ItemFn) -> Vec<(syn::Ident, syn::Type)> {
    let mut out = Vec::new();
    for arg in &f.sig.inputs {
        if let FnArg::Typed(pt) = arg {
            if let Pat::Ident(id) = &*pt.pat {
                out.push((id.ident.clone(), (*pt.ty).clone()));
            }
        }
    }
    out
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
                    if let Some(mock_name) = take_mock_name(&local.attrs) {
                        if let Some(stmts) = expand_mock_let(&local, &mock_name) {
                            new.extend(stmts);
                            continue;
                        }
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
        if a.path().is_ident("spec_mock") {
            if let Ok(NameArg(name)) = a.parse_args::<NameArg>() {
                return Some(name);
            }
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
    let expr = match stmt {
        Stmt::Expr(e, Some(_)) => e,
        _ => return None,
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
    let field = match lhs {
        Expr::Field(f) => f,
        _ => return None,
    };
    let field_name = match &field.member {
        syn::Member::Named(id) => id.to_string(),
        _ => return None,
    };
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
    let NameArg(op_name) = parse_macro_input!(attr as NameArg);
    let mut func = parse_macro_input!(item as ItemFn);

    let return_kind = classify_return(&func.sig.output);
    let is_method = has_receiver(&func);
    let params = typed_params(&func);
    let param_names: Vec<String> = params.iter().map(|(i, _)| i.to_string()).collect();
    let has_ref_param = params.iter().any(|(_, t)| is_reference(t));

    let mut visitor = BodyInstrumenter {
        param_names: param_names.clone(),
    };
    visitor.visit_block_mut(&mut func.block);
    let body = &func.block;

    let _ = return_kind;
    let pre = build_pre_stmts(&op_name, &params, is_method, has_ref_param);
    let new_body: Block = parse_quote!({
        #(#pre)*
        #body
    });
    func.block = Box::new(new_body);
    quote!(#func).into()
}

fn build_pre_stmts(
    op_name: &str,
    params: &[(syn::Ident, syn::Type)],
    is_method: bool,
    _has_ref_param: bool,
) -> Vec<Stmt> {
    let rt = rt();
    let mut out: Vec<Stmt> = vec![parse_quote!(#rt::emit_run(#op_name);)];
    if is_method {
        return out;
    }
    let all_printable = params.iter().all(|(_, t)| is_printable_param(t));
    if !all_printable {
        return out;
    }
    for (id, _) in params {
        let event_name = format!("{op_name}.{}", id);
        out.push(parse_quote!(
            #rt::emit_event(#event_name, &::std::format!("{}", #id));
        ));
    }
    out
}

// ---------------------------------------------------------------------------
// #[spec_setup("name")]
// ---------------------------------------------------------------------------

#[proc_macro_attribute]
pub fn spec_setup(attr: TokenStream, item: TokenStream) -> TokenStream {
    let NameArg(setup_name) = parse_macro_input!(attr as NameArg);
    let mut func = parse_macro_input!(item as ItemFn);
    let params = typed_params(&func);
    let rt = rt();

    let mut pre: Vec<Stmt> = Vec::new();
    for (id, ty) in &params {
        if is_owned_primitive(ty) {
            let name = format!("{setup_name}.{}", id);
            pre.push(parse_quote!(
                #rt::emit_event(#name, &::std::format!("{}", #id));
            ));
        }
    }
    let body = &func.block;
    let new_body: Block = parse_quote!({
        #(#pre)*
        #body
    });
    func.block = Box::new(new_body);
    quote!(#func).into()
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

#[proc_macro_derive(SpecEvent, attributes(spec_event))]
pub fn derive_spec_event(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let rt = rt();

    let (impl_g, ty_g, where_c) = input.generics.split_for_impl();

    // --- Enum: match each variant and emit variant name + named fields ---
    if let Data::Enum(data_enum) = &input.data {
        let enum_name_lower = name.to_string().to_lowercase();
        let mut arms: Vec<TokenStream2> = Vec::new();
        let mut to_spec_value_arms: Vec<TokenStream2> = Vec::new();

        for variant in &data_enum.variants {
            let vname = &variant.ident;
            let vname_str = vname.to_string();
            match &variant.fields {
                Fields::Unit => {
                    arms.push(quote! {
                        #name::#vname => {
                            #rt::emit_event_v(
                                &__sg_base,
                                #rt::Value::String(#vname_str.to_string()),
                            );
                        }
                    });
                    to_spec_value_arms.push(quote! {
                        #name::#vname => #rt::Value::String(#vname_str.to_string()),
                    });
                }
                Fields::Named(named) => {
                    let field_idents: Vec<&syn::Ident> = named
                        .named
                        .iter()
                        .filter_map(|f| f.ident.as_ref())
                        .collect();
                    let field_strs: Vec<String> =
                        field_idents.iter().map(|id| id.to_string()).collect();
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
                    arms.push(quote! {
                        #name::#vname(..) => {
                            #rt::emit_event_v(
                                &__sg_base,
                                #rt::Value::String(#vname_str.to_string()),
                            );
                        }
                    });
                    to_spec_value_arms.push(quote! {
                        #name::#vname(..) => #rt::Value::String(#vname_str.to_string()),
                    });
                }
            }
        }

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
        };
        return out.into();
    }

    // --- Struct: emit each field annotated with #[spec_event] ---
    let mut emits = Vec::new();
    let mut to_spec_value_inserts = Vec::new();
    if let Data::Struct(s) = &input.data {
        for field in &s.fields {
            // Build ToSpecValue insert for every named field.
            if let Some(id) = &field.ident {
                let fname = id.to_string();
                to_spec_value_inserts.push(quote! {
                    __sg_m.insert(
                        #fname.to_string(),
                        #rt::ToSpecValue::to_spec_value(&self.#id),
                    );
                });
            }

            // emit_fields only covers #[spec_event]-annotated fields.
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
            if !marked { continue; }
            if let Some(id) = &field.ident {
                let fname = override_name.unwrap_or_else(|| id.to_string());
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
    }

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
    };
    out.into()
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
        #rt::emit_event(#name, &::std::format!("{}", #expr))
    };
    out.into()
}
