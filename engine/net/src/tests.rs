use std::io::{Read, Write};
use std::net::{IpAddr, TcpListener};
use std::time::Duration;

use hamlet_engine_api::{
    Ask, Capability, FetchRequest, Manifest, Method, PluginId,
};
use hamlet_engine_plugins::{PluginInfo, PluginKind};

use crate::address::NetPluginInfoExt;
use crate::fetch::send;
use crate::{reach, reachable};

/// A plugin that asks for `net.fetch` on each of `targets`, with the
/// variables of `env` and their defaults, the engine's environment
/// as `lookup` gives it.
fn plugin(
    targets: &[&str],
    env: &[(&str, &str)],
    lookup: impl Fn(&str) -> Option<String>,
) -> PluginInfo {
    let mut manifest = Manifest::new(
        PluginId::try_from("test.fetching").unwrap(),
        "Fetching",
        "0.1.0",
    );
    manifest.asks = targets
        .iter()
        .map(|target| Ask {
            capability: Capability::NetFetch,
            target: (*target).into(),
            reason: "Fetches".into(),
        })
        .collect();
    for (name, default) in env {
        manifest.env.insert((*name).into(), (*default).into());
    }
    assert_eq!(manifest.check(), Ok(()));
    PluginInfo::looking_up(manifest, PluginKind::Installed, lookup)
}

/// The target of the ask `url` is on, if it is listed.
fn target(info: &PluginInfo, url: &str) -> Option<String> {
    info.listed(url).ok().map(|(_, target)| target)
}

#[test]
fn only_multicast_broadcast_and_unspecified_are_not_reached() {
    let refused = [
        "0.0.0.0",
        "0.1.2.3",
        "255.255.255.255",
        "224.0.0.1",
        "239.255.255.250",
        "::",
        "ff02::1",
        "::ffff:224.0.0.1",
        "::ffff:0.0.0.0",
    ];
    for address in refused {
        let address = address.parse::<IpAddr>().unwrap();
        assert!(!reachable(address), "{address} was reached");
    }
    let reached = [
        "127.0.0.1",
        "10.1.2.3",
        "172.16.0.1",
        "192.168.1.1",
        "169.254.169.254",
        "100.64.0.1",
        "203.0.113.7",
        "::1",
        "fc00::1",
        "fe80::1",
        "::ffff:127.0.0.1",
        "2001:db8::7",
    ];
    for address in reached {
        let address = address.parse::<IpAddr>().unwrap();
        assert!(reachable(address), "{address} was not reached");
    }

    // A name is resolved first, and refused for where it is.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let patience = Duration::from_secs(5);
    for host in ["localhost", "127.0.0.1", "[::1]"] {
        let reached = runtime.block_on(reach(host, 80, patience));
        assert!(reached.is_ok(), "{host} was not reached");
    }
    for host in ["0.0.0.0", "[::]", "255.255.255.255"] {
        let reached = runtime.block_on(reach(host, 80, patience));
        assert!(reached.is_err(), "{host} was reached");
    }
}

#[test]
fn a_host_is_at_its_schemes_port_unless_it_names_one() {
    let info = plugin(
        &[
            "api.example.org",
            "localhost:8787",
            "[::1]:9000",
            "10.0.0.2",
        ],
        &[],
        |_| None,
    );
    let listed = [
        ("https://api.example.org/a", "api.example.org"),
        ("http://api.example.org/a", "api.example.org"),
        ("https://API.example.org:443", "api.example.org"),
        ("http://api.example.org:80", "api.example.org"),
        ("http://localhost:8787/x", "localhost:8787"),
        ("https://localhost:8787/x", "localhost:8787"),
        ("http://[::1]:9000/", "[::1]:9000"),
        ("http://10.0.0.2/", "10.0.0.2"),
    ];
    for (url, wanted) in listed {
        assert_eq!(
            target(&info, url).as_deref(),
            Some(wanted),
            "{url}"
        );
    }
    let refused = [
        "https://api.example.org:8443/",
        "http://api.example.org:443/",
        "https://api.example.org:80/",
        "http://localhost/",
        "http://localhost:8788/",
        "ftp://api.example.org/",
        "file:///etc/passwd",
        "http://someone@api.example.org/",
        "https://elsewhere.example.org/",
        "not an address",
    ];
    for url in refused {
        assert_eq!(target(&info, url), None, "{url} was listed");
    }
}

#[test]
fn an_ask_on_a_variable_is_on_the_host_it_holds() {
    let env = [("HAMLET_BAZAAR_URL", "http://localhost:8787")];
    let unset = plugin(&["$HAMLET_BAZAAR_URL"], &env, |_| None);
    assert_eq!(unset.asks[0].target, "localhost:8787");
    assert_eq!(
        target(&unset, "http://localhost:8787/listings").as_deref(),
        Some("localhost:8787")
    );
    assert_eq!(target(&unset, "https://bazaar.example.com/"), None);
    assert_eq!(
        unset.shown(Capability::NetFetch, "localhost:8787"),
        "localhost:8787 (HAMLET_BAZAAR_URL)"
    );
    assert_eq!(
        unset.env,
        [(
            "HAMLET_BAZAAR_URL".to_owned(),
            "http://localhost:8787".to_owned()
        )]
    );

    let set = plugin(&["$HAMLET_BAZAAR_URL"], &env, |name| {
        (name == "HAMLET_BAZAAR_URL")
            .then(|| "https://Bazaar.example.com/v1".to_owned())
    });
    assert_eq!(set.asks[0].target, "bazaar.example.com");
    assert_eq!(
        target(&set, "https://bazaar.example.com/listings")
            .as_deref(),
        Some("bazaar.example.com")
    );
    assert_eq!(target(&set, "http://localhost:8787/"), None);
    assert_eq!(
        set.shown(Capability::NetFetch, "bazaar.example.com"),
        "bazaar.example.com (HAMLET_BAZAAR_URL)"
    );

    // Set empty, it is the default; set to no address, it is on no
    // host.
    let empty =
        plugin(
            &["$HAMLET_BAZAAR_URL"],
            &env,
            |_| Some(String::new()),
        );
    assert_eq!(empty.asks[0].target, "localhost:8787");
    let nowhere = plugin(&["$HAMLET_BAZAAR_URL"], &env, |_| {
        Some("bazaar".to_owned())
    });
    assert_eq!(nowhere.asks[0].target, "$HAMLET_BAZAAR_URL");
    assert_eq!(target(&nowhere, "http://bazaar/"), None);

    // Asked for both by name and by variable, it is asked once.
    let both = plugin(
        &["localhost:8787", "$HAMLET_BAZAAR_URL"],
        &env,
        |_| None,
    );
    assert_eq!(both.asks.len(), 1);
}

#[test]
fn a_fetch_goes_over_http_to_this_computer() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut asked = [0; 1024];
        let read = stream.read(&mut asked).unwrap();
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\
                  Content-Length: 5\r\nConnection: close\r\n\r\nhello",
            )
            .unwrap();
        String::from_utf8_lossy(&asked[..read]).into_owned()
    });
    let info = plugin(&[&format!("localhost:{port}")], &[], |_| None);
    let url = format!("http://localhost:{port}/hello");
    let (url, _) = info.listed(&url).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let fetch = FetchRequest {
        url: url.to_string(),
        method: Method::Get,
        content_type: None,
        body: Vec::new(),
    };
    let reply = runtime
        .block_on(send(url, fetch, Duration::from_secs(5), 1_024))
        .unwrap();
    assert_eq!(reply.status, 200);
    assert_eq!(reply.body, b"hello");
    assert!(
        server.join().unwrap().starts_with("GET /hello HTTP/1.1")
    );
}
