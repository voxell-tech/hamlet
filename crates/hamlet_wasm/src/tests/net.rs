use bevy::prelude::*;
use hamlet_sdk::capability::{Capability, Subject};

use super::acts::{does, open, probe};
use super::permissions::petitions;
use crate::fixture::home::Scratch;
use crate::inbox::Inbox;
use crate::interface::exports::hamlet::plugin::plugin::{
    FetchAnswer, FetchRequest, HttpMethod, Incoming, Intent,
};

fn fetch(url: &str) -> Vec<Intent> {
    vec![Intent::Fetch(FetchRequest {
        id: 7,
        method: HttpMethod::Get,
        url: url.into(),
        content_type: String::new(),
        body: Vec::new(),
    })]
}

/// The answer to the fetch 7 that reached the plugin, if one did.
fn answered(app: &mut App) -> Option<Result<(), String>> {
    let mut inbox = app.world_mut().query::<&mut Inbox>();
    let mail = inbox.single_mut(app.world_mut()).unwrap().take();
    mail.into_iter().find_map(|incoming| match incoming {
        Incoming::Fetched(FetchAnswer { id: 7, outcome }) => {
            Some(outcome.map(drop))
        }
        _ => None,
    })
}

const HOST: &[(&str, &[&str])] = &[("net", &["api.example.com"])];

#[test]
fn a_fetch_is_refused_with_why_and_asks_the_chief_where_it_is_yet_to_decide()
 {
    let scratch = Scratch::new("net_asked");
    let mut app = open(
        &scratch,
        probe()
            .asking(&["net"])
            .asking_next_time(&["net"])
            .limited_to(HOST),
    );

    does(&mut app, fetch("https://api.example.com/x"));
    assert_eq!(
        answered(&mut app),
        Some(Err("net is yet to be decided".into()))
    );
    assert!(matches!(
        &petitions(&mut app)[..],
        [(
            _,
            Subject::Use {
                capability: Capability::Net,
                ..
            }
        )]
    ));
}

#[test]
fn a_fetch_outside_the_listed_hosts_or_not_asked_for_is_refused_without_asking()
 {
    let scratch = Scratch::new("net_scope");
    let mut app = open(
        &scratch,
        probe().asking(&["net"]).granted(&["net"]).limited_to(HOST),
    );
    for url in [
        "https://evil.example.com/x",
        "http://api.example.com/x",
        "https://api.example.com@evil.com/x",
    ] {
        does(&mut app, fetch(url));
        let Some(Err(why)) = answered(&mut app) else {
            panic!("{url} was not refused");
        };
        assert!(
            why.contains("not a host") || why.contains("https"),
            "{why}"
        );
    }
    assert!(petitions(&mut app).is_empty());

    // A host that is listed goes on, to a network that is not there.
    does(&mut app, fetch("https://api.example.com/x"));
    assert_eq!(
        answered(&mut app),
        Some(Err("there is no network".into()))
    );

    let scratch = Scratch::new("net_unnamed");
    let mut app = open(&scratch, probe());
    does(&mut app, fetch("https://api.example.com/x"));
    assert_eq!(
        answered(&mut app),
        Some(Err("net not granted".into()))
    );
    assert!(petitions(&mut app).is_empty());
}
