use thiserror::Error;

use anyhow::Context as _;

use oasis_runtime_sdk::{
    self as sdk,
    context::{Context, TxContext},
    core::common::crypto::hash::Hash,
    error::RuntimeError,
    keymanager::KeyPairId,
    module::{CallResult, Module as _},
    modules::{
        core,
        core::{Error as CoreError, Module as Core, API as _},
    },
    types::{address, transaction},
};

pub mod types;

/// The name of our module.
const MODULE_NAME: &str = "keyvalue";

/// The signature context used in the special greeting encoding scheme signature.
const SPECIAL_GREETING_SIGNATURE_CONTEXT: &[u8] =
    "oasis-runtime-sdk-test/simplekv-special-greeting: v0".as_bytes();

/// Errors emitted by the keyvalue module.
#[derive(Error, Debug, sdk::Error)]
pub enum Error {
    #[error("invalid argument")]
    #[sdk_error(code = 1)]
    InvalidArgument,

    #[error("core: {0}")]
    #[sdk_error(transparent)]
    Core(#[from] core::Error),

    #[error("{0}")]
    #[sdk_error(transparent, abort)]
    Abort(#[source] sdk::dispatcher::Error),
}

/// Events emitted by the keyvalue module.
#[derive(Debug, cbor::Encode, sdk::Event)]
#[cbor(untagged)]
pub enum Event {
    #[sdk_event(code = 1)]
    Insert { kv: types::KeyValue },

    #[sdk_event(code = 2)]
    Remove { key: types::Key },
}

/// Gas costs.
#[derive(Clone, Debug, Default, cbor::Encode, cbor::Decode)]
pub struct GasCosts {
    pub insert_absent: u64,
    pub insert_existing: u64,
    pub remove_absent: u64,
    pub remove_existing: u64,
}

/// Parameters for the keyvalue module.
#[derive(Clone, Debug, Default, cbor::Encode, cbor::Decode)]
pub struct Parameters {
    pub gas_costs: GasCosts,
}

impl sdk::module::Parameters for Parameters {
    type Error = ();
}

/// Genesis state for the keyvalue module.
#[derive(Clone, Debug, Default, cbor::Encode, cbor::Decode)]
pub struct Genesis {
    pub parameters: Parameters,
}

/// Simple keyvalue runtime module.
pub struct Module;

impl sdk::module::Module for Module {
    const NAME: &'static str = MODULE_NAME;
    const VERSION: u32 = 1;
    type Error = Error;
    type Event = Event;
    type Parameters = Parameters;
}

impl sdk::module::AuthHandler for Module {
    fn decode_tx<C: Context>(
        _ctx: &mut C,
        scheme: &str,
        body: &[u8],
    ) -> Result<Option<transaction::Transaction>, CoreError> {
        match scheme {
            "keyvalue.special-greeting.v0" => {
                let special_greeting: types::SpecialGreeting = cbor::from_slice(body)
                    .with_context(|| "decoding special greeting")
                    .map_err(CoreError::MalformedTransaction)?;
                special_greeting
                    .from
                    .verify(
                        SPECIAL_GREETING_SIGNATURE_CONTEXT,
                        &special_greeting.params_cbor,
                        &special_greeting.signature,
                    )
                    .with_context(|| "verifying special greeting signature")
                    .map_err(CoreError::MalformedTransaction)?;
                let params: types::SpecialGreetingParams =
                    cbor::from_slice(&special_greeting.params_cbor)
                        .with_context(|| "decoding special greeting parameters")
                        .map_err(CoreError::MalformedTransaction)?;
                Ok(Some(transaction::Transaction {
                    version: transaction::LATEST_TRANSACTION_VERSION,
                    call: transaction::Call {
                        format: transaction::CallFormat::Plain,
                        method: "keyvalue.Insert".to_string(),
                        body: cbor::to_value(types::KeyValue {
                            key: "greeting".as_bytes().to_owned(),
                            value: params.greeting.into_bytes(),
                        }),
                    },
                    auth_info: transaction::AuthInfo {
                        signer_info: vec![transaction::SignerInfo {
                            address_spec: transaction::AddressSpec::Signature(
                                address::SignatureAddressSpec::Ed25519(special_greeting.from),
                            ),
                            nonce: params.nonce,
                        }],
                        fee: transaction::Fee {
                            gas: 500,
                            ..Default::default()
                        },
                    },
                }))
                // After we decode this, the accounts module will check the nonce.
            }
            _ => Ok(None),
        }
    }
}

impl sdk::module::BlockHandler for Module {}
impl sdk::module::InvariantHandler for Module {}

impl sdk::module::MethodHandler for Module {
    fn dispatch_call<C: TxContext>(
        ctx: &mut C,
        method: &str,
        body: cbor::Value,
    ) -> sdk::module::DispatchResult<cbor::Value, CallResult> {
        match method {
            "keyvalue.Insert" => sdk::module::dispatch_call(ctx, body, Self::tx_insert),
            "keyvalue.Remove" => sdk::module::dispatch_call(ctx, body, Self::tx_remove),
            "keyvalue.GetCreateKey" => sdk::module::dispatch_call(ctx, body, Self::tx_getcreatekey),
            _ => sdk::module::DispatchResult::Unhandled(body),
        }
    }

    fn dispatch_query<C: Context>(
        ctx: &mut C,
        method: &str,
        args: cbor::Value,
    ) -> sdk::module::DispatchResult<cbor::Value, Result<cbor::Value, RuntimeError>> {
        match method {
            "keyvalue.Get" => sdk::module::dispatch_query(ctx, args, Self::query_get),
            _ => sdk::module::DispatchResult::Unhandled(args),
        }
    }
}

// Actual implementation of this runtime's externally-callable methods.
impl Module {
    /// Insert given keyvalue into storage.
    fn tx_insert<C: TxContext>(ctx: &mut C, body: types::KeyValue) -> Result<(), Error> {
        let params = Self::params(ctx.runtime_state());

        let mut store = sdk::storage::PrefixStore::new(ctx.runtime_state(), &MODULE_NAME);
        let ts = sdk::storage::TypedStore::new(&mut store);
        let cost = match ts.get::<_, Vec<u8>>(body.key.as_slice()) {
            None => params.gas_costs.insert_absent,
            Some(_) => params.gas_costs.insert_existing,
        };
        // We must drop ts and store so that use_gas can borrow ctx.
        Core::use_tx_gas(ctx, cost)?;

        if ctx.is_check_only() {
            return Ok(());
        }

        // Recreate store and ts after we get ctx back
        let mut store = sdk::storage::PrefixStore::new(ctx.runtime_state(), &MODULE_NAME);
        let mut ts = sdk::storage::TypedStore::new(&mut store);
        let bc = body.clone();
        ts.insert(&body.key, body.value);
        ctx.emit_event(Event::Insert { kv: bc });
        Ok(())
    }

    /// Remove keyvalue from storage using given key.
    fn tx_remove<C: TxContext>(ctx: &mut C, body: types::Key) -> Result<(), Error> {
        let params = Self::params(ctx.runtime_state());

        let mut store = sdk::storage::PrefixStore::new(ctx.runtime_state(), &MODULE_NAME);
        let ts = sdk::storage::TypedStore::new(&mut store);
        let cost = match ts.get::<_, Vec<u8>>(body.key.as_slice()) {
            None => params.gas_costs.remove_absent,
            Some(_) => params.gas_costs.remove_existing,
        };
        // We must drop ts and store so that use_gas can borrow ctx.
        Core::use_tx_gas(ctx, cost)?;

        if ctx.is_check_only() {
            return Ok(());
        }

        // Recreate store and ts after we get ctx back
        let mut store = sdk::storage::PrefixStore::new(ctx.runtime_state(), &MODULE_NAME);
        let mut ts = sdk::storage::TypedStore::new(&mut store);
        let bc = body.clone();
        ts.remove(&body.key);
        ctx.emit_event(Event::Remove { key: bc });
        Ok(())
    }

    fn tx_getcreatekey<C: TxContext>(ctx: &mut C, body: types::Key) -> Result<(), Error> {
        if ctx.is_check_only() || ctx.is_simulation() {
            return Ok(());
        }

        let key_result = ctx
            .key_manager()
            .unwrap()
            .get_or_create_keys(KeyPairId::from(Hash::digest_bytes(&body.key).as_ref()));
        match key_result {
            Ok(_) => Ok(()),
            Err(err) => Err(Error::Abort(sdk::dispatcher::Error::KeyManagerFailure(err))),
        }
    }

    /// Fetch keyvalue from storage using given key.
    fn query_get<C: Context>(ctx: &mut C, body: types::Key) -> Result<types::KeyValue, Error> {
        let mut store = sdk::storage::PrefixStore::new(ctx.runtime_state(), &MODULE_NAME);
        let ts = sdk::storage::TypedStore::new(&mut store);
        let v: Vec<u8> = ts.get(body.key.clone()).ok_or(Error::InvalidArgument)?;
        Ok(types::KeyValue {
            key: body.key,
            value: v,
        })
    }
}

impl sdk::module::MigrationHandler for Module {
    type Genesis = Genesis;

    fn init_or_migrate<C: Context>(
        ctx: &mut C,
        meta: &mut sdk::modules::core::types::Metadata,
        genesis: Self::Genesis,
    ) -> bool {
        let version = meta.versions.get(Self::NAME).copied().unwrap_or_default();
        if version == 0 {
            // Initialize state from genesis.
            Self::set_params(ctx.runtime_state(), genesis.parameters);
            meta.versions.insert(Self::NAME.to_owned(), Self::VERSION);
            return true;
        }

        // Migrations are not supported.
        false
    }
}
