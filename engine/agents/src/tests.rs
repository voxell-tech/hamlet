use hamlet_core::core::{Link, Listing, Settings};
use hamlet_engine_api::agents::AgentsReply;
use hamlet_engine_api::{Reply, encode};
use hamlet_model::config::{Config, Endpoint};

use crate::setup::SetupState;
use crate::views;

const KEY: &str = "sk-never-out";

fn keyed() -> Endpoint {
    Endpoint {
        name: "openai".into(),
        base_url: "https://api.openai.com/v1".into(),
        chatgpt: false,
        api_key: Some(KEY.into()),
    }
}

#[test]
fn no_reply_carries_a_key() {
    let settings = Settings {
        config: Config {
            endpoints: vec![keyed()],
            ..Config::default()
        },
        error: None,
    };
    let home = std::env::temp_dir().join("hamlet-no-such-home");
    let told = views::settings_of(
        &settings,
        &Link::Unknown,
        &Listing::default(),
        &home,
    );
    assert!(told.endpoints[0].has_key);
    let setup = SetupState {
        asked: Some(keyed()),
        ..SetupState::default()
    };
    for reply in [
        AgentsReply::Settings(told),
        AgentsReply::Setup(setup.view()),
    ] {
        let bytes = encode(&Reply::Agents(reply)).unwrap();
        assert!(
            !bytes.windows(KEY.len()).any(|at| at == KEY.as_bytes())
        );
    }
}
