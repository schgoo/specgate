//! Invocation code expanded only in its operation or receiver producer's scope.

use super::{Link, Plan, model};

pub(super) fn source(plan: &Plan) -> String {
    let mut arms = Vec::new();
    let package = plan.target.package_name.as_str();
    for link in &plan.links {
        let component = link.component_id.as_str();
        let operation = link.operation_name.as_str();
        let body = invocation(link);
        let components = [quote::quote!(#component), quote::quote!(crate::__SPECGATE_COMPONENT)];
        if link.is_method {
            let setup = link.setups.iter().find(|setup| setup.fills.is_empty()).expect("validated receiver");
            let function = &setup.fn_name;
            let fills = &setup.declared_fills;
            let bridge = quote::format_ident!("__SPECGATE_replay_{}", link.fn_name.trim_start_matches("r#"));
            for component in components {
                arms.push(quote::quote! {
                    (#package, setup, #component, #operation, #function, #fills, $output:ty) => {
                        Some::<__specgate::harness::Invocation>(|receiver, inputs, setups| {
                            <$output>::#bridge(#operation, receiver, inputs, setups)
                        })
                    };
                    (#package, method, #component, #operation, $function:ident, [$($types:ty),*]) => {
                        Some::<__specgate::harness::Invocation>(|receiver, inputs, setups| {
                            Box::pin(async move {
                                let receiver = receiver.downcast_mut::<Self>()
                                    .ok_or_else(|| __specgate::harness::error("selected setup returned a different receiver type"))?;
                                #body
                            })
                        })
                    };
                });
            }
        } else {
            for component in components {
                arms.push(quote::quote! {
                    (#package, operation, #component, #operation, $function:ident, [$($types:ty),*]) => {
                        Some::<__specgate::harness::Invocation>(|_, inputs, setups| {
                            Box::pin(async move { #body })
                        })
                    };
                });
            }
        }
    }
    quote::quote! {{
        macro_rules! __specgate_dispatch {
            #(#arms)*
            ($($unused:tt)*) => { None::<__specgate::harness::Invocation> };
        }
        __specgate_selected!()
    }}
    .to_string()
}

fn invocation(link: &Link) -> impl quote::ToTokens {
    let mut bindings = Vec::new();
    let mut arguments = Vec::new();
    for (position, parameter) in link.parameters.iter().enumerate() {
        let local = quote::format_ident!("__sg_argument_{position}");
        match parameter {
            model::Parameter::Input(index) => {
                let definition = &link.inputs[*index];
                let borrowed = definition.rust_type.replace(' ', "") == "&str";
                let native: syn::Type = if borrowed {
                    syn::parse_quote!(String)
                } else {
                    syn::parse_str(&definition.rust_type).expect("validated primitive input")
                };
                bindings.push(quote::quote! {
                    let #local = <#native as __specgate::harness::Input>::decode(
                        inputs.get(#index).ok_or_else(|| __specgate::harness::error("missing operation input"))?.clone()
                    )?;
                });
                arguments.push(if borrowed { quote::quote!(&#local) } else { quote::quote!(#local) });
            }
            model::Parameter::Setup { borrow, .. } => {
                bindings.push(quote::quote! {
                    let mut #local = __specgate::harness::take(
                        setups.next().ok_or_else(|| __specgate::harness::error("missing injected setup value"))?
                    )?;
                });
                arguments.push(match borrow.as_str() {
                    "&" => quote::quote!(&#local),
                    "&mut " => quote::quote!(&mut #local),
                    "" => quote::quote!(#local),
                    _ => unreachable!("validated borrow syntax"),
                });
            }
        }
    }
    let call = if link.is_method {
        if link.type_arguments.is_empty() {
            quote::quote!(receiver.$function)
        } else {
            quote::quote!(receiver.$function::<$($types),*>)
        }
    } else if link.type_arguments.is_empty() {
        quote::quote!($function)
    } else {
        quote::quote!($function::<$($types),*>)
    };
    let awaited = link.is_async.then(|| quote::quote!(.await));
    quote::quote! {
        let mut setups = setups.into_iter();
        #(#bindings)*
        if setups.next().is_some() {
            return Err(__specgate::harness::error("unexpected injected setup value"));
        }
        let _ = #call(#(#arguments),*) #awaited;
        Ok(())
    }
}
