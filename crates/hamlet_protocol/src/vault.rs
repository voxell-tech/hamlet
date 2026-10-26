//! The vault: what keeps a Hamlet account's OpenRouter key so that
//! the bazaar's server can never read it.
//!
//! A random [`DataKey`] seals the key. The data key is itself locked
//! with a [`PasswordKey`], made from the account's password and a
//! random [`Salt`]. The bazaar's server keeps only the salt, the
//! locked data key and the sealed key, each as `v1:` and the
//! standard, padded base64 of its bytes.
//!
//! ```
//! use hamlet_protocol::vault::{DataKey, PasswordKey, Salt};
//!
//! let salt = Salt::new();
//! let data = DataKey::new();
//! let locked =
//!     PasswordKey::derive("a long password", &salt).lock(&data);
//! let sealed = data.seal("sk-or-v1-key");
//!
//! let salt = Salt::read(&salt.text()).unwrap();
//! let data = PasswordKey::derive("a long password", &salt)
//!     .unlock(&locked)
//!     .unwrap();
//! assert_eq!(data.unseal(&sealed).unwrap(), "sk-or-v1-key");
//! ```

use argon2::{Algorithm, Argon2, Params, Version};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chacha20poly1305::aead::rand_core::RngCore;
use chacha20poly1305::aead::{Aead, AeadCore, OsRng};
use chacha20poly1305::{Key, KeyInit, XChaCha20Poly1305, XNonce};

/// What every value of the vault begins with.
const PREFIX: &str = "v1:";

/// The bytes of a nonce, before the sealed bytes.
const NONCE: usize = 24;

/// Argon2id's memory, in KiB.
const MEMORY: u32 = 65_536;

/// Argon2id's passes.
const PASSES: u32 = 3;

/// Why a value of the vault could not be read or opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum VaultError {
    /// It is not `v1:` and base64 of the right length.
    #[error("the vault's value is not in its form")]
    Form,
    /// It was sealed with another key, or changed since.
    #[error("the vault's value does not open with this key")]
    Refused,
}

/// The 16 random bytes a [`PasswordKey`] is made with.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Salt(pub [u8; 16]);

impl Salt {
    /// A new salt of random bytes.
    pub fn new() -> Salt {
        let mut bytes = [0; 16];
        OsRng.fill_bytes(&mut bytes);
        Salt(bytes)
    }

    /// The salt as the vault keeps it.
    pub fn text(&self) -> String {
        format!("{PREFIX}{}", STANDARD.encode(self.0))
    }

    /// Reads a salt kept as [`Salt::text`] writes it.
    ///
    /// # Errors
    ///
    /// [`VaultError::Form`] when `text` is not one.
    pub fn read(text: &str) -> Result<Salt, VaultError> {
        bytes(text)?
            .try_into()
            .map(Salt)
            .map_err(|_| VaultError::Form)
    }
}

impl Default for Salt {
    fn default() -> Self {
        Salt::new()
    }
}

/// The key that locks a [`DataKey`], made from a password and a
/// [`Salt`] with Argon2id. It never leaves the computer it is made
/// on.
pub struct PasswordKey([u8; 32]);

impl PasswordKey {
    /// The key of `password` with `salt`: Argon2id with 64 MiB, three
    /// passes and one lane. It takes a while on purpose.
    pub fn derive(password: &str, salt: &Salt) -> PasswordKey {
        let params = Params::new(MEMORY, PASSES, 1, Some(32))
            .expect("the vault's parameters are Argon2's");
        let mut key = [0; 32];
        Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
            .hash_password_into(
                password.as_bytes(),
                &salt.0,
                &mut key,
            )
            .expect("a salt of 16 bytes and a key of 32");
        PasswordKey(key)
    }

    /// `data` locked, as the vault keeps it.
    pub fn lock(&self, data: &DataKey) -> String {
        seal(&self.0, &data.0)
    }

    /// The data key locked in `locked`.
    ///
    /// # Errors
    ///
    /// [`VaultError::Refused`] when it was locked with another
    /// password, [`VaultError::Form`] when it is not a locked key.
    pub fn unlock(
        &self,
        locked: &str,
    ) -> Result<DataKey, VaultError> {
        open(&self.0, locked)?
            .try_into()
            .map(DataKey)
            .map_err(|_| VaultError::Form)
    }
}

impl std::fmt::Debug for PasswordKey {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        f.write_str("PasswordKey(..)")
    }
}

/// The 32 random bytes that seal an account's OpenRouter key.
#[derive(Clone, PartialEq, Eq)]
pub struct DataKey(pub [u8; 32]);

impl DataKey {
    /// A new data key of random bytes.
    pub fn new() -> DataKey {
        let mut bytes = [0; 32];
        OsRng.fill_bytes(&mut bytes);
        DataKey(bytes)
    }

    /// The key as standard, padded base64, with no `v1:`.
    pub fn to_base64(&self) -> String {
        STANDARD.encode(self.0)
    }

    /// Reads a key written by [`DataKey::to_base64`].
    ///
    /// # Errors
    ///
    /// [`VaultError::Form`] when `text` is not one.
    pub fn from_base64(text: &str) -> Result<DataKey, VaultError> {
        STANDARD
            .decode(text)
            .map_err(|_| VaultError::Form)?
            .try_into()
            .map(DataKey)
            .map_err(|_| VaultError::Form)
    }

    /// `secret` sealed, as the vault keeps it.
    pub fn seal(&self, secret: &str) -> String {
        seal(&self.0, secret.as_bytes())
    }

    /// The secret sealed in `sealed`.
    ///
    /// # Errors
    ///
    /// [`VaultError::Refused`] when it was sealed with another key,
    /// [`VaultError::Form`] when it is not a sealed secret.
    pub fn unseal(&self, sealed: &str) -> Result<String, VaultError> {
        String::from_utf8(open(&self.0, sealed)?)
            .map_err(|_| VaultError::Form)
    }
}

impl Default for DataKey {
    fn default() -> Self {
        DataKey::new()
    }
}

impl std::fmt::Debug for DataKey {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        f.write_str("DataKey(..)")
    }
}

/// The bytes of a value of the vault.
fn bytes(text: &str) -> Result<Vec<u8>, VaultError> {
    let text = text.strip_prefix(PREFIX).ok_or(VaultError::Form)?;
    STANDARD.decode(text).map_err(|_| VaultError::Form)
}

/// `plain` sealed with `key` under a random nonce, the nonce first.
fn seal(key: &[u8; 32], plain: &[u8]) -> String {
    let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
    let sealed = XChaCha20Poly1305::new(Key::from_slice(key))
        .encrypt(&nonce, plain)
        .expect("XChaCha20-Poly1305 seals any length kept here");
    let mut out = nonce.to_vec();
    out.extend(sealed);
    format!("{PREFIX}{}", STANDARD.encode(out))
}

fn open(key: &[u8; 32], text: &str) -> Result<Vec<u8>, VaultError> {
    let bytes = bytes(text)?;
    if bytes.len() < NONCE {
        return Err(VaultError::Form);
    }
    let (nonce, sealed) = bytes.split_at(NONCE);
    XChaCha20Poly1305::new(Key::from_slice(key))
        .decrypt(XNonce::from_slice(nonce), sealed)
        .map_err(|_| VaultError::Refused)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_sealed_and_locked_opens_with_the_same_password_alone() {
        let salt = Salt::read(&Salt::new().text()).unwrap();
        let data = DataKey::new();
        let locked =
            PasswordKey::derive("first password", &salt).lock(&data);
        let sealed = data.seal("sk-or-v1-abc");
        let opened = PasswordKey::derive("first password", &salt)
            .unlock(&locked)
            .unwrap();
        assert_eq!(opened, data);
        assert_eq!(opened.unseal(&sealed).unwrap(), "sk-or-v1-abc");
        assert_eq!(
            DataKey::from_base64(&data.to_base64()).unwrap(),
            data
        );
        assert_eq!(
            PasswordKey::derive("second password", &salt)
                .unlock(&locked),
            Err(VaultError::Refused)
        );
        assert_eq!(
            DataKey::new().unseal(&sealed),
            Err(VaultError::Refused)
        );
        assert_eq!(data.unseal("v2:AAAA"), Err(VaultError::Form));
        assert_eq!(data.unseal("v1:AAAA"), Err(VaultError::Form));
    }

    #[test]
    fn a_password_key_is_argon2id_of_the_contract() {
        let salt = Salt(*b"0123456789abcdef");
        let key = PasswordKey::derive(
            "correct horse battery staple",
            &salt,
        );
        let hex = key
            .0
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        // As argon2-cffi's `hash_secret_raw` makes it.
        assert_eq!(
            hex,
            "94c86f541abdb3d9aabfea59aa03963549483e9c0b1a79336e76b54cee6c917e"
        );
    }
}
