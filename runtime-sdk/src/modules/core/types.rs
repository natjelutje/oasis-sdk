use std::collections::BTreeMap;

use crate::{
    keymanager::SignedPublicKey,
    types::{address::Address, transaction::Transaction},
};

/// Key in the versions map used for the global state version.
pub const VERSION_GLOBAL_KEY: &str = "";

/// Per-module metadata.
#[derive(Clone, Debug, Default, cbor::Encode, cbor::Decode)]
pub struct Metadata {
    /// A set of state versions for all supported modules.
    pub versions: BTreeMap<String, u32>,
}

// CallerAddress is the EstimateGas caller address.
#[derive(Clone, Debug, cbor::Encode, cbor::Decode)]
pub enum CallerAddress {
    #[cbor(rename = "address")]
    Address(Address),
    #[cbor(rename = "eth_address")]
    EthAddress([u8; 20]),
}

/// Arguments for the EstimateGas query.
#[derive(Clone, Debug, cbor::Encode, cbor::Decode)]
pub struct EstimateGasQuery {
    /// The address of the caller for which to do estimation. If not specified the authentication
    /// information from the passed transaction is used.
    #[cbor(optional)]
    pub caller: Option<CallerAddress>,
    /// The unsigned transaction to estimate.
    pub tx: Transaction,
}

/// Response to the call data public key query.
#[derive(Clone, Debug, cbor::Encode, cbor::Decode)]
pub struct CallDataPublicKeyQueryResponse {
    /// Public key used for deriving the shared secret for encrypting call data.
    pub public_key: SignedPublicKey,
}
