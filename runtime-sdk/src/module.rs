//! Runtime modules.
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Debug,
};

use impl_trait_for_tuples::impl_for_tuples;

use crate::{
    context::{Context, TxContext},
    dispatcher, error,
    error::Error as _,
    event, modules, storage,
    storage::{Prefix, Store},
    types::{
        message::MessageResult,
        transaction::{
            self, AuthInfo, Call, Transaction, TransactionWeight, UnverifiedTransaction,
        },
    },
};

/// Result of invoking the method handler.
pub enum DispatchResult<B, R> {
    Handled(R),
    Unhandled(B),
}

impl<B, R> DispatchResult<B, R> {
    /// Transforms `DispatchResult<B, R>` into `Result<R, E>`, mapping `Handled(r)` to `Ok(r)` and
    /// `Unhandled(_)` to `Err(err)`.
    pub fn ok_or<E>(self, err: E) -> Result<R, E> {
        match self {
            DispatchResult::Handled(result) => Ok(result),
            DispatchResult::Unhandled(_) => Err(err),
        }
    }

    /// Transforms `DispatchResult<B, R>` into `Result<R, E>`, mapping `Handled(r)` to `Ok(r)` and
    /// `Unhandled(_)` to `Err(err)` using the provided function.
    pub fn ok_or_else<E, F: FnOnce() -> E>(self, errf: F) -> Result<R, E> {
        match self {
            DispatchResult::Handled(result) => Ok(result),
            DispatchResult::Unhandled(_) => Err(errf()),
        }
    }
}

/// A variant of `types::transaction::CallResult` but used for dispatch purposes so the dispatch
/// process can use a different representation.
///
/// Specifically, this type is not serializable.
#[derive(Debug)]
pub enum CallResult {
    /// Call has completed successfully.
    Ok(cbor::Value),

    /// Call has completed with failure.
    Failed {
        module: String,
        code: u32,
        message: String,
    },

    /// A fatal error has occurred and the batch must be aborted.
    Aborted(dispatcher::Error),
}

impl CallResult {
    /// Check whether the call result indicates a successful operation or not.
    pub fn is_success(&self) -> bool {
        matches!(self, CallResult::Ok(_))
    }
}

impl From<CallResult> for transaction::CallResult {
    fn from(v: CallResult) -> Self {
        match v {
            CallResult::Ok(data) => Self::Ok(data),
            CallResult::Failed {
                module,
                code,
                message,
            } => Self::Failed {
                module,
                code,
                message,
            },
            CallResult::Aborted(err) => Self::Failed {
                module: err.module_name().to_string(),
                code: err.code(),
                message: err.to_string(),
            },
        }
    }
}

/// A convenience function for dispatching method calls.
pub fn dispatch_call<C, B, R, E, F>(
    ctx: &mut C,
    body: cbor::Value,
    f: F,
) -> DispatchResult<cbor::Value, CallResult>
where
    C: TxContext,
    B: cbor::Decode,
    R: cbor::Encode,
    E: error::Error,
    F: FnOnce(&mut C, B) -> Result<R, E>,
{
    DispatchResult::Handled((|| {
        let args = match cbor::from_value(body)
            .map_err(|err| modules::core::Error::InvalidArgument(err.into()))
        {
            Ok(args) => args,
            Err(err) => return err.into_call_result(),
        };

        match f(ctx, args) {
            Ok(value) => CallResult::Ok(cbor::to_value(value)),
            Err(err) => err.into_call_result(),
        }
    })())
}

/// A convenience function for dispatching queries.
pub fn dispatch_query<C, B, R, E, F>(
    ctx: &mut C,
    body: cbor::Value,
    f: F,
) -> DispatchResult<cbor::Value, Result<cbor::Value, error::RuntimeError>>
where
    C: Context,
    B: cbor::Decode,
    R: cbor::Encode,
    E: error::Error,
    error::RuntimeError: From<E>,
    F: FnOnce(&mut C, B) -> Result<R, E>,
{
    DispatchResult::Handled((|| {
        let args = cbor::from_value(body).map_err(|err| -> error::RuntimeError {
            modules::core::Error::InvalidArgument(err.into()).into()
        })?;
        Ok(cbor::to_value(f(ctx, args)?))
    })())
}

/// Method handler.
pub trait MethodHandler {
    /// Add storage prefixes to prefetch.
    fn prefetch(
        _prefixes: &mut BTreeSet<Prefix>,
        _method: &str,
        body: cbor::Value,
        _auth_info: &AuthInfo,
    ) -> DispatchResult<cbor::Value, Result<(), error::RuntimeError>> {
        // Default implementation indicates that the call was not handled.
        DispatchResult::Unhandled(body)
    }

    /// Dispatch a call.
    fn dispatch_call<C: TxContext>(
        _ctx: &mut C,
        _method: &str,
        body: cbor::Value,
    ) -> DispatchResult<cbor::Value, CallResult> {
        // Default implementation indicates that the call was not handled.
        DispatchResult::Unhandled(body)
    }

    /// Dispatch a query.
    fn dispatch_query<C: Context>(
        _ctx: &mut C,
        _method: &str,
        args: cbor::Value,
    ) -> DispatchResult<cbor::Value, Result<cbor::Value, error::RuntimeError>> {
        // Default implementation indicates that the query was not handled.
        DispatchResult::Unhandled(args)
    }

    /// Dispatch a message result.
    fn dispatch_message_result<C: Context>(
        _ctx: &mut C,
        _handler_name: &str,
        result: MessageResult,
    ) -> DispatchResult<MessageResult, ()> {
        // Default implementation indicates that the query was not handled.
        DispatchResult::Unhandled(result)
    }
}

#[impl_for_tuples(30)]
impl MethodHandler for Tuple {
    fn prefetch(
        prefixes: &mut BTreeSet<Prefix>,
        method: &str,
        body: cbor::Value,
        auth_info: &AuthInfo,
    ) -> DispatchResult<cbor::Value, Result<(), error::RuntimeError>> {
        // Return on first handler that can handle the method.
        for_tuples!( #(
            let body = match Tuple::prefetch(prefixes, method, body, auth_info) {
                DispatchResult::Handled(result) => return DispatchResult::Handled(result),
                DispatchResult::Unhandled(body) => body,
            };
        )* );

        DispatchResult::Unhandled(body)
    }

    fn dispatch_call<C: TxContext>(
        ctx: &mut C,
        method: &str,
        body: cbor::Value,
    ) -> DispatchResult<cbor::Value, CallResult> {
        // Return on first handler that can handle the method.
        for_tuples!( #(
            let body = match Tuple::dispatch_call::<C>(ctx, method, body) {
                DispatchResult::Handled(result) => return DispatchResult::Handled(result),
                DispatchResult::Unhandled(body) => body,
            };
        )* );

        DispatchResult::Unhandled(body)
    }

    fn dispatch_query<C: Context>(
        ctx: &mut C,
        method: &str,
        args: cbor::Value,
    ) -> DispatchResult<cbor::Value, Result<cbor::Value, error::RuntimeError>> {
        // Return on first handler that can handle the method.
        for_tuples!( #(
            let args = match Tuple::dispatch_query::<C>(ctx, method, args) {
                DispatchResult::Handled(result) => return DispatchResult::Handled(result),
                DispatchResult::Unhandled(args) => args,
            };
        )* );

        DispatchResult::Unhandled(args)
    }

    fn dispatch_message_result<C: Context>(
        ctx: &mut C,
        handler_name: &str,
        result: MessageResult,
    ) -> DispatchResult<MessageResult, ()> {
        // Return on first handler that can handle the method.
        for_tuples!( #(
            let result = match Tuple::dispatch_message_result::<C>(ctx, handler_name, result) {
                DispatchResult::Handled(result) => return DispatchResult::Handled(result),
                DispatchResult::Unhandled(result) => result,
            };
        )* );

        DispatchResult::Unhandled(result)
    }
}

/// Authentication handler.
pub trait AuthHandler {
    /// Judge if an unverified transaction is good enough to undergo verification.
    /// This takes place before even verifying signatures.
    fn approve_unverified_tx<C: Context>(
        _ctx: &mut C,
        _utx: &UnverifiedTransaction,
    ) -> Result<(), modules::core::Error> {
        // Default implementation doesn't do any checks.
        Ok(())
    }

    /// Decode a transaction that was sent with module-controlled decoding and verify any
    /// signatures.
    ///
    /// Postcondition: if returning a Transaction, that transaction must pass `validate_basic`.
    ///
    /// Returns Ok(Some(_)) if the module is in charge of the encoding scheme identified by _scheme
    /// or Ok(None) otherwise.
    fn decode_tx<C: Context>(
        _ctx: &mut C,
        _scheme: &str,
        _body: &[u8],
    ) -> Result<Option<Transaction>, modules::core::Error> {
        // Default implementation is not in charge of any schemes.
        Ok(None)
    }

    /// Authenticate a transaction.
    ///
    /// Note that any signatures have already been verified.
    fn authenticate_tx<C: Context>(
        _ctx: &mut C,
        _tx: &Transaction,
    ) -> Result<(), modules::core::Error> {
        // Default implementation accepts all transactions.
        Ok(())
    }

    /// Perform any action after authentication, within the transaction context.
    fn before_handle_call<C: TxContext>(
        _ctx: &mut C,
        _call: &Call,
    ) -> Result<(), modules::core::Error> {
        // Default implementation doesn't do anything.
        Ok(())
    }
}

#[impl_for_tuples(30)]
impl AuthHandler for Tuple {
    fn approve_unverified_tx<C: Context>(
        ctx: &mut C,
        utx: &UnverifiedTransaction,
    ) -> Result<(), modules::core::Error> {
        for_tuples!( #( Tuple::approve_unverified_tx(ctx, utx)?; )* );
        Ok(())
    }

    fn decode_tx<C: Context>(
        ctx: &mut C,
        scheme: &str,
        body: &[u8],
    ) -> Result<Option<Transaction>, modules::core::Error> {
        for_tuples!( #(
            let decoded = Tuple::decode_tx(ctx, scheme, body)?;
            if (decoded.is_some()) {
                return Ok(decoded);
            }
        )* );
        Ok(None)
    }

    fn authenticate_tx<C: Context>(
        ctx: &mut C,
        tx: &Transaction,
    ) -> Result<(), modules::core::Error> {
        for_tuples!( #( Tuple::authenticate_tx(ctx, tx)?; )* );
        Ok(())
    }

    fn before_handle_call<C: TxContext>(
        ctx: &mut C,
        call: &Call,
    ) -> Result<(), modules::core::Error> {
        for_tuples!( #( Tuple::before_handle_call(ctx, call)?; )* );
        Ok(())
    }
}

/// Migration handler.
pub trait MigrationHandler {
    /// Genesis state type.
    ///
    /// If this state is expensive to compute and not often updated, prefer
    /// to make the genesis type something like `once_cell::unsync::Lazy<T>`.
    type Genesis;

    /// Initialize state from genesis or perform a migration.
    ///
    /// Should return true in case metadata has been changed.
    fn init_or_migrate<C: Context>(
        _ctx: &mut C,
        _meta: &mut modules::core::types::Metadata,
        _genesis: Self::Genesis,
    ) -> bool {
        // Default implementation doesn't perform any migrations.
        false
    }
}

#[allow(clippy::type_complexity)]
#[impl_for_tuples(30)]
impl MigrationHandler for Tuple {
    for_tuples!( type Genesis = ( #( Tuple::Genesis ),* ); );

    fn init_or_migrate<C: Context>(
        ctx: &mut C,
        meta: &mut modules::core::types::Metadata,
        genesis: Self::Genesis,
    ) -> bool {
        [for_tuples!( #( Tuple::init_or_migrate(ctx, meta, genesis.Tuple) ),* )]
            .iter()
            .any(|x| *x)
    }
}

/// Block handler.
pub trait BlockHandler {
    /// Perform any common actions at the start of the block (before any transactions have been
    /// executed).
    fn begin_block<C: Context>(_ctx: &mut C) {
        // Default implementation doesn't do anything.
    }

    /// Perform any common actions at the end of the block (after all transactions have been
    /// executed).
    fn end_block<C: Context>(_ctx: &mut C) {
        // Default implementation doesn't do anything.
    }

    /// Returns module per-batch weight limits.
    fn get_block_weight_limits<C: Context>(_ctx: &mut C) -> BTreeMap<TransactionWeight, u64> {
        BTreeMap::new()
    }
}

#[impl_for_tuples(30)]
impl BlockHandler for Tuple {
    fn begin_block<C: Context>(ctx: &mut C) {
        for_tuples!( #( Tuple::begin_block(ctx); )* );
    }

    fn end_block<C: Context>(ctx: &mut C) {
        for_tuples!( #( Tuple::end_block(ctx); )* );
    }

    // Ignore let and return for the empty tuple case.
    #[allow(clippy::let_and_return)]
    fn get_block_weight_limits<C: Context>(ctx: &mut C) -> BTreeMap<TransactionWeight, u64> {
        let mut result = BTreeMap::new();

        for_tuples!( #(
            result.extend( Tuple::get_block_weight_limits(ctx) );
        )* );

        result
    }
}

/// Invariant handler.
pub trait InvariantHandler {
    /// Check invariants.
    fn check_invariants<C: Context>(_ctx: &mut C) -> Result<(), modules::core::Error> {
        // Default implementation doesn't do anything.
        Ok(())
    }
}

#[impl_for_tuples(30)]
impl InvariantHandler for Tuple {
    /// Check the invariants in all modules in the tuple.
    fn check_invariants<C: Context>(ctx: &mut C) -> Result<(), modules::core::Error> {
        for_tuples!( #( Tuple::check_invariants(ctx)?; )* );
        Ok(())
    }
}

/// A runtime module.
pub trait Module {
    /// Module name.
    const NAME: &'static str;

    /// Module version.
    const VERSION: u32 = 1;

    /// Module error type.
    type Error: error::Error + 'static;

    /// Module event type.
    type Event: event::Event + 'static;

    /// Module parameters.
    type Parameters: Parameters + 'static;

    /// Return the module's parameters.
    fn params<S: Store>(store: S) -> Self::Parameters {
        let store = storage::PrefixStore::new(store, &Self::NAME);
        let store = storage::TypedStore::new(store);
        store.get(Self::Parameters::STORE_KEY).unwrap_or_default()
    }

    /// Set the module's parameters.
    fn set_params<S: Store>(store: S, params: Self::Parameters) {
        let store = storage::PrefixStore::new(store, &Self::NAME);
        let mut store = storage::TypedStore::new(store);
        store.insert(Self::Parameters::STORE_KEY, params);
    }
}

/// Parameters for a runtime module.
pub trait Parameters: Debug + Default + cbor::Encode + cbor::Decode {
    type Error;

    /// Store key used for storing parameters.
    const STORE_KEY: &'static [u8] = &[0x00];

    /// Perform basic parameter validation.
    fn validate_basic(&self) -> Result<(), Self::Error> {
        // No validation by default.
        Ok(())
    }
}

impl Parameters for () {
    type Error = std::convert::Infallible;
}
