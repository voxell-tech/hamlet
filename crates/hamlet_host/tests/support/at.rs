//! A test's address.

use hamlet_host::Address;

/// The address of the actor of `kind` and `key`, which a test names
/// plainly.
pub fn at(kind: &str, key: &str) -> Address {
    Address::new(kind, key).unwrap()
}
