#![doc = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/README.md"))]

use proc_macro::TokenStream;

use quote::quote;
use syn::{parse_macro_input, AttributeArgs, DataEnum, DeriveInput, Path};

// Merges the variants of two enums.
fn merge_variants(metadata: TokenStream, left: TokenStream, right: TokenStream) -> TokenStream {
    use syn::Data::Enum;

    let args = parse_macro_input!(metadata as AttributeArgs);
    if let Some(first_arg) = args.first() {
        return syn::Error::new_spanned(first_arg, "macro takes no arguments")
            .to_compile_error()
            .into();
    }

    let mut left: DeriveInput = parse_macro_input!(left);
    let right: DeriveInput = parse_macro_input!(right);

    if let (
        Enum(DataEnum { variants, .. }),
        Enum(DataEnum {
            variants: to_add, ..
        }),
    ) = (&mut left.data, right.data)
    {
        variants.extend(to_add.into_iter());

        quote! { #left }.into()
    } else {
        syn::Error::new(left.ident.span(), "variants may only be added for enums")
            .to_compile_error()
            .into()
    }
}

/// Gets the cwd_interface path for something exported by
/// cwd_interface. If we are currently compiling the cwd-interface
/// crate, `crate::{internal}` is returned. If we are not,
/// `::cwd_interface::{internal}` is returned.
fn cwd_interface_path(inside: &str) -> Path {
    let pkg = std::env::var("CARGO_PKG_NAME").unwrap();
    let base = if pkg == "cwd-interface" {
        "crate"
    } else {
        "::cwd_interface"
    };
    let path = format!("{base}::{inside}");
    let path: Path = syn::parse_str(&path).unwrap();
    path
}

/// Adds the necessary fields to an enum such that the enum implements the
/// interface needed to be a voting module.
///
/// For example:
///
/// ```
/// use cwd_macros::voting_module_query;
/// use cosmwasm_schema::{cw_serde, QueryResponses};
/// use cwd_interface::voting::TotalPowerAtHeightResponse;
/// use cwd_interface::voting::VotingPowerAtHeightResponse;
///
/// #[voting_module_query]
/// #[cw_serde]
/// #[derive(QueryResponses)]
/// enum QueryMsg {}
///
/// ```
/// Will transform the enum to:
///
/// ```
///
/// enum QueryMsg {
///     VotingPowerAtHeight {
///       address: String,
///       height: Option<u64>
///     },
///     TotalPowerAtHeight {
///       height: Option<u64>
///     },
///     Dao {},
///     Info {},
/// }
/// ```
///
/// Note that other derive macro invocations must occur after this
/// procedural macro as they may depend on the new fields. For
/// example, the following will fail becase the `Clone` derivation
/// occurs before the addition of the field.
///
/// ```compile_fail
/// use cwd_macros::voting_module_query;
/// use cosmwasm_schema::{cw_serde, QueryResponses};
/// use cwd_interface::voting::TotalPowerAtHeightResponse;
/// use cwd_interface::voting::VotingPowerAtHeightResponse;
///
/// #[derive(Clone)]
/// #[voting_module_query]
/// #[allow(dead_code)]
/// #[cw_serde]
/// #[derive(QueryResponses)]
/// enum Test {
///     #[returns(String)]
///     Foo,
///     #[returns(String)]
///     Bar(u64),
///     #[returns(String)]
///     Baz { foo: u64 },
/// }
/// ```
#[proc_macro_attribute]
pub fn voting_module_query(metadata: TokenStream, input: TokenStream) -> TokenStream {
    let i = cwd_interface_path("voting::InfoResponse");
    let vp = cwd_interface_path("voting::VotingPowerAtHeightResponse");
    let tp = cwd_interface_path("voting::TotalPowerAtHeightResponse");

    merge_variants(
        metadata,
        input,
        quote! {
        enum Right {
            /// Returns the voting power for an address at a given height.
            #[returns(#vp)]
            VotingPowerAtHeight {
                address: ::std::string::String,
                height: ::std::option::Option<::std::primitive::u64>
            },
            /// Returns the total voting power at a given block heigh.
            #[returns(#tp)]
            TotalPowerAtHeight {
                height: ::std::option::Option<::std::primitive::u64>
            },
            /// Returns the address of the DAO this module belongs to.
            #[returns(cosmwasm_std::Addr)]
            Dao {},
            /// Returns contract version info.
            #[returns(#i)]
            Info {}
        }
        }
        .into(),
    )
}

/// Adds the necessary fields to an enum such that it implements the
/// interface needed to be a voting module with a token.
///
/// For example:
///
/// ```
/// use cwd_macros::token_query;
/// use cosmwasm_schema::{cw_serde, QueryResponses};
/// use cosmwasm_std::Addr;
///
/// #[token_query]
/// #[cw_serde]
/// #[derive(QueryResponses)]
/// enum QueryMsg {}
/// ```
///
/// Will transform the enum to:
///
/// ```
/// enum QueryMsg {
///     TokenContract {},
/// }
/// ```
///
/// Note that other derive macro invocations must occur after this
/// procedural macro as they may depend on the new fields. For
/// example, the following will fail becase the `Clone` derivation
/// occurs before the addition of the field.
///
/// ```compile_fail
/// use cwd_macros::token_query;
/// use cosmwasm_schema::{cw_serde, QueryResponses};
///
/// #[derive(Clone)]
/// #[token_query]
/// #[allow(dead_code)]
/// #[cw_serde]
/// #[derive(QueryResponses)]
/// enum Test {
///     Foo,
///     Bar(u64),
///     Baz { foo: u64 },
/// }
/// ```
#[proc_macro_attribute]
pub fn token_query(metadata: TokenStream, input: TokenStream) -> TokenStream {
    merge_variants(
        metadata,
        input,
        quote! {
        enum Right {
            #[returns(::cosmwasm_std::Addr)]
            TokenContract {}
        }
        }
        .into(),
    )
}

/// Adds the necessary fields to an enum such that it implements the
/// interface needed to be a voting module that has an
/// active check threshold.
///
/// For example:
///
/// ```
/// use cwd_macros::active_query;
/// use cosmwasm_schema::{cw_serde, QueryResponses};
///
/// #[active_query]
/// #[cw_serde]
/// #[derive(QueryResponses)]
/// enum QueryMsg {}
/// ```
///
/// Will transform the enum to:
///
/// ```
/// enum QueryMsg {
///     IsActive {},
/// }
/// ```
///
/// Note that other derive macro invocations must occur after this
/// procedural macro as they may depend on the new fields. For
/// example, the following will fail becase the `Clone` derivation
/// occurs before the addition of the field.
///
/// ```compile_fail
/// use cwd_macros::active_query;
///
/// #[derive(Clone)]
/// #[active_query]
/// #[allow(dead_code)]
/// enum Test {
///     Foo,
///     Bar(u64),
///     Baz { foo: u64 },
/// }
/// ```
#[proc_macro_attribute]
pub fn active_query(metadata: TokenStream, input: TokenStream) -> TokenStream {
    merge_variants(
        metadata,
        input,
        quote! {
        enum Right {
            #[returns(::std::primitive::bool)]
            IsActive {}
        }
        }
        .into(),
    )
}

/// Adds the necessary fields to an enum such that it implements the
/// interface needed to be a proposal module.
///
/// For example:
///
/// ```
/// use cwd_macros::proposal_module_query;
/// use cosmwasm_schema::{cw_serde, QueryResponses};
/// use cosmwasm_std::Addr;
///
/// #[proposal_module_query]
/// #[cw_serde]
/// #[derive(QueryResponses)]
/// enum QueryMsg {}
/// ```
///
/// Will transform the enum to:
///
/// ```
/// enum QueryMsg {
///     Dao {},
///     Info {},
///     ProposalCreationPolicy {},
///     ProposalHooks {},
/// }
/// ```
///
/// Note that other derive macro invocations must occur after this
/// procedural macro as they may depend on the new fields. For
/// example, the following will fail becase the `Clone` derivation
/// occurs before the addition of the field.
///
/// ```compile_fail
/// use cwd_macros::proposal_module_query;
/// use cosmwasm_schema::{cw_serde, QueryResponses};
/// use cosmwasm_std::Addr;
///
/// #[derive(Clone)]
/// #[proposal_module_query]
/// #[allow(dead_code)]
/// #[cw_serde]
/// #[derive(QueryResponses)]
/// enum Test {
///     Foo,
///     Bar(u64),
///     Baz { foo: u64 },
/// }
/// ```
#[proc_macro_attribute]
pub fn proposal_module_query(metadata: TokenStream, input: TokenStream) -> TokenStream {
    let i = cwd_interface_path("voting::InfoResponse");

    merge_variants(
        metadata,
        input,
        quote! {
        enum Right {
            /// Returns the address of the DAO this module belongs to
            #[returns(::cosmwasm_std::Addr)]
            Dao {},
            /// Returns contract version info
            #[returns(#i)]
            Info { },
            /// Returns the number of proposals that have been created in this module.
            #[returns(::std::primitive::u64)]
            ProposalCount {},
            /// Gets the current proposal creation policy for this module.
            #[returns(::cwd_voting::pre_propose::ProposalCreationPolicy)]
            ProposalCreationPolicy {},
            /// Lists all of the consumers of proposal hooks for this module.
            #[returns(::cwd_hooks::HooksResponse)]
            ProposalHooks {},
            /// Lists all of the consumers of vote hooks for this module.
            #[returns(::cwd_hooks::HooksResponse)]
            VoteHooks {},
        }
        }
        .into(),
    )
}

/// Limits the number of variants allowed on an enum at compile
/// time. For example, the following will not compile:
///
/// ```compile_fail
/// use cwd_macros::limit_variant_count;
///
/// #[limit_variant_count(1)]
/// enum Two {
///     One {},
///     Two {},
/// }
/// ```
#[proc_macro_attribute]
pub fn limit_variant_count(metadata: TokenStream, input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(metadata as AttributeArgs);
    if args.len() != 1 {
        panic!("macro takes one argument. ex: `#[limit_variant_count(4)]`")
    }

    let limit: usize = if let syn::NestedMeta::Lit(syn::Lit::Int(unparsed)) = args.first().unwrap()
    {
        match unparsed.base10_parse() {
            Ok(limit) => limit,
            Err(e) => return e.to_compile_error().into(),
        }
    } else {
        return syn::Error::new_spanned(args[0].clone(), "argument should be an integer literal")
            .to_compile_error()
            .into();
    };

    let ast: DeriveInput = parse_macro_input!(input);
    match ast.data {
        syn::Data::Enum(DataEnum { ref variants, .. }) => {
            if variants.len() > limit {
                return syn::Error::new_spanned(
                    variants[limit].clone(),
                    format!("this enum's variant count is limited to {limit}"),
                )
                .to_compile_error()
                .into();
            }
        }
        _ => {
            return syn::Error::new(
                ast.ident.span(),
                "limit_variant_count may only be derived for enums",
            )
            .to_compile_error()
            .into()
        }
    };

    quote! {
    #ast
    }
    .into()
}
