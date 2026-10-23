use std::net::IpAddr;

use crate::{public, reach};

#[test]
fn only_public_addresses_are_reached() {
    let refused = [
        "127.0.0.1",
        "10.1.2.3",
        "172.16.0.1",
        "192.168.1.1",
        "169.254.169.254",
        "100.64.0.1",
        "0.0.0.0",
        "255.255.255.255",
        "::1",
        "fc00::1",
        "fe80::1",
        "::ffff:127.0.0.1",
        "::127.0.0.1",
        "64:ff9b::10.0.0.1",
        "2002:a00:1::1",
        "fec0::1",
        "198.18.0.1",
        "240.0.0.1",
    ];
    for address in refused {
        let address = address.parse::<IpAddr>().unwrap();
        assert!(!public(address), "{address} was reached");
    }
    assert!(public("203.0.113.7".parse().unwrap()));
    assert!(public("2001:db8::7".parse().unwrap()));
    assert!(public("64:ff9b::203.0.113.7".parse().unwrap()));

    // A name is resolved first, and refused for where it is.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    for host in ["localhost", "127.0.0.1", "[::1]"] {
        let reached = runtime.block_on(reach(host, 443));
        assert!(reached.is_err(), "{host} was reached");
    }
}
