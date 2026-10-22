//! Invocation code expanded only in its operation or receiver producer's scope.

use super::{Link, Plan, model};

pub(super) fn source(plan: &Plan, module: Option<&str>) -> String {
    let mut arms = Vec::new();
    let package = plan.target.package_name.as_str();
    for link in &plan.links {
        let component_id = link.component_id.as_str();
        let component = component(component_id, link.explicit_component);
        let operation = link.operation_name.as_str();
        let body = invocation(link);
        for parameter in &link.parameters {
            if let model::Parameter::Setup {
                index,
                borrow,
                projection: Some(_),
            } = parameter
            {
                let setup = &link.setups[*index];
                if module != Some(setup.native_module.as_str()) {
                    continue;
                }
                let function = &setup.fn_name;
                let fills = &setup.declared_fills;
                let project = if borrow == "&mut " {
                    quote::quote!(__specgate::harness::borrowed_mut)
                } else {
                    quote::quote!(__specgate::harness::borrowed)
                };
                let component = self::component(component_id, setup.explicit_component);
                arms.push(quote::quote! {
                    (input, #package, #component, #operation, #function, #fills, $output:ty) => {
                        Some::<__specgate::harness::Projection>(#project::<$output>)
                    };
                });
            }
        }
        if link.is_method {
            let setup = link.setups.iter().find(|setup| setup.fills.is_empty()).expect("validated receiver");
            let function = &setup.fn_name;
            let fills = &setup.declared_fills;
            let bridge = quote::format_ident!("__SPECGATE_replay_{}", link.fn_name.trim_start_matches("r#"));
            if module == Some(setup.native_module.as_str()) {
                let component = self::component(component_id, setup.explicit_component);
                arms.push(quote::quote! {
                    (#package, setup, #component, #operation, #function, #fills, $output:ty) => {
                        Some::<__specgate::harness::Invocation>(|receiver, inputs, setups| {
                            <$output>::#bridge(#operation, receiver, inputs, setups)
                        })
                    };
                });
            }
            if module == Some(link.native_module.as_str()) {
                arms.push(quote::quote! {
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
        } else if module == Some(link.native_module.as_str()) {
            arms.push(quote::quote! {
                (#package, operation, #component, #operation, $function:ident, [$($types:ty),*]) => {
                    Some::<__specgate::harness::Invocation>(|_, inputs, setups| {
                        Box::pin(async move { #body })
                    })
                };
            });
        }
    }
    quote::quote! {{
        macro_rules! __specgate_dispatch {
            #(#arms)*
            (input, $($unused:tt)*) => { None::<__specgate::harness::Projection> };
            ($($unused:tt)*) => { None::<__specgate::harness::Invocation> };
        }
        __specgate_selected!()
    }}
    .to_string()
}

fn component(value: &str, explicit: bool) -> impl quote::ToTokens {
    if explicit {
        quote::quote!(#value)
    } else {
        quote::quote!(crate::__SPECGATE_COMPONENT)
    }
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
            model::Parameter::Setup { borrow, projection, .. } => {
                let annotation = projection.as_ref().map(|target| {
                    let target: syn::Type = syn::parse_str(target).expect("validated borrowed target");
                    if borrow == "&mut " {
                        quote::quote!(: ::std::boxed::Box<dyn ::std::ops::DerefMut<Target = #target>>)
                    } else {
                        quote::quote!(: ::std::boxed::Box<dyn ::std::ops::Deref<Target = #target>>)
                    }
                });
                bindings.push(quote::quote! {
                    let mut #local #annotation = __specgate::harness::take(
                        setups.next().ok_or_else(|| __specgate::harness::error("missing injected setup value"))?
                    )?;
                });
                arguments.push(match (borrow.as_str(), projection.is_some()) {
                    ("&", true) => quote::quote!(&**#local),
                    ("&mut ", true) => quote::quote!(&mut **#local),
                    ("&", false) => quote::quote!(&#local),
                    ("&mut ", false) => quote::quote!(&mut #local),
                    ("", false) => quote::quote!(#local),
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
        let _ = &inputs;
        let mut setups = setups.into_iter();
        #(#bindings)*
        if setups.next().is_some() {
            return Err(__specgate::harness::error("unexpected injected setup value"));
        }
        let _ = #call(#(#arguments),*) #awaited;
        Ok(())
    }
}
