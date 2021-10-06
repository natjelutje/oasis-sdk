//! Contract storage.
use oasis_contract_sdk_types::storage::StoreKind;
use oasis_runtime_sdk::{
    context::Context,
    keymanager::{self, KeyPair},
    storage::{self, Store},
};

use crate::{state, types, Error, MODULE_NAME};

/// Create a contract instance store.
pub fn for_instance<'a, C: Context>(
    ctx: &'a mut C,
    instance_info: &types::Instance,
    store_kind: StoreKind,
) -> Result<Box<dyn Store + 'a>, Error> {
    let key_pair: Option<KeyPair> = if let StoreKind::Confidential = store_kind {
        let kmgr_client = ctx.key_manager().ok_or(Error::Unsupported)?;
        let kid = keymanager::get_key_pair_id(&[&instance_info.id.to_storage_key()]);
        let kp = kmgr_client
            .get_or_create_keys(kid)
            .map_err(|err| Error::ExecutionFailed(err.into()))?;
        Some(kp)
    } else {
        None
    };

    let store = storage::PrefixStore::new(ctx.runtime_state(), &MODULE_NAME);
    let instance_prefix = instance_info.id.to_storage_key();
    let contract_state = storage::PrefixStore::new(
        storage::PrefixStore::new(store, &state::INSTANCE_STATE),
        instance_prefix,
    );
    let contract_state = storage::PrefixStore::new(contract_state, store_kind.prefix());

    match store_kind {
        // For public storage we use a hashed store using the Blake3 hash function.
        StoreKind::Public => Ok(Box::new(storage::HashedStore::<_, blake3::Hasher>::new(
            contract_state,
        ))),

        StoreKind::Confidential => {
            let confidential_store =
                storage::ConfidentialStore::new_with_key_pair(contract_state, key_pair.unwrap());
            Ok(Box::new(confidential_store))
        }
    }
}
