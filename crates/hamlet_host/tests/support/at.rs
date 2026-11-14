use hamlet_host::Address;

pub fn at(kind: &str, key: &str) -> Address {
    Address::new(kind, key).unwrap()
}
