mod codegen;
mod parser;
mod validation;

use proc_macro::TokenStream;
use quote::quote;
use syn::{DeriveInput, Expr, Lit, Meta, MetaNameValue, parse_macro_input};

/// Procedural macro for defining finite state machines.
///
/// # Example
/// ```rust,ignore
/// #[finitomata(
///     fsm = r#"
///         [*] --> idle
///         idle --> |start!| running
///         running --> |pause| paused
///         running --> |stop| idle
///         paused --> |resume| running
///         idle --> |shutdown| [*]
///     "#,
///     syntax = "mermaid",
///     timer = 5000,
///     auto_terminate = true,
/// )]
/// #[derive(Debug, Clone)]
/// struct MyWorkflow {
///     counter: u32,
/// }
/// ```
#[proc_macro_attribute]
pub fn finitomata(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);
    let struct_name = input.ident.clone();

    // Parse attributes
    let attr_args = match parse_finitomata_attrs(attr) {
        Ok(args) => args,
        Err(err) => {
            return syn::Error::new(proc_macro2::Span::call_site(), err)
                .to_compile_error()
                .into();
        }
    };

    // Parse the FSM definition
    let fsm_def = match &attr_args.syntax {
        Syntax::Mermaid => parser::parse_mermaid(&attr_args.fsm),
        Syntax::PlantUml => parser::parse_plantuml(&attr_args.fsm),
    };

    let fsm = match fsm_def {
        Ok(fsm) => fsm,
        Err(err) => {
            return syn::Error::new(
                proc_macro2::Span::call_site(),
                format!("FSM parse error: {err}"),
            )
            .to_compile_error()
            .into();
        }
    };

    // Validate the FSM
    if let Err(errors) = validation::validate(&fsm) {
        let msg = errors.join("; ");
        return syn::Error::new(
            proc_macro2::Span::call_site(),
            format!("FSM validation error: {msg}"),
        )
        .to_compile_error()
        .into();
    }

    // Generate code
    let config = codegen::CodegenConfig {
        struct_name: struct_name.clone(),
        timer: attr_args.timer,
        auto_terminate: attr_args.auto_terminate,
        cache_state: attr_args.cache_state,
    };

    let generated = codegen::generate(&fsm, &config);

    let expanded = quote! {
        #input

        #generated
    };

    expanded.into()
}

#[derive(Debug, Clone, Copy)]
enum Syntax {
    Mermaid,
    PlantUml,
}

struct FinitomataAttrs {
    fsm: String,
    syntax: Syntax,
    timer: Option<u64>,
    auto_terminate: bool,
    cache_state: bool,
}

struct AttrList(syn::punctuated::Punctuated<Meta, syn::Token![,]>);

impl syn::parse::Parse for AttrList {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        Ok(Self(syn::punctuated::Punctuated::parse_terminated(input)?))
    }
}

fn parse_finitomata_attrs(attr: TokenStream) -> Result<FinitomataAttrs, String> {
    let attr2: proc_macro2::TokenStream = attr.into();

    let mut fsm = None;
    let mut syntax = Syntax::Mermaid;
    let mut timer = None;
    let mut auto_terminate = false;
    let mut cache_state = true;

    let parsed: AttrList =
        syn::parse2(attr2).map_err(|e| format!("failed to parse attributes: {e}"))?;

    for meta in parsed.0 {
        if let Meta::NameValue(MetaNameValue { path, value, .. }) = meta {
            let key = path.get_ident().map(|i| i.to_string()).unwrap_or_default();

            match key.as_str() {
                "fsm" => {
                    if let Expr::Lit(expr_lit) = &value
                        && let Lit::Str(lit) = &expr_lit.lit
                    {
                        fsm = Some(lit.value());
                    }
                }
                "syntax" => {
                    if let Expr::Lit(expr_lit) = &value
                        && let Lit::Str(lit) = &expr_lit.lit
                    {
                        syntax = match lit.value().as_str() {
                            "plantuml" | "plant_uml" => Syntax::PlantUml,
                            _ => Syntax::Mermaid,
                        };
                    }
                }
                "timer" => {
                    if let Expr::Lit(expr_lit) = &value
                        && let Lit::Int(lit) = &expr_lit.lit
                    {
                        timer = lit.base10_parse::<u64>().ok();
                    }
                }
                "auto_terminate" => {
                    if let Expr::Lit(expr_lit) = &value
                        && let Lit::Bool(lit) = &expr_lit.lit
                    {
                        auto_terminate = lit.value;
                    }
                }
                "cache_state" => {
                    if let Expr::Lit(expr_lit) = &value
                        && let Lit::Bool(lit) = &expr_lit.lit
                    {
                        cache_state = lit.value;
                    }
                }
                _ => {}
            }
        }
    }

    let fsm = fsm.ok_or_else(|| "missing required `fsm` attribute".to_string())?;

    Ok(FinitomataAttrs {
        fsm,
        syntax,
        timer,
        auto_terminate,
        cache_state,
    })
}
