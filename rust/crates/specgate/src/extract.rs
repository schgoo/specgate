use std::fs;
use std::path::Path;

use quote::ToTokens;
use serde::Serialize;
use syn::parse::Parser;
use syn::punctuated::Punctuated;
use syn::visit::Visit;
use syn::{
    Attribute, Expr, ExprAssign, ExprLit, ExprMacro, ExprPath, Field, Fields, FnArg, ImplItem,
    Item, ItemFn, ItemImpl, ItemMod, ItemStruct, Lit, Pat, PatIdent, ReturnType, Token, Type,
    Visibility,
};

use crate::runtime::{Annotation, OperationKind};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CompileError {
    pub message: String,
}

#[derive(Clone)]
struct OperationArgs {
    operation: String,
    kind: OperationKind,
}

#[derive(Clone)]
struct NamedArgs {
    operation: String,
    name: String,
}

#[derive(Clone)]
struct OperationOnlyArgs {
    operation: String,
}

pub fn extract_annotations(
    source: &str,
    crate_name: &str,
) -> Result<Vec<Annotation>, Vec<CompileError>> {
    let file = syn::parse_file(source).map_err(|error| {
        vec![CompileError {
            message: error.to_string(),
        }]
    })?;
    let mut extractor = Extractor::new(crate_name, source);
    extractor.visit_file(&file);
    extractor.finish()
}

pub fn write_annotation_registry(
    source_path: &Path,
    manifest_dir: &Path,
    crate_name: &str,
) -> Result<(), String> {
    let source = fs::read_to_string(source_path)
        .map_err(|error| format!("failed to read {}: {error}", source_path.display()))?;
    let annotations =
        extract_annotations(&source, crate_name).map_err(|errors| format_errors(&errors))?;
    let registry_dir = manifest_dir.join("target").join("specgate");
    fs::create_dir_all(&registry_dir)
        .map_err(|error| format!("failed to create {}: {error}", registry_dir.display()))?;
    let registry_path = registry_dir.join("annotations.json");
    let json = serde_json::to_string(&annotations)
        .map_err(|error| format!("failed to serialize annotations: {error}"))?;
    fs::write(&registry_path, json)
        .map_err(|error| format!("failed to write {}: {error}", registry_path.display()))?;
    Ok(())
}

fn format_errors(errors: &[CompileError]) -> String {
    errors
        .iter()
        .map(|error| error.message.as_str())
        .collect::<Vec<_>>()
        .join("; ")
}

struct Extractor<'a> {
    crate_name: &'a str,
    modules: Vec<String>,
    annotations: Vec<Annotation>,
    errors: Vec<CompileError>,
}

impl<'a> Extractor<'a> {
    fn new(crate_name: &'a str, source: &'a str) -> Self {
        let _ = source;
        Self {
            crate_name,
            modules: Vec::new(),
            annotations: Vec::new(),
            errors: Vec::new(),
        }
    }

    fn finish(self) -> Result<Vec<Annotation>, Vec<CompileError>> {
        if self.errors.is_empty() {
            Ok(self.annotations)
        } else {
            Err(self.errors)
        }
    }

    fn push_error(&mut self, message: impl Into<String>) {
        self.errors.push(CompileError {
            message: message.into(),
        });
    }

    fn symbol_prefix(&self) -> String {
        if self.modules.is_empty() {
            self.crate_name.to_string()
        } else {
            format!("{}::{}", self.crate_name, self.modules.join("::"))
        }
    }

    fn function_symbol(&self, function_name: &str) -> String {
        format!("{}::{function_name}", self.symbol_prefix())
    }

    fn method_symbol(&self, type_name: &str, method_name: &str) -> String {
        format!("{}::{type_name}::{method_name}", self.symbol_prefix())
    }

    fn field_symbol(&self, type_name: &str, field_name: &str) -> String {
        format!("{}::{type_name}::{field_name}", self.symbol_prefix())
    }

    fn handle_item_mod(&mut self, item_mod: &ItemMod) {
        if let Some((_, items)) = &item_mod.content {
            self.modules.push(item_mod.ident.to_string());
            for item in items {
                self.handle_item(item);
            }
            self.modules.pop();
        }
    }

    fn handle_item(&mut self, item: &Item) {
        match item {
            Item::Fn(item_fn) => self.handle_fn(item_fn, None),
            Item::Impl(item_impl) => self.handle_impl(item_impl),
            Item::Mod(item_mod) => self.handle_item_mod(item_mod),
            Item::Struct(item_struct) => self.handle_struct(item_struct),
            _ => {}
        }
    }

    fn handle_impl(&mut self, item_impl: &ItemImpl) {
        let Some(type_name) = type_ident(&item_impl.self_ty) else {
            return;
        };

        for item in &item_impl.items {
            if let ImplItem::Fn(method) = item {
                self.handle_impl_fn(method, &type_name);
            }
        }
    }

    fn handle_struct(&mut self, item_struct: &ItemStruct) {
        let type_name = item_struct.ident.to_string();
        let struct_capture = item_struct
            .attrs
            .iter()
            .find(|attr| attr.path().is_ident("spec_capture"))
            .map(parse_operation_only);
        let mut field_captures = Vec::new();

        for field in named_fields(item_struct) {
            for attr in &field.attrs {
                if attr.path().is_ident("spec_capture") {
                    field_captures.push((field, parse_operation_only(attr)));
                }
                if attr.path().is_ident("spec_checkpoint") {
                    self.push_error("spec_checkpoint must be placed on a method");
                }
            }
        }

        if item_struct
            .attrs
            .iter()
            .any(|attr| attr.path().is_ident("spec_checkpoint"))
        {
            self.push_error("spec_checkpoint must be placed on a method");
        }

        if let Some(Err(error)) = &struct_capture {
            self.push_error(error.clone());
        }

        for (_, result) in &field_captures {
            if let Err(error) = result {
                self.push_error(error.clone());
            }
        }

        if struct_capture.is_some() && !field_captures.is_empty() {
            self.push_error("spec_capture on struct and field cannot be combined");
            return;
        }

        if let Some(Ok(args)) = struct_capture {
            for field in named_fields(item_struct) {
                if matches!(field.vis, Visibility::Public(_)) {
                    if let Some(field_name) = &field.ident {
                        self.annotations.push(Annotation::SpecCapture {
                            operation: args.operation.clone(),
                            symbol: self.field_symbol(&type_name, &field_name.to_string()),
                            capture_all: true,
                        });
                    }
                }
            }
        }

        for (field, result) in field_captures {
            if let (Some(field_name), Ok(args)) = (&field.ident, result) {
                self.annotations.push(Annotation::SpecCapture {
                    operation: args.operation,
                    symbol: self.field_symbol(&type_name, &field_name.to_string()),
                    capture_all: false,
                });
            }
        }
    }

    fn handle_fn(&mut self, item_fn: &ItemFn, impl_type: Option<&str>) {
        if item_fn
            .attrs
            .iter()
            .any(|attr| attr.path().is_ident("spec_capture"))
        {
            self.push_error("spec_capture must be placed on a struct or struct field");
        }

        let symbol = match impl_type {
            Some(type_name) => self.method_symbol(type_name, &item_fn.sig.ident.to_string()),
            None => self.function_symbol(&item_fn.sig.ident.to_string()),
        };

        self.collect_function_annotations(
            &item_fn.attrs,
            &item_fn.sig.inputs,
            &item_fn.sig.output,
            impl_type,
            &symbol,
            Some(&item_fn.block),
        );
    }

    fn handle_impl_fn(&mut self, method: &syn::ImplItemFn, impl_type: &str) {
        if method
            .attrs
            .iter()
            .any(|attr| attr.path().is_ident("spec_capture"))
        {
            self.push_error("spec_capture must be placed on a struct or struct field");
        }

        let symbol = self.method_symbol(impl_type, &method.sig.ident.to_string());
        self.collect_function_annotations(
            &method.attrs,
            &method.sig.inputs,
            &method.sig.output,
            Some(impl_type),
            &symbol,
            Some(&method.block),
        );
    }

    fn collect_function_annotations(
        &mut self,
        attrs: &[Attribute],
        inputs: &Punctuated<FnArg, Token![,]>,
        output: &ReturnType,
        impl_type: Option<&str>,
        symbol: &str,
        block: Option<&syn::Block>,
    ) {
        for attr in attrs {
            if attr.path().is_ident("spec_operation") {
                match parse_operation_args(attr) {
                    Ok(args) => {
                        self.annotations.push(Annotation::SpecOperation {
                            operation: args.operation.clone(),
                            kind: args.kind,
                            symbol: symbol.to_string(),
                        });
                    }
                    Err(error) => self.push_error(error),
                }
            } else if attr.path().is_ident("spec_setup") {
                match parse_named_args(attr) {
                    Ok(args) => {
                        if inputs.iter().any(is_receiver) {
                            self.push_error("spec_setup must not take self");
                        } else {
                            self.annotations.push(Annotation::SpecSetup {
                                operation: args.operation,
                                name: args.name,
                                symbol: symbol.to_string(),
                                params: param_names(inputs),
                                returns: render_return_type(output, impl_type),
                            });
                        }
                    }
                    Err(error) => self.push_error(error),
                }
            } else if attr.path().is_ident("spec_checkpoint") {
                match parse_operation_only(attr) {
                    Ok(args) => {
                        if impl_type.is_none() {
                            self.push_error("spec_checkpoint must be placed on a method");
                        } else {
                            self.annotations.push(Annotation::SpecCheckpoint {
                                operation: args.operation,
                                symbol: symbol.to_string(),
                            });
                        }
                    }
                    Err(error) => self.push_error(error),
                }
            } else if attr.path().is_ident("spec_mock") {
                match parse_named_args(attr) {
                    Ok(args) => {
                        self.annotations.push(Annotation::SpecMock {
                            operation: args.operation,
                            symbol: symbol.to_string(),
                            mock_name: args.name,
                        });
                    }
                    Err(error) => self.push_error(error),
                }
            }
        }

        if let Some(block) = block {
            let mut checkpoints = InlineCheckpointCollector {
                extractor: self,
                symbol: symbol.to_string(),
                next_index: 1,
            };
            checkpoints.visit_block(block);
        }
    }
}

impl<'ast> Visit<'ast> for Extractor<'_> {
    fn visit_item(&mut self, item: &'ast Item) {
        self.handle_item(item);
    }
}

struct InlineCheckpointCollector<'a, 'b> {
    extractor: &'a mut Extractor<'b>,
    symbol: String,
    next_index: usize,
}

impl<'ast> Visit<'ast> for InlineCheckpointCollector<'_, '_> {
    fn visit_expr_macro(&mut self, expr_macro: &'ast ExprMacro) {
        if expr_macro.mac.path.is_ident("spec_checkpoint") {
            match parse_inline_checkpoint(&expr_macro.mac.tokens) {
                Ok(operation) => {
                    self.extractor.annotations.push(Annotation::SpecCheckpoint {
                        operation,
                        symbol: format!("{}::checkpoint_{}", self.symbol, self.next_index),
                    });
                    self.next_index += 1;
                }
                Err(error) => self.extractor.push_error(error),
            }
        }
        syn::visit::visit_expr_macro(self, expr_macro);
    }
}

fn named_fields(item_struct: &ItemStruct) -> Vec<&Field> {
    match &item_struct.fields {
        Fields::Named(fields) => fields.named.iter().collect(),
        _ => Vec::new(),
    }
}

fn type_ident(ty: &Type) -> Option<String> {
    match ty {
        Type::Path(type_path) => type_path
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string()),
        _ => None,
    }
}

fn parse_operation_args(attr: &Attribute) -> Result<OperationArgs, String> {
    let args = parse_attr_args(attr)?;
    let Some(first) = args.first() else {
        return Err("missing operation name".to_string());
    };
    let operation = match first {
        Expr::Lit(ExprLit {
            lit: Lit::Str(value),
            ..
        }) => value.value(),
        Expr::Assign(_) => return Err("missing operation name".to_string()),
        _ => return Err("missing operation name".to_string()),
    };

    let kind = find_named_ident(&args[1..], "kind")?
        .ok_or_else(|| "missing required parameter: kind".to_string())?;
    let kind = match kind.as_str() {
        "Stateless" => OperationKind::Stateless,
        "StateMachine" => OperationKind::StateMachine,
        "Sequence" => OperationKind::Sequence,
        "ErrorMap" => OperationKind::ErrorMap,
        "Structural" => OperationKind::Structural,
        other => return Err(format!("invalid kind: {other}")),
    };

    Ok(OperationArgs { operation, kind })
}

fn parse_named_args(attr: &Attribute) -> Result<NamedArgs, String> {
    let args = parse_attr_args(attr)?;
    let Some(first) = args.first() else {
        return Err("missing operation name".to_string());
    };
    let operation = match first {
        Expr::Lit(ExprLit {
            lit: Lit::Str(value),
            ..
        }) => value.value(),
        Expr::Assign(_) => return Err("missing operation name".to_string()),
        _ => return Err("missing operation name".to_string()),
    };

    let name = find_named_lit(&args[1..], "name")?
        .ok_or_else(|| "missing required parameter: name".to_string())?;

    Ok(NamedArgs { operation, name })
}

fn parse_operation_only(attr: &Attribute) -> Result<OperationOnlyArgs, String> {
    let args = parse_attr_args(attr)?;
    let Some(Expr::Lit(ExprLit {
        lit: Lit::Str(value),
        ..
    })) = args.first()
    else {
        return Err("missing operation name".to_string());
    };
    Ok(OperationOnlyArgs {
        operation: value.value(),
    })
}

fn parse_inline_checkpoint(tokens: &proc_macro2::TokenStream) -> Result<String, String> {
    let parser = Punctuated::<Expr, Token![,]>::parse_terminated;
    let args = parser
        .parse2(tokens.clone())
        .map_err(|error| error.to_string())?;
    let Some(Expr::Lit(ExprLit {
        lit: Lit::Str(operation),
        ..
    })) = args.first()
    else {
        return Err("missing operation name".to_string());
    };
    Ok(operation.value())
}

fn parse_attr_args(attr: &Attribute) -> Result<Vec<Expr>, String> {
    let parser = Punctuated::<Expr, Token![,]>::parse_terminated;
    parser
        .parse2(
            attr.meta
                .require_list()
                .map_err(|error| error.to_string())?
                .tokens
                .clone(),
        )
        .map(|punctuated| punctuated.into_iter().collect())
        .map_err(|error| error.to_string())
}

fn find_named_ident(args: &[Expr], name: &str) -> Result<Option<String>, String> {
    for arg in args {
        let Expr::Assign(ExprAssign { left, right, .. }) = arg else {
            continue;
        };
        let Expr::Path(ExprPath { path, .. }) = &**left else {
            continue;
        };
        if !path.is_ident(name) {
            continue;
        }
        let Expr::Path(ExprPath { path, .. }) = &**right else {
            return Err(format!("invalid {name} value"));
        };
        let Some(segment) = path.segments.last() else {
            return Err(format!("invalid {name} value"));
        };
        return Ok(Some(segment.ident.to_string()));
    }
    Ok(None)
}

fn find_named_lit(args: &[Expr], name: &str) -> Result<Option<String>, String> {
    for arg in args {
        let Expr::Assign(ExprAssign { left, right, .. }) = arg else {
            continue;
        };
        let Expr::Path(ExprPath { path, .. }) = &**left else {
            continue;
        };
        if !path.is_ident(name) {
            continue;
        }
        let Expr::Lit(ExprLit {
            lit: Lit::Str(value),
            ..
        }) = &**right
        else {
            return Err(format!("invalid {name} value"));
        };
        return Ok(Some(value.value()));
    }
    Ok(None)
}

fn is_receiver(arg: &FnArg) -> bool {
    matches!(arg, FnArg::Receiver(_))
}

fn param_names(inputs: &Punctuated<FnArg, Token![,]>) -> Vec<String> {
    inputs
        .iter()
        .filter_map(|arg| match arg {
            FnArg::Typed(arg) => match &*arg.pat {
                Pat::Ident(PatIdent { ident, .. }) => Some(ident.to_string()),
                _ => None,
            },
            FnArg::Receiver(_) => None,
        })
        .collect()
}

fn render_return_type(output: &ReturnType, impl_type: Option<&str>) -> String {
    match output {
        ReturnType::Default => "()".to_string(),
        ReturnType::Type(_, ty) => {
            let rendered = ty.to_token_stream().to_string().replace(' ', "");
            if rendered == "Self" {
                impl_type.unwrap_or("Self").to_string()
            } else {
                rendered
            }
        }
    }
}
