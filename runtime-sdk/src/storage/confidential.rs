use std::convert::TryInto as _;

use anyhow::{anyhow, Error};

use crate::{
    context::Context,
    core::{
        common::crypto::{
            hash::Hash,
            mrae::deoxysii::{self, NONCE_SIZE},
        },
        storage::mkvs,
    },
    keymanager::{KeyManagerClientWithContext, KeyPair, KeyPairId},
    storage::Store,
};

type Nonce = [u8; NONCE_SIZE];

/// A key-value store that encrypts all content with DeoxysII.
pub struct ConfidentialStore<S: Store> {
    inner: S,
    deoxys: deoxysii::DeoxysII,
}

impl<S: Store> ConfidentialStore<S> {
    /// Create a new confidential store with the given keypair.
    pub fn new_with_key_pair(inner: S, keypair: KeyPair) -> Self {
        let mut actual_key = [0u8; deoxysii::KEY_SIZE];
        actual_key.copy_from_slice(&keypair.state_key.0[..deoxysii::KEY_SIZE]);
        ConfidentialStore {
            inner,
            deoxys: deoxysii::DeoxysII::new(&actual_key),
        }
    }

    pub fn new_with_id_from_client(
        kmgr: &KeyManagerClientWithContext<'_>,
        inner: S,
        kid: KeyPairId,
    ) -> Result<Self, Error> {
        let keypair = kmgr.get_or_create_keys(kid)?;
        Ok(Self::new_with_key_pair(inner, keypair))
    }

    /// Create a new confidential store with the given keymanager key id.
    pub fn new_with_id<C: Context>(ctx: &C, inner: S, kid: KeyPairId) -> Result<Self, Error> {
        let kmgr = ctx
            .key_manager()
            .ok_or_else(|| anyhow!("confidential transactions not available"))?;
        Self::new_with_id_from_client(kmgr, inner, kid)
    }

    fn pack_key(&self, enc_key: &[u8], nonce: &Nonce) -> Vec<u8> {
        let mut ret = Vec::with_capacity(nonce.len() + enc_key.len());
        ret.extend_from_slice(nonce);
        ret.extend_from_slice(enc_key);
        ret
    }

    fn make_key(&self, plain_key: &[u8]) -> (Nonce, Vec<u8>) {
        let mut nonce = [0u8; NONCE_SIZE];
        let plain_hash = Hash::digest_bytes(plain_key);
        nonce.copy_from_slice(plain_hash.truncated(NONCE_SIZE));
        let enc_key = self.deoxys.seal(&nonce, plain_key.to_vec(), nonce.to_vec());
        let key = self.pack_key(&enc_key, &nonce);
        (nonce, key)
    }

    fn unpack_key<'a>(&self, raw_key: &'a [u8]) -> Option<(&'a Nonce, &'a [u8])> {
        if raw_key.len() <= NONCE_SIZE {
            return None;
        }
        let nonce_ref: &'a Nonce = raw_key[..NONCE_SIZE]
            .try_into()
            .expect("nonce size mismatch");
        Some((nonce_ref, &raw_key[NONCE_SIZE..]))
    }

    fn get_key(&self, raw_key: &[u8]) -> Result<(Nonce, Vec<u8>), Error> {
        match self.unpack_key(raw_key) {
            Some((nonce, enc_key_ref)) => {
                let enc_key = Vec::from(enc_key_ref);
                let key = self.deoxys.open(nonce, enc_key, nonce.to_vec())?;
                Ok((*nonce, key))
            }
            None => Err(anyhow!("raw key value not long enough")),
        }
    }

    fn get_value(&self, enc_value: &[u8], nonce: &Nonce) -> Result<Vec<u8>, Error> {
        let enc_val_vec = Vec::from(enc_value);
        Ok(self.deoxys.open(nonce, enc_val_vec, nonce.to_vec())?)
    }

    fn make_value(&self, value: &[u8], nonce: &Nonce) -> Vec<u8> {
        self.deoxys.seal(nonce, value.to_vec(), nonce.to_vec())
    }
}

impl<S: Store> Store for ConfidentialStore<S> {
    fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        let (nonce, inner_key) = self.make_key(key);
        match self.inner.get(&inner_key) {
            None => None,
            Some(inner_value) => {
                let value = self
                    .get_value(&inner_value, &nonce)
                    .expect("error decrypting value");
                Some(value)
            }
        }
    }

    fn insert(&mut self, key: &[u8], value: &[u8]) {
        let (nonce, inner_key) = self.make_key(key);
        let raw_value = self.make_value(value, &nonce);
        self.inner.insert(&inner_key, &raw_value)
    }

    fn remove(&mut self, key: &[u8]) {
        let (_, inner_key) = self.make_key(key);
        self.inner.remove(&inner_key)
    }

    fn iter(&self) -> Box<dyn mkvs::Iterator + '_> {
        Box::new(ConfidentialStoreIterator::new(self))
    }
}

struct ConfidentialStoreIterator<'store, S: Store> {
    inner: Box<dyn mkvs::Iterator + 'store>,
    store: &'store ConfidentialStore<S>,

    key: Option<mkvs::Key>,
    value: Option<Vec<u8>>,
    error: Option<Error>,
}

impl<'store, S: Store> ConfidentialStoreIterator<'store, S> {
    fn new(store: &'store ConfidentialStore<S>) -> ConfidentialStoreIterator<'_, S> {
        ConfidentialStoreIterator {
            inner: store.inner.iter(),
            store,
            key: None,
            value: None,
            error: None,
        }
    }

    fn reset(&mut self) {
        self.key = None;
        self.value = None;
        self.error = None;
    }

    fn load(&mut self, inner_key: &[u8], inner_value: &[u8]) {
        if !mkvs::Iterator::is_valid(self) {
            return;
        }

        match self.store.get_key(inner_key) {
            Ok((nonce, key)) => match self.store.get_value(inner_value, &nonce) {
                Ok(value) => {
                    self.key = Some(key);
                    self.value = Some(value);
                }
                Err(err) => {
                    self.error = Some(err);
                }
            },
            Err(err) => {
                self.error = Some(err);
            }
        }
    }

    fn reset_and_load(&mut self) {
        self.reset();
        if self.inner.is_valid() {
            if let Some(ref inner_key) = self.inner.get_key().clone() {
                if let Some(ref inner_value) = self.inner.get_value().clone() {
                    self.load(inner_key, inner_value);
                } else {
                    self.error = Some(anyhow!("no value in valid inner iterator"));
                }
            } else {
                self.error = Some(anyhow!("no key in valid inner iterator"));
            }
        }
    }
}

impl<'store, S: Store> Iterator for ConfidentialStoreIterator<'store, S> {
    type Item = (Vec<u8>, Vec<u8>);

    fn next(&mut self) -> Option<Self::Item> {
        self.reset();
        match Iterator::next(&mut *self.inner) {
            Some(item) => {
                self.load(&item.0, &item.1);
                if mkvs::Iterator::is_valid(self) {
                    Some((self.key.clone().unwrap(), self.value.clone().unwrap()))
                } else {
                    None
                }
            }
            None => None,
        }
    }
}

impl<'store, S: Store> mkvs::Iterator for ConfidentialStoreIterator<'store, S> {
    fn set_prefetch(&mut self, prefetch: usize) {
        self.inner.set_prefetch(prefetch)
    }

    fn is_valid(&self) -> bool {
        self.error.is_none() && self.inner.is_valid()
    }

    fn error(&self) -> &Option<Error> {
        match self.error {
            Some(_) => &self.error,
            None => self.inner.error(),
        }
    }

    fn rewind(&mut self) {
        self.inner.rewind();
        self.reset_and_load();
    }

    fn seek(&mut self, key: &[u8]) {
        let (_, inner_key) = self.store.make_key(key);
        self.inner.seek(&inner_key);
        self.reset_and_load();
    }

    fn get_key(&self) -> &Option<mkvs::Key> {
        &self.key
    }

    fn get_value(&self) -> &Option<Vec<u8>> {
        &self.value
    }

    fn next(&mut self) {
        mkvs::Iterator::next(&mut *self.inner);
        self.reset_and_load();
    }
}
