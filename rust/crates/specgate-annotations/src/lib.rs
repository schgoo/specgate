use proc_macro::TokenStream;
use quote::quote;
use syn::fold::Fold;
use syn::parse::{Parse, ParseStream, Parser};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::{
    Attribute, Data, DeriveInput, Expr, ExprAssign, ExprLit, ExprMacro, ExprPath, Field, Fields,
    FnArg, ImplItemFn, ItemFn, ItemStruct, Lit, LitStr, ReturnType, Signature, Token, Type,
    Visibility, parse_macro_input,
};

#[proc_macro_attribute]
pub fn spec_operation(args: TokenStream, input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(args as OperationArgs);
    let input2 = proc_macro2::TokenStream::from(input.clone());
    if let Ok(method) = syn::parse2::<ImplItemFn>(input2.clone()) {
        if has_receiver(&method.sig) {
            return expand_operation_method(method, args).into();
        }
    }
    if let Ok(function) = syn::parse2::<ItemFn>(input2.clone()) {
        return expand_operation_function(function, args, false).into();
    }
    if let Ok(method) = syn::parse2::<ImplItemFn>(input2) {
        return expand_operation_method(method, args).into();
    }
    compile_error2("spec_operation must be placed on a function or method").into()
}

#[proc_macro_attribute]
pub fn spec_setup(args: TokenStream, input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(args as NamedArgs);
    let input2 = proc_macro2::TokenStream::from(input.clone());
    if let Ok(method) = syn::parse2::<ImplItemFn>(input2.clone()) {
        if has_receiver(&method.sig) {
            return expand_passthrough_method(method, args, MacroKind::Setup).into();
        }
    }
    if let Ok(function) = syn::parse2::<ItemFn>(input2.clone()) {
        return expand_passthrough_function(function, args, MacroKind::Setup).into();
    }
    if let Ok(method) = syn::parse2::<ImplItemFn>(input2) {
        return expand_passthrough_method(method, args, MacroKind::Setup).into();
    }
    compile_error2("spec_setup must be placed on a function or method").into()
}

#[proc_macro_attribute]
pub fn spec_checkpoint(args: TokenStream, input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(args as OperationOnlyArgs);
    let input2 = proc_macro2::TokenStream::from(input.clone());
    if let Ok(method) = syn::parse2::<ImplItemFn>(input2.clone()) {
        if has_receiver(&method.sig) {
            return expand_checkpoint_method(method, args).into();
        }
    }
    if let Ok(function) = syn::parse2::<ItemFn>(input2.clone()) {
        return expand_checkpoint_function(function, args).into();
    }
    if let Ok(method) = syn::parse2::<ImplItemFn>(input2) {
        return expand_checkpoint_method(method, args).into();
    }
    compile_error2("spec_checkpoint must be placed on a method").into()
}

#[proc_macro_attribute]
pub fn spec_mock(args: TokenStream, input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(args as NamedArgs);
    let input2 = proc_macro2::TokenStream::from(input.clone());
    if let Ok(method) = syn::parse2::<ImplItemFn>(input2.clone()) {
        if has_receiver(&method.sig) {
            return expand_mock_method(method, args).into();
        }
    }
    if let Ok(function) = syn::parse2::<ItemFn>(input2.clone()) {
        return expand_mock_function(function, args).into();
    }
    if let Ok(method) = syn::parse2::<ImplItemFn>(input2) {
        return expand_mock_method(method, args).into();
    }
    compile_error2("spec_mock must be placed on a function or method").into()
}

#[proc_macro_attribute]
pub fn spec_capture(_args: TokenStream, input: TokenStream) -> TokenStream {
    let input2 = proc_macro2::TokenStream::from(input.clone());
    if syn::parse2::<ItemStruct>(input2).is_ok() {
        input
    } else {
        compile_error2("spec_capture must be placed on a struct or struct field").into()
    }
}

#[proc_macro_derive(SpecCapture, attributes(spec_capture))]
pub fn derive_spec_capture(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand_spec_capture(input).into()
}

fn expand_operation_function(
    mut function: ItemFn,
    args: OperationArgs,
    method_style: bool,
) -> proc_macro2::TokenStream {
    strip_spec_attrs(&mut function.attrs);
    let ident = function.sig.ident.clone();
    let mut rewriter = InlineCheckpointRewriter::new();
    function.block = Box::new(rewriter.fold_block(*function.block));
    let checkpoint_body = function.block;
    let symbol_expr = quote! { concat!(module_path!(), "::", stringify!(#ident)).to_string() };
    let instrumented = instrument_body(
        &function.sig,
        symbol_expr,
        checkpoint_body,
        &args.operation,
        args.kind,
    );
    function.block = instrumented;
    let _ = method_style;
    quote! { #function }
}

fn expand_operation_method(
    mut method: ImplItemFn,
    args: OperationArgs,
) -> proc_macro2::TokenStream {
    strip_spec_attrs(&mut method.attrs);
    let ident = method.sig.ident.clone();
    let mut rewriter = InlineCheckpointRewriter::new();
    method.block = rewriter.fold_block(method.block);
    let original = method.block;
    let symbol_expr = quote! { ::specgate::runtime::method_symbol::<Self>(stringify!(#ident)) };
    let instrumented = instrument_body(
        &method.sig,
        symbol_expr,
        Box::new(original),
        &args.operation,
        args.kind,
    );
    method.block = *instrumented;
    quote! { #method }
}

fn expand_passthrough_function(
    mut function: ItemFn,
    _args: NamedArgs,
    kind: MacroKind,
) -> proc_macro2::TokenStream {
    strip_spec_attrs(&mut function.attrs);
    if matches!(kind, MacroKind::Setup) && has_receiver(&function.sig) {
        return compile_error2("spec_setup must not take self");
    }
    quote! { #function }
}

fn expand_passthrough_method(
    mut method: ImplItemFn,
    _args: NamedArgs,
    kind: MacroKind,
) -> proc_macro2::TokenStream {
    strip_spec_attrs(&mut method.attrs);
    if matches!(kind, MacroKind::Setup) && has_receiver(&method.sig) {
        return compile_error2("spec_setup must not take self");
    }
    quote! { #method }
}

fn expand_checkpoint_function(
    mut function: ItemFn,
    args: OperationOnlyArgs,
) -> proc_macro2::TokenStream {
    strip_spec_attrs(&mut function.attrs);
    let ident = function.sig.ident.clone();
    let symbol_expr = quote! { concat!(module_path!(), "::", stringify!(#ident)).to_string() };
    let block = function.block.clone();
    function.block = wrap_checkpoint_body(&function.sig, block, symbol_expr, &args.operation);
    quote! { #function }
}

fn expand_checkpoint_method(
    mut method: ImplItemFn,
    args: OperationOnlyArgs,
) -> proc_macro2::TokenStream {
    strip_spec_attrs(&mut method.attrs);
    let ident = method.sig.ident.clone();
    let symbol_expr = quote! { ::specgate::runtime::method_symbol::<Self>(stringify!(#ident)) };
    let block = Box::new(method.block.clone());
    method.block = *wrap_checkpoint_body(&method.sig, block, symbol_expr, &args.operation);
    quote! { #method }
}

fn expand_mock_function(mut function: ItemFn, args: NamedArgs) -> proc_macro2::TokenStream {
    strip_spec_attrs(&mut function.attrs);
    function.block = wrap_mock_body(
        &function.sig,
        function.block.clone(),
        &args.operation,
        &args.name,
    );
    quote! { #function }
}

fn expand_mock_method(mut method: ImplItemFn, args: NamedArgs) -> proc_macro2::TokenStream {
    strip_spec_attrs(&mut method.attrs);
    method.block = *wrap_mock_body(
        &method.sig,
        Box::new(method.block.clone()),
        &args.operation,
        &args.name,
    );
    quote! { #method }
}

fn expand_spec_capture(input: DeriveInput) -> proc_macro2::TokenStream {
    let ident = input.ident;
    let Data::Struct(data) = input.data else {
        return compile_error2("SpecCapture can only be derived for structs");
    };
    let Fields::Named(fields) = data.fields else {
        return compile_error2("SpecCapture can only be derived for structs with named fields");
    };

    let struct_ops = parse_capture_attrs(&input.attrs);
    if let Err(message) = &struct_ops {
        return compile_error2(message);
    }
    let struct_ops = struct_ops.expect("checked above");

    let mut field_entries = Vec::new();
    let mut any_field_capture = false;
    for field in &fields.named {
        let field_ops = parse_capture_attrs(&field.attrs);
        if let Err(message) = &field_ops {
            return compile_error2(message);
        }
        let field_ops = field_ops.expect("checked above");
        if !field_ops.is_empty() {
            any_field_capture = true;
        }
        field_entries.push((field.clone(), field_ops));
    }

    if !struct_ops.is_empty() && any_field_capture {
        return compile_error2("spec_capture on struct and field cannot be combined");
    }

    let mut operations = std::collections::BTreeMap::<String, Vec<Field>>::new();
    if !struct_ops.is_empty() {
        for operation in struct_ops {
            let fields = fields
                .named
                .iter()
                .filter(|field| matches!(field.vis, Visibility::Public(_)))
                .cloned()
                .collect::<Vec<_>>();
            operations.insert(operation, fields);
        }
    } else {
        for (field, ops) in field_entries {
            for operation in ops {
                operations.entry(operation).or_default().push(field.clone());
            }
        }
    }

    let match_arms = operations.into_iter().map(|(operation, fields)| {
        let field_pushes = fields.iter().filter_map(|field| {
            let field_name = field.ident.as_ref()?;
            let field_label = field_name.to_string();
            Some(quote! {
                values.push(::specgate::runtime::SnapshotField::new(
                    #field_label,
                    ::specgate::runtime::stringify_value(&self.#field_name),
                ));
            })
        });
        quote! {
            #operation => {
                let mut values = Vec::new();
                #(#field_pushes)*
                values
            }
        }
    });

    quote! {
        impl ::specgate::runtime::CaptureSnapshot for #ident {
            fn specgate_snapshot(&self, operation: &str) -> Vec<::specgate::runtime::SnapshotField> {
                match operation {
                    #(#match_arms,)*
                    _ => Vec::new(),
                }
            }
        }
    }
}

fn instrument_body(
    sig: &Signature,
    symbol_expr: proc_macro2::TokenStream,
    body: Box<syn::Block>,
    operation: &str,
    kind: specgate::OperationKind,
) -> Box<syn::Block> {
    let operation_literal = LitStr::new(operation, proc_macro2::Span::call_site());
    let capture_after = if should_capture_return(&sig.output) {
        quote! {
            #[cfg(feature = "specgate")]
            {
                ::specgate::runtime::capture_after(#operation_literal, &specgate_result);
            }
        }
    } else {
        quote! {}
    };

    let capture_before_self =
        if matches!(kind, specgate::OperationKind::StateMachine) && has_receiver(sig) {
            quote! {
                #[cfg(feature = "specgate")]
                {
                    ::specgate::runtime::capture_before(#operation_literal, &*self);
                }
            }
        } else {
            quote! {}
        };

    let capture_after_self =
        if matches!(kind, specgate::OperationKind::StateMachine) && has_receiver(sig) {
            quote! {
                #[cfg(feature = "specgate")]
                {
                    ::specgate::runtime::capture_after(#operation_literal, &*self);
                }
            }
        } else {
            quote! {}
        };

    let body = if sig.asyncness.is_some() {
        quote! {
            {
                let specgate_symbol = #symbol_expr;
                #[cfg(feature = "specgate")]
                {
                    ::specgate::runtime::operation_enter(#operation_literal, &specgate_symbol);
                }
                #capture_before_self
                let specgate_result = (async move #body).await;
                #capture_after_self
                #capture_after
                #[cfg(feature = "specgate")]
                {
                    ::specgate::runtime::operation_exit(#operation_literal, &specgate_symbol);
                }
                specgate_result
            }
        }
    } else {
        quote! {
            {
                let specgate_symbol = #symbol_expr;
                #[cfg(feature = "specgate")]
                {
                    ::specgate::runtime::operation_enter(#operation_literal, &specgate_symbol);
                }
                #capture_before_self
                let specgate_result = (|| #body)();
                #capture_after_self
                #capture_after
                #[cfg(feature = "specgate")]
                {
                    ::specgate::runtime::operation_exit(#operation_literal, &specgate_symbol);
                }
                specgate_result
            }
        }
    };

    Box::new(syn::parse2(body).expect("instrumented body should parse"))
}

fn wrap_checkpoint_body(
    sig: &Signature,
    body: Box<syn::Block>,
    symbol_expr: proc_macro2::TokenStream,
    operation: &str,
) -> Box<syn::Block> {
    let operation_literal = LitStr::new(operation, proc_macro2::Span::call_site());
    let wrapped = if sig.asyncness.is_some() {
        quote! {
            {
                let specgate_symbol = #symbol_expr;
                let specgate_result = (async move #body).await;
                #[cfg(feature = "specgate")]
                {
                    let specgate_result = ::specgate::runtime::checkpoint(#operation_literal, &specgate_symbol, specgate_result);
                    specgate_result
                }
                #[cfg(not(feature = "specgate"))]
                {
                    specgate_result
                }
            }
        }
    } else {
        quote! {
            {
                let specgate_symbol = #symbol_expr;
                let specgate_result = (|| #body)();
                #[cfg(feature = "specgate")]
                {
                    let specgate_result = ::specgate::runtime::checkpoint(#operation_literal, &specgate_symbol, specgate_result);
                    specgate_result
                }
                #[cfg(not(feature = "specgate"))]
                {
                    specgate_result
                }
            }
        }
    };
    Box::new(syn::parse2(wrapped).expect("wrapped checkpoint body should parse"))
}

fn wrap_mock_body(
    sig: &Signature,
    body: Box<syn::Block>,
    operation: &str,
    mock_name: &str,
) -> Box<syn::Block> {
    let operation_literal = LitStr::new(operation, proc_macro2::Span::call_site());
    let mock_literal = LitStr::new(mock_name, proc_macro2::Span::call_site());
    let wrapped = if sig.asyncness.is_some() {
        quote! {
            {
                #[cfg(feature = "specgate")]
                if let Some(specgate_mocked) = ::specgate::runtime::mock_value(#mock_literal) {
                    ::specgate::runtime::mock_call(#operation_literal, #mock_literal, &specgate_mocked);
                    return specgate_mocked;
                }
                (async move #body).await
            }
        }
    } else {
        quote! {
            {
                #[cfg(feature = "specgate")]
                if let Some(specgate_mocked) = ::specgate::runtime::mock_value(#mock_literal) {
                    ::specgate::runtime::mock_call(#operation_literal, #mock_literal, &specgate_mocked);
                    return specgate_mocked;
                }
                (|| #body)()
            }
        }
    };
    let _ = sig;
    Box::new(syn::parse2(wrapped).expect("wrapped mock body should parse"))
}

fn strip_spec_attrs(attrs: &mut Vec<Attribute>) {
    attrs.retain(|attr| {
        !(attr.path().is_ident("spec_operation")
            || attr.path().is_ident("spec_setup")
            || attr.path().is_ident("spec_checkpoint")
            || attr.path().is_ident("spec_mock")
            || attr.path().is_ident("spec_capture"))
    });
}

fn has_receiver(sig: &Signature) -> bool {
    sig.inputs
        .iter()
        .any(|arg| matches!(arg, FnArg::Receiver(_)))
}

fn should_capture_return(output: &ReturnType) -> bool {
    match output {
        ReturnType::Default => false,
        ReturnType::Type(_, ty) => match &**ty {
            Type::Path(type_path) => {
                let Some(segment) = type_path.path.segments.last() else {
                    return false;
                };
                let ident = segment.ident.to_string();
                ident.chars().next().is_some_and(char::is_uppercase)
                    && !matches!(
                        ident.as_str(),
                        "String" | "Result" | "Option" | "Vec" | "Box" | "Self"
                    )
            }
            _ => false,
        },
    }
}

fn parse_capture_attrs(attrs: &[Attribute]) -> Result<Vec<String>, String> {
    let mut operations = Vec::new();
    for attr in attrs {
        if !attr.path().is_ident("spec_capture") {
            continue;
        }
        let args = attr
            .parse_args::<LitStr>()
            .map_err(|_| "missing operation name".to_string())?;
        operations.push(args.value());
    }
    Ok(operations)
}

fn compile_error2(message: &str) -> proc_macro2::TokenStream {
    quote! { compile_error!(#message); }
}

#[derive(Clone, Copy)]
enum MacroKind {
    Setup,
}

struct OperationArgs {
    operation: String,
    kind: specgate::OperationKind,
}

impl Parse for OperationArgs {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let args = Punctuated::<Expr, Token![,]>::parse_terminated(input)?
            .into_iter()
            .collect::<Vec<_>>();
        let Some(first) = args.first() else {
            return Err(syn::Error::new(input.span(), "missing operation name"));
        };
        let operation = match first {
            Expr::Lit(ExprLit {
                lit: Lit::Str(value),
                ..
            }) => value.value(),
            Expr::Assign(_) => return Err(syn::Error::new(first.span(), "missing operation name")),
            _ => return Err(syn::Error::new(first.span(), "missing operation name")),
        };
        let kind = find_named_ident(&args[1..], "kind")?
            .ok_or_else(|| syn::Error::new(input.span(), "missing required parameter: kind"))?;
        let kind = match kind.as_str() {
            "Stateless" => specgate::OperationKind::Stateless,
            "StateMachine" => specgate::OperationKind::StateMachine,
            "Sequence" => specgate::OperationKind::Sequence,
            "ErrorMap" => specgate::OperationKind::ErrorMap,
            "Structural" => specgate::OperationKind::Structural,
            other => {
                return Err(syn::Error::new(
                    input.span(),
                    format!("invalid kind: {other}"),
                ));
            }
        };
        Ok(Self { operation, kind })
    }
}

struct NamedArgs {
    operation: String,
    name: String,
}

impl Parse for NamedArgs {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let args = Punctuated::<Expr, Token![,]>::parse_terminated(input)?
            .into_iter()
            .collect::<Vec<_>>();
        let Some(first) = args.first() else {
            return Err(syn::Error::new(input.span(), "missing operation name"));
        };
        let operation = match first {
            Expr::Lit(ExprLit {
                lit: Lit::Str(value),
                ..
            }) => value.value(),
            Expr::Assign(_) => return Err(syn::Error::new(first.span(), "missing operation name")),
            _ => return Err(syn::Error::new(first.span(), "missing operation name")),
        };
        let name = find_named_lit(&args[1..], "name")?
            .ok_or_else(|| syn::Error::new(input.span(), "missing required parameter: name"))?;
        Ok(Self { operation, name })
    }
}

struct OperationOnlyArgs {
    operation: String,
}

impl Parse for OperationOnlyArgs {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let value = input
            .parse::<LitStr>()
            .map_err(|_| syn::Error::new(input.span(), "missing operation name"))?;
        Ok(Self {
            operation: value.value(),
        })
    }
}

struct InlineCheckpointArgs {
    _operation: LitStr,
    expr: Expr,
}

impl Parse for InlineCheckpointArgs {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let operation = input.parse::<LitStr>()?;
        input.parse::<Token![,]>()?;
        let expr = input.parse::<Expr>()?;
        Ok(Self {
            _operation: operation,
            expr,
        })
    }
}

fn find_named_ident(args: &[Expr], name: &str) -> syn::Result<Option<String>> {
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
            return Err(syn::Error::new(arg.span(), format!("invalid {name} value")));
        };
        let Some(segment) = path.segments.last() else {
            return Err(syn::Error::new(arg.span(), format!("invalid {name} value")));
        };
        return Ok(Some(segment.ident.to_string()));
    }
    Ok(None)
}

fn find_named_lit(args: &[Expr], name: &str) -> syn::Result<Option<String>> {
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
            return Err(syn::Error::new(arg.span(), format!("invalid {name} value")));
        };
        return Ok(Some(value.value()));
    }
    Ok(None)
}

struct InlineCheckpointRewriter {
    next_index: usize,
}

impl InlineCheckpointRewriter {
    fn new() -> Self {
        Self { next_index: 1 }
    }
}

impl Fold for InlineCheckpointRewriter {
    fn fold_expr(&mut self, expr: Expr) -> Expr {
        if let Expr::Macro(ExprMacro { mac, .. }) = &expr {
            if mac.path.is_ident("spec_checkpoint") {
                let args = InlineCheckpointArgs::parse.parse2(mac.tokens.clone());
                if let Ok(args) = args {
                    let checkpoint_index = self.next_index;
                    self.next_index += 1;
                    let operation = args._operation;
                    let inner_expr = args.expr;
                    let symbol_suffix = LitStr::new(
                        &format!("checkpoint_{checkpoint_index}"),
                        proc_macro2::Span::call_site(),
                    );
                    return syn::parse2(quote! {
                        {
                            let specgate_value = #inner_expr;
                            #[cfg(feature = "specgate")]
                            {
                                let specgate_symbol = format!("{}::{}", specgate_symbol, #symbol_suffix);
                                let specgate_value = ::specgate::runtime::checkpoint(#operation, &specgate_symbol, specgate_value);
                                specgate_value
                            }
                            #[cfg(not(feature = "specgate"))]
                            {
                                specgate_value
                            }
                        }
                    })
                    .expect("rewritten checkpoint expression should parse");
                }
            }
        }
        syn::fold::fold_expr(self, expr)
    }
}
