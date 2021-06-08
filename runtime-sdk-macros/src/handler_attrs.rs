use std::{
    collections::BTreeSet,
    lazy::SyncLazy as Lazy,
    path::PathBuf,
    sync::{Mutex, RwLock},
};

use inflector::Inflector as _;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use crate::generators as gen;

type GenerationRecord = Lazy<Mutex<BTreeSet<PathBuf>>>;
/// This tracks whether a client has already been generated by the `calls` or `queries` macro.
static GENERATED_CLIENT_IN_MODS: GenerationRecord = Lazy::new(Default::default);

/// User-defined `Copy` types. Provided to make the generated client more ergonomic.
static COPY_TYPES: Lazy<RwLock<BTreeSet<String>>> = Lazy::new(|| {
    let mut copy_types = BTreeSet::new();
    copy_types.insert("u8".into());
    copy_types.insert("i8".into());
    copy_types.insert("u16".into());
    copy_types.insert("i16".into());
    copy_types.insert("u32".into());
    copy_types.insert("i32".into());
    copy_types.insert("u64".into());
    copy_types.insert("i64".into());
    copy_types.insert("u128".into());
    copy_types.insert("i128".into());
    copy_types.insert("f32".into());
    copy_types.insert("f64".into());
    RwLock::new(copy_types)
});

pub fn register_copy_types(copy_types: &syn::punctuated::Punctuated<syn::Ident, syn::Token![,]>) {
    COPY_TYPES
        .write()
        .unwrap()
        .extend(copy_types.iter().map(|ident| ident.to_string()))
}

pub fn gen_call_items(methods: &syn::ItemTrait, args: &syn::AttributeArgs) -> TokenStream {
    gen_handler_items(methods, args, Handlers::Calls)
}

pub fn gen_query_items(methods: &syn::ItemTrait, args: &syn::AttributeArgs) -> TokenStream {
    gen_handler_items(methods, args, Handlers::Queries)
}

fn gen_handler_items(
    handlers: &syn::ItemTrait,
    args: &syn::AttributeArgs,
    handlers_kind: Handlers,
) -> TokenStream {
    let runtime_module_name = match gen::module_name(match find_meta_key(args, "module_name") {
        Some(syn::MetaNameValue {
            lit: syn::Lit::Str(m),
            ..
        }) => Some(&m),
        None => None,
        _ => {
            proc_macro2::Span::call_site()
                .unwrap()
                .error("expected `module_name` to be a valid path")
                .emit();
            return quote!();
        }
    }) {
        Ok(expr) => expr,
        Err(_) => return quote!(),
    };

    let handler_methods = match unpack_handler_methods(handlers, handlers_kind) {
        Ok(methods) => methods,
        Err(_) => return quote!(),
    };

    let current_module = handlers.trait_token.span.unwrap().source_file().path();

    let module_items = gen_module_items(
        handlers,
        &handler_methods,
        handlers_kind,
        &runtime_module_name,
    );

    let client_items = gen_client_items(
        &handler_methods,
        handlers_kind,
        &runtime_module_name,
        current_module,
    );

    let output = quote! {
        #(#[cfg(feature = "runtime-module")] #module_items)*
        #(#[cfg(feature = "runtime-client")] #client_items)*
    };

    output
}

fn gen_module_items(
    handlers: &syn::ItemTrait,
    handler_methods: &[HandlerMethod<'_>],
    handlers_kind: Handlers,
    runtime_module_name_path: &syn::Expr,
) -> Vec<TokenStream> {
    let sdk_crate = gen::sdk_crate_path();

    let trait_ident = &handlers.ident;
    let trait_generics = &handlers.generics;
    let supertraits = &handlers.supertraits;

    let handler_ctx_ty = handlers_kind.context_ty();

    let module_handlers = handler_methods.iter().map(|HandlerMethod { method, .. }| {
        let handler_ident = &method.sig.ident;
        let attrs = &method.attrs;
        let inputs = &method.sig.inputs;
        let generics = &method.sig.generics.params;
        let output_ty = &method.sig.output;
        quote! {
            #(#attrs)*
            fn #handler_ident<#generics>(
                ctx: &mut impl #sdk_crate::context::#handler_ctx_ty,
                #inputs
            ) #output_ty;
        }
    });

    let handler_fn_ident = format_ident!("handle_{}", handlers_kind.to_string().to_singular());
    let handler_err_ty = match handlers_kind {
        Handlers::Calls => quote!(#sdk_crate::types::transaction::CallResult),
        Handlers::Queries => {
            quote!(Result<#sdk_crate::core::common::cbor::Value, #sdk_crate::error::RuntimeError>)
        }
    };
    let dispatch_arms = handler_methods.iter().map(|m| {
        let result_ident = format_ident!("result");

        let handler_ident = &m.ident;
        let rpc_method_name = &m.rpc_name;

        let cfg_attrs = &m.cfg_attrs;

        let arg_idents: Vec<_> = m.args.iter().map(|arg| &arg.binding).collect();
        let arg_tys = m.args.iter().map(|arg| &arg.ty);

        let result_encoder = match handlers_kind {
            Handlers::Calls => quote! {
                match #result_ident {
                    Ok(value) => #sdk_crate::types::transaction::CallResult::Ok(value),
                    Err(e) => #sdk_crate::error::Error::to_call_result(&e),
                }
            },
            Handlers::Queries => quote!(#result_ident.map_err(Into::into)),
        };

        let serde_transparent = (arg_idents.len() == 1).then(|| quote!(#[serde(transparent)]));

        quote! {
            #(#cfg_attrs)*
            Some(#rpc_method_name) => {
                use #sdk_crate::core::common::cbor;
                #[derive(serde::Deserialize)]
                #serde_transparent
                struct QueryArgs {
                    #(#arg_idents: #arg_tys),*
                }
                let #result_ident = cbor::from_value(args)
                    .map_err(Into::into)
                    .and_then(|QueryArgs { #(#arg_idents),* }| {
                        Self::#handler_ident(ctx, #(#arg_idents),*)
                    })
                    .map(|result| cbor::to_value(&result));
                #sdk_crate::module::DispatchResult::Handled(#result_encoder)
            }
        }
    });

    let module_trait = quote! {
        pub trait #trait_ident #trait_generics : #supertraits {
            #(#module_handlers)*

            #[allow(warnings)]
            fn #handler_fn_ident<C: #sdk_crate::context::#handler_ctx_ty>(
                ctx: &mut C,
                method: &str,
                args: #sdk_crate::core::common::cbor::Value,
            ) -> #sdk_crate::module::DispatchResult<
                #sdk_crate::core::common::cbor::Value,
                #handler_err_ty,
            > {
                let mut method_parts = method.splitn(1, '.');
                if method_parts.next().map(|p| p == #runtime_module_name_path).unwrap_or_default() {
                    return #sdk_crate::module::DispatchResult::Unhandled(args);
                }
                match method_parts.next() {
                    #(#dispatch_arms)*
                    _ => #sdk_crate::module::DispatchResult::Unhandled(args),
                }
            }
        }
    };

    vec![module_trait]
}

fn gen_client_items(
    handler_methods: &[HandlerMethod<'_>],
    handlers_kind: Handlers,
    runtime_module_name_path: &syn::Expr,
    current_module: PathBuf,
) -> Vec<TokenStream> {
    let sdk_crate = gen::sdk_crate_path();

    let mut client_items: Vec<TokenStream> = generate_once(
        &GENERATED_CLIENT_IN_MODS,
        current_module,
        gen_client_struct_and_ctor,
    )
    .unwrap_or_default();

    let rpc_signatures: Vec<_> = handler_methods
        .iter()
        .map(|m| {
            let cfg_attrs = &m.cfg_attrs;

            let method_ident = &m.client_method_ident;

            let arg_idents: Vec<_> = m.args.iter().map(|arg| &arg.binding).collect();
            let args_lifetime = syn::Lifetime::new("'_", proc_macro2::Span::call_site());
            let arg_tys: Vec<_> = m
                .args
                .iter()
                .map(|arg| to_borrowed(arg.ty, &args_lifetime).1)
                .collect();

            let res_ty = match &m.method.sig.output {
                syn::ReturnType::Default => quote!(()),
                syn::ReturnType::Type(_, box syn::Type::Path(syn::TypePath { path, .. }))
                    if path.segments.last().unwrap().ident == "Result" =>
                {
                    let ok_ty = extract_generic_ty(&path.segments.last().unwrap().arguments);
                    quote!(#ok_ty)
                }
                syn::ReturnType::Type(_, ty) => quote!(#ty),
            };

            quote! {
                #(#cfg_attrs)*
                async fn #method_ident(
                    &mut self,
                    #(#arg_idents: #arg_tys),*
                ) -> Result<#res_ty, oasis_client_sdk::Error>
            }
        })
        .collect();

    let rpcs = handler_methods
        .iter()
        .zip(rpc_signatures.iter())
        .map(|(m, sig)| {
            let rpc_method_name = &m.rpc_name;

            let arg_idents: Vec<_> = m.args.iter().map(|arg| &arg.binding).collect();
            let args_lifetime = syn::Lifetime::new("'a", proc_macro2::Span::call_site());
            let mut arg_tys = Vec::with_capacity(m.args.len());
            let mut any_is_borrowed = false;
            for arg in m.args.iter() {
                let (is_borrowed, arg_ty) = to_borrowed(arg.ty, &args_lifetime);
                arg_tys.push(arg_ty);
                any_is_borrowed |= is_borrowed;
            }
            let struct_lifetime = any_is_borrowed.then(|| args_lifetime);

            let serde_transparent = (arg_idents.len() == 1).then(|| quote!(#[serde(transparent)]));

            quote! {
                #sig {
                    use #sdk_crate::{
                        types::transaction::CallResult as _CallResult,
                        core::common::cbor as _cbor,
                    };
                    #[derive(serde::Serialize)]
                    #serde_transparent
                    struct CallArgs<#struct_lifetime> {
                        #(#arg_idents: #arg_tys),*
                    }
                    let serialized_call_result = self.inner.tx(
                        &format!("{}.{}", #runtime_module_name_path, #rpc_method_name),
                        &_cbor::to_value(&CallArgs {
                            #(#arg_idents),*
                        }),
                    ).await?;
                    match _cbor::from_slice::<_CallResult>(&serialized_call_result)? {
                        _CallResult::Ok(res) => _cbor::from_value(res).map_err(Into::into),
                        _CallResult::Failed {
                            module, code, message
                        } => {
                            let message = if message.is_empty() { None } else { Some(message) };
                            Err(oasis_client_sdk::Error::TxReverted { module, code, message })
                        }
                    }
                }
            }
        });

    let trait_ident = format_ident!("Client{}", handlers_kind.to_string().to_pascal_case());

    client_items.push(quote! {
        #[oasis_client_sdk::async_trait]
        pub trait #trait_ident {
            #(#rpc_signatures;)*
        }
    });
    client_items.push(quote! {
        #[oasis_client_sdk::async_trait]
        impl<S: oasis_client_sdk::signer::Signer + Send + Sync> #trait_ident for RuntimeClient<S> {
            #(#rpcs)*
        }
    });

    client_items
}

fn generate_once<T, F>(record: &GenerationRecord, current_mod: PathBuf, generator: F) -> Option<T>
where
    F: FnOnce() -> T,
{
    let mut record = record.lock().unwrap();
    if record.contains(&current_mod) {
        return None;
    }
    record.insert(current_mod);
    Some(generator())
}

fn gen_client_struct_and_ctor() -> Vec<TokenStream> {
    let sdk_crate = gen::sdk_crate_path();

    let client_struct = quote! {
        #[derive(Clone)]
        pub struct RuntimeClient<S: oasis_client_sdk::signer::Signer + Send + Sync> {
            inner: oasis_client_sdk::Client<S>
        }
    };

    let client_impl = gen::wrap_in_const(quote! {
        use #sdk_crate::core::common::namespace::Namespace;

        impl<S: oasis_client_sdk::signer::Signer + Send + Sync> RuntimeClient<S> {
            /// Connects to the oasis-node listening on Unix socket at `sock_path` communicating
            /// with the identified runtime. Transactions will be signed by the `signer`.
            /// Do remember to call `set_fee` as appropriate before making the first call.
            pub async fn connect(
                sock_path: impl AsRef<std::path::Path> + Clone + Send + Sync + 'static,
                runtime_id: Namespace,
                signer: S,
            ) -> Result<Self, oasis_client_sdk::Error> {
                Ok(Self {
                    inner: oasis_client_sdk::Client::connect(sock_path, runtime_id, signer).await?
                })
            }

            /// Sets the new fee provided with each transaction.
            pub fn set_fee(&mut self, fee: #sdk_crate::types::transaction::Fee) {
                self.inner.set_fee(fee);
            }
        }
    });

    vec![client_struct, client_impl]
}

/// Returns the parsed attribute with path ending with `name` or `None` if not found.
fn find_attr(attrs: &[syn::Attribute], attr_path: &[&str]) -> Option<syn::Meta> {
    attrs.iter().find_map(|attr| {
        if attr
            .path
            .segments
            .iter()
            .rev()
            .zip(attr_path.iter().rev())
            .all(|(seg, expected)| seg.ident == expected)
        {
            attr.parse_meta().ok()
        } else {
            None
        }
    })
}

/// Returns the `MetaNameValue` identified by `key` or `None` if not found.
fn find_meta_key<'a>(
    metas: impl IntoIterator<Item = &'a syn::NestedMeta>,
    key: &str,
) -> Option<&'a syn::MetaNameValue> {
    metas.into_iter().find_map(|meta| match meta {
        syn::NestedMeta::Meta(syn::Meta::NameValue(meta)) if meta.path.is_ident(key) => Some(meta),
        _ => None,
    })
}

/// Returns `(has_been_borrowed, ty)`
fn to_borrowed(ty: &syn::Type, lifetime: &syn::Lifetime) -> (bool, TokenStream) {
    match ty {
        syn::Type::Reference(syn::TypeReference { elem, .. }) => (true, quote!(&#lifetime #elem)),
        syn::Type::Array(syn::TypeArray { elem, len, .. }) => {
            (true, quote!(&#lifetime [#elem; #len]))
        }
        syn::Type::Group(syn::TypeGroup { elem, .. }) => to_borrowed(elem, lifetime),
        syn::Type::Paren(syn::TypeParen { elem, .. }) => to_borrowed(elem, lifetime),
        syn::Type::ImplTrait(t) => (true, quote!(&#lifetime #t)),
        syn::Type::Tuple(t) => (true, quote!(&#lifetime #t)),
        syn::Type::TraitObject(t) => (true, quote!(&#lifetime #t)),
        syn::Type::Path(syn::TypePath { path, .. }) => {
            let last_segment = path.segments.last().unwrap();
            if last_segment.ident == "Box"
                || last_segment.ident == "Arc"
                || last_segment.ident == "Rc"
                || last_segment.ident == "RefCell"
                || last_segment.ident == "Mutex"
                || last_segment.ident == "RwLock"
            {
                let elem_ty = extract_generic_ty(&last_segment.arguments);
                to_borrowed(elem_ty, lifetime)
            } else if last_segment.ident == "Cell" {
                let elem_ty = extract_generic_ty(&last_segment.arguments);
                (false, quote!(#elem_ty))
            } else if last_segment.ident == "Vec" {
                let elem_ty = extract_generic_ty(&last_segment.arguments);
                (true, quote!(&#lifetime [#elem_ty]))
            } else if last_segment.ident == "String" {
                (true, quote!(&#lifetime str))
            } else if last_segment.ident == "PathBuf" {
                (true, quote!(&#lifetime Path))
            } else if is_copy_ty(&last_segment.ident) {
                (false, quote!(#ty))
            } else {
                (true, quote!(&#lifetime #path))
            }
        }
        _ => (false, quote!(#ty)),
    }
}

fn is_copy_ty(ty_ident: &syn::Ident) -> bool {
    let copy_types = COPY_TYPES.read().unwrap();
    copy_types.iter().any(|ty_str| ty_ident == ty_str)
}

fn extract_generic_ty(args: &syn::PathArguments) -> &syn::Type {
    let generics = match args {
        syn::PathArguments::AngleBracketed(ab) => &ab.args,
        _ => panic!("expected generics"),
    };
    match generics.first().unwrap() {
        syn::GenericArgument::Type(ty) => ty,
        _ => panic!("expected a generic type"),
    }
}

struct HandlerMethod<'a> {
    method: &'a syn::TraitItemMethod,
    ident: &'a syn::Ident,
    client_method_ident: syn::Ident,
    args: Vec<MethodArg<'a>>,
    rpc_name: String,
    cfg_attrs: Vec<&'a syn::Attribute>,
}

fn unpack_handler_methods(
    handlers: &syn::ItemTrait,
    handlers_kind: Handlers,
) -> Result<Vec<HandlerMethod<'_>>, ()> {
    let mut handler_methods = Vec::with_capacity(handlers.items.len());
    for item in handlers.items.iter() {
        let method = match item {
            syn::TraitItem::Method(m) => m,
            _ => continue,
        };

        let handlers_attr = &["sdk", &handlers_kind.to_string().to_singular()];

        let handler_metas = match find_attr(&method.attrs, handlers_attr) {
            Some(syn::Meta::List(metas)) => metas.nested,
            _ => Default::default(),
        };

        let client_method_ident = find_meta_key(&handler_metas, "client_method")
            .map(|meta| match &meta.lit {
                syn::Lit::Str(name) => name.parse().map_err(|_| {
                    name.span()
                        .unwrap()
                        .error("expected a valid identifier")
                        .emit();
                }),
                _ => {
                    meta.lit
                        .span()
                        .unwrap()
                        .error("expected a literal string containing valid identifier")
                        .emit();
                    Err(())
                }
            })
            .transpose()?
            .unwrap_or_else(|| method.sig.ident.clone());

        let rpc_name = find_meta_key(&handler_metas, "name")
            .map(|meta| match &meta.lit {
                syn::Lit::Str(name) => Ok(name.value()),
                _ => {
                    meta.lit
                        .span()
                        .unwrap()
                        .error("expected a literal string containing valid identifier")
                        .emit();
                    Err(())
                }
            })
            .transpose()?
            .unwrap_or_else(|| method.sig.ident.to_string().to_pascal_case());

        let cfg_attrs = method
            .attrs
            .iter()
            .filter(|attr| attr.path.is_ident("cfg") || attr.path.is_ident("cfg_attr"))
            .collect();

        handler_methods.push(HandlerMethod {
            method,
            ident: &method.sig.ident,
            client_method_ident,
            args: unpack_method_args(&method.sig)?,
            rpc_name,
            cfg_attrs,
        })
    }
    Ok(handler_methods)
}

struct MethodArg<'a> {
    binding: &'a syn::Ident,
    ty: &'a syn::Type,
}

fn unpack_method_args(sig: &syn::Signature) -> Result<Vec<MethodArg<'_>>, ()> {
    if let Some(syn::FnArg::Receiver(receiver)) = sig.receiver() {
        receiver
            .self_token
            .span
            .unwrap()
            .error("must not have a `self` argument")
            .emit();
        return Err(());
    };
    let mut args = Vec::new();
    for inp in sig.inputs.iter() {
        match inp {
            syn::FnArg::Typed(syn::PatType {
                pat: box syn::Pat::Ident(syn::PatIdent { ident, .. }),
                box ty,
                ..
            }) => {
                args.push(MethodArg { binding: ident, ty });
            }
            syn::FnArg::Receiver(_) => unreachable!("checked above"),
            syn::FnArg::Typed(ty) => {
                ty.colon_token.spans[0]
                    .unwrap()
                    .error("all arguments must be named")
                    .emit();
                return Err(());
            }
        }
    }
    Ok(args)
}

#[derive(Clone, Copy)]
enum Handlers {
    Calls,
    Queries,
}

impl Handlers {
    fn context_ty(&self) -> TokenStream {
        match self {
            Self::Calls => quote!(TxContext),
            Self::Queries => quote!(Context),
        }
    }
}

impl std::fmt::Display for Handlers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Calls => "calls",
            Self::Queries => "queries",
        })
    }
}
