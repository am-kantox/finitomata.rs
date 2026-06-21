use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use crate::parser::{EventKind, ParsedFsm};

pub struct CodegenConfig {
    pub struct_name: syn::Ident,
    pub timer: Option<u64>,
    pub auto_terminate: bool,
    #[allow(dead_code)]
    pub cache_state: bool,
}

pub fn generate(fsm: &ParsedFsm, config: &CodegenConfig) -> TokenStream {
    let struct_name = &config.struct_name;
    let state_enum_name = format_ident!("{}State", struct_name);
    let event_enum_name = format_ident!("{}Event", struct_name);

    // Generate state variants
    let state_variants: Vec<_> = fsm
        .states
        .iter()
        .map(|s| format_ident!("{}", to_pascal_case(s)))
        .collect();

    // Generate event variants
    let event_names: Vec<String> = fsm
        .transitions
        .iter()
        .map(|t| t.event.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();

    let event_variants: Vec<_> = event_names
        .iter()
        .map(|e| format_ident!("{}", to_pascal_case(e)))
        .collect();

    // Generate event kind mapping (deduplicated by event name)
    let mut seen_events = std::collections::BTreeSet::new();
    let event_kind_arms: Vec<_> = fsm
        .transitions
        .iter()
        .filter(|t| seen_events.insert(t.event.clone()))
        .map(|t| {
            let variant = format_ident!("{}", to_pascal_case(&t.event));
            let kind = match t.kind {
                EventKind::Hard => quote! { ::finitomata::EventKind::Hard },
                EventKind::Soft => quote! { ::finitomata::EventKind::Soft },
                EventKind::Normal => quote! { ::finitomata::EventKind::Normal },
            };
            quote! { #event_enum_name::#variant => #kind }
        })
        .collect();

    // Generate transition table entries
    let transition_entries: Vec<_> = fsm
        .transitions
        .iter()
        .map(|t| {
            let from = format_ident!("{}", to_pascal_case(&t.from));
            let event = format_ident!("{}", to_pascal_case(&t.event));
            let to = if t.to == "__terminal__" {
                // For terminal transitions, use the from state (will be handled by auto_terminate)
                format_ident!("{}", to_pascal_case(&t.from))
            } else {
                format_ident!("{}", to_pascal_case(&t.to))
            };
            let kind = match t.kind {
                EventKind::Hard => quote! { ::finitomata::EventKind::Hard },
                EventKind::Soft => quote! { ::finitomata::EventKind::Soft },
                EventKind::Normal => quote! { ::finitomata::EventKind::Normal },
            };
            quote! {
                ::finitomata::Transition {
                    from: #state_enum_name::#from,
                    to: vec![#state_enum_name::#to],
                    event: #event_enum_name::#event,
                    kind: #kind,
                }
            }
        })
        .collect();

    // Initial state
    let initial_variant = format_ident!("{}", to_pascal_case(&fsm.initial));

    // Final states
    let final_variants: Vec<_> = fsm
        .finals
        .iter()
        .map(|s| format_ident!("{}", to_pascal_case(s)))
        .collect();

    // State display arms
    let state_display_arms: Vec<_> = fsm
        .states
        .iter()
        .map(|s| {
            let variant = format_ident!("{}", to_pascal_case(s));
            let name = s.as_str();
            quote! { Self::#variant => write!(f, #name) }
        })
        .collect();

    // Event display arms
    let event_display_arms: Vec<_> = event_names
        .iter()
        .map(|e| {
            let variant = format_ident!("{}", to_pascal_case(e));
            let name = e.as_str();
            quote! { Self::#variant => write!(f, #name) }
        })
        .collect();

    let auto_terminate_val = config.auto_terminate;

    let timer_setup = if let Some(ms) = config.timer {
        quote! {
            pub const TIMER_INTERVAL_MS: u64 = #ms;
        }
    } else {
        quote! {}
    };

    let graph_fn_name = format_ident!("{}_graph", to_snake_case(&struct_name.to_string()));

    quote! {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum #state_enum_name {
            #(#state_variants),*
        }

        impl ::std::fmt::Display for #state_enum_name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                match self {
                    #(#state_display_arms),*
                }
            }
        }

        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum #event_enum_name {
            #(#event_variants),*
        }

        impl ::std::fmt::Display for #event_enum_name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                match self {
                    #(#event_display_arms),*
                }
            }
        }

        impl #event_enum_name {
            pub fn kind(&self) -> ::finitomata::EventKind {
                match self {
                    #(#event_kind_arms),*
                }
            }
        }

        impl #struct_name {
            pub const AUTO_TERMINATE: bool = #auto_terminate_val;

            #timer_setup

            pub fn build_graph() -> ::finitomata::TransitionGraph<#state_enum_name, #event_enum_name> {
                let transitions = vec![
                    #(#transition_entries),*
                ];
                let finals = ::std::collections::BTreeSet::from([
                    #(#state_enum_name::#final_variants),*
                ]);
                ::finitomata::TransitionGraph::new(
                    #state_enum_name::#initial_variant,
                    finals,
                    transitions,
                )
            }
        }

        pub fn #graph_fn_name() -> ::finitomata::TransitionGraph<#state_enum_name, #event_enum_name> {
            #struct_name::build_graph()
        }
    }
}

fn to_pascal_case(s: &str) -> String {
    s.split('_')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                None => String::new(),
                Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect()
}

fn to_snake_case(s: &str) -> String {
    let mut result = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                result.push('_');
            }
            result.push(c.to_lowercase().next().unwrap());
        } else {
            result.push(c);
        }
    }
    result
}
