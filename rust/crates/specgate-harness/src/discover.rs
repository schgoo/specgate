//! Source discovery — parses a fixture `.rs` file with `syn` and pulls
//! out the `#[spec_setup]`, `#[spec_operation]`, `#[spec_event]`, and
//! `#[spec_mock]` annotations along with the bodies they apply to.

use std::collections::{BTreeMap, BTreeSet};
use syn::{Attribute, Expr, ExprLit, ImplItem, Item, Lit, Meta, Stmt};

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Module {
    pub setups: BTreeMap<String, FnDef>,           // name -> setup
    pub free_ops: BTreeMap<String, FnDef>,         // name -> free operation
    pub method_ops: BTreeMap<String, MethodDef>,   // name -> method
    pub structs: BTreeMap<String, StructDef>,      // type name -> struct definition
    pub method_owner: BTreeMap<String, String>,    // method name -> impl-target type name
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct FnDef {
    pub fn_name: String,
    pub params: Vec<Param>,
    pub return_kind: ReturnKind,
    pub body: syn::Block,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct MethodDef {
    pub method_name: String,
    pub owner_type: String,
    pub takes_self: bool,
    pub params: Vec<Param>, // excluding self
    pub return_kind: ReturnKind,
    pub body: syn::Block,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Param {
    pub name: String,
    pub is_reference: bool,
    pub type_name: String, // simplified
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ReturnKind {
    Unit,
    Plain,           // any T that isn't Result, Vec, etc.
    Result,          // Result<_, _>
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct StructDef {
    pub type_name: String,
    pub fields: Vec<String>,
    pub tracked: BTreeSet<String>, // fields with #[spec_event]
}

pub fn parse_module(src: &str) -> Result<Module, syn::Error> {
    let file = syn::parse_file(src)?;
    let mut module = Module {
        setups: BTreeMap::new(),
        free_ops: BTreeMap::new(),
        method_ops: BTreeMap::new(),
        structs: BTreeMap::new(),
        method_owner: BTreeMap::new(),
    };

    for item in file.items.iter() {
        match item {
            Item::Fn(f) => {
                let attrs = &f.attrs;
                if let Some(name) = name_from_attr(attrs, "spec_setup") {
                    module
                        .setups
                        .insert(name, fn_to_def(&f.sig, &f.block, attrs));
                } else if let Some(name) = name_from_attr(attrs, "spec_operation") {
                    module
                        .free_ops
                        .insert(name, fn_to_def(&f.sig, &f.block, attrs));
                }
            }
            Item::Struct(s) => {
                let mut fields = Vec::new();
                let mut tracked = BTreeSet::new();
                if let syn::Fields::Named(named) = &s.fields {
                    for f in &named.named {
                        if let Some(id) = &f.ident {
                            let n = id.to_string();
                            fields.push(n.clone());
                            if has_attr(&f.attrs, "spec_event") {
                                tracked.insert(n);
                            }
                        }
                    }
                }
                module.structs.insert(
                    s.ident.to_string(),
                    StructDef {
                        type_name: s.ident.to_string(),
                        fields,
                        tracked,
                    },
                );
            }
            Item::Impl(im) => {
                let owner = type_path_name(&im.self_ty);
                for it in &im.items {
                    if let ImplItem::Fn(m) = it {
                        if let Some(name) = name_from_attr(&m.attrs, "spec_operation") {
                            let (params, takes_self) = parse_method_inputs(&m.sig.inputs);
                            let rk = parse_return_kind(&m.sig.output);
                            module.method_ops.insert(
                                name.clone(),
                                MethodDef {
                                    method_name: m.sig.ident.to_string(),
                                    owner_type: owner.clone(),
                                    takes_self,
                                    params,
                                    return_kind: rk,
                                    body: m.block.clone(),
                                },
                            );
                            module.method_owner.insert(name, owner.clone());
                        }
                    }
                }
            }
            _ => {}
        }
    }
    Ok(module)
}

fn fn_to_def(sig: &syn::Signature, body: &syn::Block, _attrs: &[Attribute]) -> FnDef {
    let (params, _) = parse_method_inputs(&sig.inputs);
    let rk = parse_return_kind(&sig.output);
    FnDef {
        fn_name: sig.ident.to_string(),
        params,
        return_kind: rk,
        body: body.clone(),
    }
}

fn parse_method_inputs(
    inputs: &syn::punctuated::Punctuated<syn::FnArg, syn::Token![,]>,
) -> (Vec<Param>, bool) {
    let mut out = Vec::new();
    let mut takes_self = false;
    for a in inputs {
        match a {
            syn::FnArg::Receiver(_) => takes_self = true,
            syn::FnArg::Typed(pt) => {
                let name = match pt.pat.as_ref() {
                    syn::Pat::Ident(pi) => pi.ident.to_string(),
                    _ => "_".into(),
                };
                let (is_ref, ty) = type_info(&pt.ty);
                out.push(Param {
                    name,
                    is_reference: is_ref,
                    type_name: ty,
                });
            }
        }
    }
    (out, takes_self)
}

fn type_info(t: &syn::Type) -> (bool, String) {
    match t {
        syn::Type::Reference(r) => {
            let inner = type_info(&r.elem).1;
            (true, inner)
        }
        syn::Type::Path(p) => {
            let name = p
                .path
                .segments
                .last()
                .map(|s| s.ident.to_string())
                .unwrap_or_default();
            (false, name)
        }
        _ => (false, String::new()),
    }
}

fn parse_return_kind(out: &syn::ReturnType) -> ReturnKind {
    match out {
        syn::ReturnType::Default => ReturnKind::Unit,
        syn::ReturnType::Type(_, ty) => match ty.as_ref() {
            syn::Type::Tuple(t) if t.elems.is_empty() => ReturnKind::Unit,
            syn::Type::Path(p) => {
                let last = p.path.segments.last().map(|s| s.ident.to_string());
                if last.as_deref() == Some("Result") {
                    ReturnKind::Result
                } else {
                    ReturnKind::Plain
                }
            }
            _ => ReturnKind::Plain,
        },
    }
}

fn type_path_name(t: &syn::Type) -> String {
    match t {
        syn::Type::Path(p) => p
            .path
            .segments
            .last()
            .map(|s| s.ident.to_string())
            .unwrap_or_default(),
        _ => String::new(),
    }
}

fn has_attr(attrs: &[Attribute], name: &str) -> bool {
    attrs.iter().any(|a| match &a.meta {
        Meta::Path(p) => p.is_ident(name),
        Meta::List(ml) => ml.path.is_ident(name),
        Meta::NameValue(nv) => nv.path.is_ident(name),
    })
}

pub fn name_from_attr(attrs: &[Attribute], attr_name: &str) -> Option<String> {
    for a in attrs {
        match &a.meta {
            Meta::List(ml) if ml.path.is_ident(attr_name) => {
                // parse first string literal in the tokens
                let mut iter = ml.tokens.clone().into_iter();
                while let Some(tok) = iter.next() {
                    if let proc_macro2::TokenTree::Literal(lit) = tok {
                        let s = lit.to_string();
                        if s.starts_with('"') && s.ends_with('"') {
                            return Some(s[1..s.len() - 1].to_string());
                        }
                    }
                }
                return None;
            }
            Meta::Path(p) if p.is_ident(attr_name) => {
                return Some(String::new());
            }
            _ => {}
        }
    }
    None
}

pub fn stmt_mock_name(stmt: &Stmt) -> Option<String> {
    if let Stmt::Local(local) = stmt {
        return name_from_attr(&local.attrs, "spec_mock");
    }
    None
}

pub fn expr_str_lit(e: &Expr) -> Option<String> {
    if let Expr::Lit(ExprLit {
        lit: Lit::Str(s), ..
    }) = e
    {
        return Some(s.value());
    }
    None
}
