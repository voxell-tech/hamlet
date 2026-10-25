//! Finding the server a client connects to.

use hamlet_protocol::client::discovery::ServerAddress;
use hamlet_testing::Scratch;

#[test]
fn explicit_server_and_token_win_over_the_home() {
    let address = ServerAddress::find(|key| match key {
        "HAMLET_SERVER" => Some("ws://127.0.0.1:7313".into()),
        "HAMLET_TOKEN" => Some("window-token".into()),
        "HAMLET_HOME" => panic!("An explicit server needs no home."),
        _ => None,
    })
    .unwrap();
    assert_eq!(address.url, "ws://127.0.0.1:7313");
    assert_eq!(address.token, "window-token");
}

#[test]
fn the_local_home_supplies_only_the_url_and_connection_token() {
    let home = Scratch::new("connection-find-home");
    std::fs::write(
        home.join("server.json"),
        r#"{"url":"ws://127.0.0.1:7314","pid":7,"started":"2026-01-01T00:00:00.000Z"}"#,
    )
    .unwrap();
    std::fs::write(
        home.join("credentials.json"),
        r#"{"connection":{"token":"home-token"},"models":false}"#,
    )
    .unwrap();
    let address = ServerAddress::find(|key| {
        (key == "HAMLET_HOME").then(|| home.display().to_string())
    })
    .unwrap();
    assert_eq!(address.url, "ws://127.0.0.1:7314");
    assert_eq!(address.token, "home-token");
}
