use darling::{util::Flag, FromDeriveInput, FromField, FromVariant};
use proc_macro2::TokenStream;
use quote::{format_ident, quote, quote_spanned};
use syn::{DeriveInput, Ident, Index, Member, Path};

use crate::generators::{self as gen, CodedVariant};

#[derive(FromDeriveInput)]
#[darling(supports(enum_any), attributes(sdk_error))]
struct Error {
    ident: Ident,

    data: darling::ast::Data<ErrorVariant, darling::util::Ignored>,

    /// The path to a const set to the module name.
    #[darling(default)]
    module_name: Option<syn::Path>,

    /// Whether to sequentially autonumber the error codes.
    /// This option exists as a convenience for runtimes that
    /// only append errors or release only breaking changes.
    #[darling(default, rename = "autonumber")]
    autonumber: Flag,
}

#[derive(FromVariant)]
#[darling(attributes(sdk_error))]
struct ErrorVariant {
    ident: Ident,

    fields: darling::ast::Fields<ErrorField>,

    /// The explicit ID of the error code. Overrides any autonumber set on the error enum.
    #[darling(default, rename = "code")]
    code: Option<u32>,

    #[darling(default, rename = "transparent")]
    transparent: Flag,

    #[darling(default, rename = "abort")]
    abort: Flag,
}

impl CodedVariant for ErrorVariant {
    const FIELD_NAME: &'static str = "code";

    fn ident(&self) -> &Ident {
        &self.ident
    }

    fn code(&self) -> Option<u32> {
        self.code
    }
}

#[derive(FromField)]
#[darling(forward_attrs(source, from))]
struct ErrorField {
    ident: Option<Ident>,

    attrs: Vec<syn::Attribute>,
}

pub fn derive_error(input: DeriveInput) -> TokenStream {
    let error = match Error::from_derive_input(&input) {
        Ok(error) => error,
        Err(e) => return e.write_errors(),
    };

    let error_ty_ident = &error.ident;

    let module_name = error
        .module_name
        .unwrap_or_else(|| syn::parse_quote!(MODULE_NAME));

    let (module_name_body, code_body, abort_body) = convert_variants(
        &format_ident!("self"),
        module_name,
        &error.data.as_ref().take_enum().unwrap(),
        error.autonumber.is_some(),
    );

    let sdk_crate = gen::sdk_crate_path();

    gen::wrap_in_const(quote! {
        use #sdk_crate::{self as __sdk, error::Error as _};

        #[automatically_derived]
        impl __sdk::error::Error for #error_ty_ident {
            fn module_name(&self) -> &str {
                #module_name_body
            }

            fn code(&self) -> u32 {
                #code_body
            }

            fn into_abort(self) -> Result<__sdk::dispatcher::Error, Self> {
                #abort_body
            }
        }

        #[automatically_derived]
        impl From<#error_ty_ident> for __sdk::error::RuntimeError {
            fn from(err: #error_ty_ident) -> Self {
                Self::new(err.module_name(), err.code(), &err.to_string())
            }
        }
    })
}

fn convert_variants(
    enum_binding: &Ident,
    module_name: Path,
    variants: &[&ErrorVariant],
    autonumber: bool,
) -> (TokenStream, TokenStream, TokenStream) {
    if variants.is_empty() {
        return (quote!(#module_name), quote!(0), quote!(Err(#enum_binding)));
    }

    let mut next_autonumber = 0u32;
    let mut reserved_numbers = std::collections::BTreeSet::new();

    let abort_variants: Vec<_> = variants
        .iter()
        .filter_map(|variant| {
            if variant.abort.is_none() {
                return None;
            }

            let variant_ident = &variant.ident;

            Some(quote! {
                match #enum_binding {
                    Self::#variant_ident(err) => Ok(err),
                    _ => Err(#enum_binding),
                }
            })
        })
        .collect();
    let abort_variant = match abort_variants.len() {
        0 => quote!(Err(#enum_binding)),
        1 => abort_variants.into_iter().next().unwrap(),
        _ => {
            enum_binding
                .span()
                .unwrap()
                .error("multiple abort variants specified")
                .emit();
            return (quote!(), quote!(), quote!());
        }
    };

    let (module_name_matches, code_matches): (Vec<_>, Vec<_>) = variants
        .iter()
        .map(|variant| {
            let variant_ident = &variant.ident;

            if variant.transparent.is_some() {
                // Transparently forward everything to the source.
                let mut maybe_sources = variant
                    .fields
                    .iter()
                    .enumerate()
                    .filter_map(|(i, f)| (!f.attrs.is_empty()).then(|| (i, f.ident.clone())));
                let source = maybe_sources.next();
                if maybe_sources.count() != 0 {
                    variant_ident
                        .span()
                        .unwrap()
                        .error("multiple error sources specified for variant")
                        .emit();
                    return (quote!(), quote!());
                }
                if source.is_none() {
                    variant_ident
                        .span()
                        .unwrap()
                        .error("no source error specified for variant")
                        .emit();
                    return (quote!(), quote!());
                }
                let (field_index, field_ident) = source.unwrap();

                let field = match field_ident {
                    Some(ident) => Member::Named(ident),
                    None => Member::Unnamed(Index {
                        index: field_index as u32,
                        span: variant_ident.span(),
                    }),
                };

                let source = quote!(source);
                let module_name = quote_spanned!(variant_ident.span()=> #source.module_name());
                let code = quote_spanned!(variant_ident.span()=> #source.code());

                (
                    quote! {
                        Self::#variant_ident { #field: #source, .. } => #module_name,
                    },
                    quote! {
                        Self::#variant_ident { #field: #source, .. } => #code,
                    },
                )
            } else {
                // Regular case without forwarding.
                let code = match variant.code {
                    Some(code) => {
                        if reserved_numbers.contains(&code) {
                            variant_ident
                                .span()
                                .unwrap()
                                .error(format!("code {} already used", code))
                                .emit();
                            return (quote!(), quote!());
                        }
                        reserved_numbers.insert(code);
                        code
                    }
                    None if autonumber => {
                        let mut reserved_successors = reserved_numbers.range(next_autonumber..);
                        while reserved_successors.next() == Some(&next_autonumber) {
                            next_autonumber += 1;
                        }
                        let code = next_autonumber;
                        reserved_numbers.insert(code);
                        next_autonumber += 1;
                        code
                    }
                    None => {
                        variant_ident
                            .span()
                            .unwrap()
                            .error("missing `code` for variant")
                            .emit();
                        return (quote!(), quote!());
                    }
                };

                (
                    quote! {
                        Self::#variant_ident { .. } => #module_name,
                    },
                    quote! {
                        Self::#variant_ident { .. } => #code,
                    },
                )
            }
        })
        .unzip();

    (
        quote! {
            match #enum_binding {
                #(#module_name_matches)*
            }
        },
        quote! {
            match #enum_binding {
                #(#code_matches)*
            }
        },
        abort_variant,
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn generate_error_impl_auto_abort() {
        let expected: syn::Stmt = syn::parse_quote!(
            const _: () = {
                use oasis_runtime_sdk::{self as __sdk, error::Error as _};
                #[automatically_derived]
                impl __sdk::error::Error for Error {
                    fn module_name(&self) -> &str {
                        match self {
                            Self::Error0 { .. } => MODULE_NAME,
                            Self::Error2 { .. } => MODULE_NAME,
                            Self::Error1 { .. } => MODULE_NAME,
                            Self::Error3 { .. } => MODULE_NAME,
                            Self::ErrorAbort { .. } => MODULE_NAME,
                        }
                    }
                    fn code(&self) -> u32 {
                        match self {
                            Self::Error0 { .. } => 0u32,
                            Self::Error2 { .. } => 2u32,
                            Self::Error1 { .. } => 1u32,
                            Self::Error3 { .. } => 3u32,
                            Self::ErrorAbort { .. } => 4u32,
                        }
                    }
                    fn into_abort(self) -> Result<__sdk::dispatcher::Error, Self> {
                        match self {
                            Self::ErrorAbort(err) => Ok(err),
                            _ => Err(self),
                        }
                    }
                }
                #[automatically_derived]
                impl From<Error> for __sdk::error::RuntimeError {
                    fn from(err: Error) -> Self {
                        Self::new(err.module_name(), err.code(), &err.to_string())
                    }
                }
            };
        );

        let input: syn::DeriveInput = syn::parse_quote!(
            #[derive(Error)]
            #[sdk_error(autonumber)]
            pub enum Error {
                Error0,
                #[sdk_error(code = 2)]
                Error2 {
                    payload: Vec<u8>,
                },
                Error1(String),
                Error3,
                #[sdk_error(abort)]
                ErrorAbort(sdk::dispatcher::Error),
            }
        );
        let error_derivation = super::derive_error(input);
        let actual: syn::Stmt = syn::parse2(error_derivation).unwrap();

        crate::assert_empty_diff!(actual, expected);
    }

    #[test]
    fn generate_error_impl_manual() {
        let expected: syn::Stmt = syn::parse_quote!(
            const _: () = {
                use oasis_runtime_sdk::{self as __sdk, error::Error as _};
                #[automatically_derived]
                impl __sdk::error::Error for Error {
                    fn module_name(&self) -> &str {
                        THE_MODULE_NAME
                    }
                    fn code(&self) -> u32 {
                        0
                    }
                    fn into_abort(self) -> Result<__sdk::dispatcher::Error, Self> {
                        Err(self)
                    }
                }
                #[automatically_derived]
                impl From<Error> for __sdk::error::RuntimeError {
                    fn from(err: Error) -> Self {
                        Self::new(err.module_name(), err.code(), &err.to_string())
                    }
                }
            };
        );

        let input: syn::DeriveInput = syn::parse_quote!(
            #[derive(Error)]
            #[sdk_error(autonumber, module_name = "THE_MODULE_NAME")]
            pub enum Error {}
        );
        let error_derivation = super::derive_error(input);
        let actual: syn::Stmt = syn::parse2(error_derivation).unwrap();

        crate::assert_empty_diff!(actual, expected);
    }

    #[test]
    fn generate_error_impl_from() {
        let expected: syn::Stmt = syn::parse_quote!(
            const _: () = {
                use oasis_runtime_sdk::{self as __sdk, error::Error as _};
                #[automatically_derived]
                impl __sdk::error::Error for Error {
                    fn module_name(&self) -> &str {
                        match self {
                            Self::Foo { 0: source, .. } => source.module_name(),
                        }
                    }
                    fn code(&self) -> u32 {
                        match self {
                            Self::Foo { 0: source, .. } => source.code(),
                        }
                    }
                    fn into_abort(self) -> Result<__sdk::dispatcher::Error, Self> {
                        Err(self)
                    }
                }
                #[automatically_derived]
                impl From<Error> for __sdk::error::RuntimeError {
                    fn from(err: Error) -> Self {
                        Self::new(err.module_name(), err.code(), &err.to_string())
                    }
                }
            };
        );

        let input: syn::DeriveInput = syn::parse_quote!(
            #[derive(Error)]
            #[sdk_error(module_name = "THE_MODULE_NAME")]
            pub enum Error {
                #[sdk_error(transparent)]
                Foo(#[from] AnotherError),
            }
        );
        let error_derivation = super::derive_error(input);
        let actual: syn::Stmt = syn::parse2(error_derivation).unwrap();

        crate::assert_empty_diff!(actual, expected);
    }
}
