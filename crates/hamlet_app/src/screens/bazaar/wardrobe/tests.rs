//! The LOOKS stall's catalogue as the server sends it, who wears
//! what as the home's server says, buying, a file come in parts and
//! the pictures of a turntable.

use bevy::app::App;
use hamlet_client::outbox::Outbox;
use hamlet_realm::dress::Looks;
use hamlet_realm::wardrobe::{Asset, Category, Target};
use reqwest::Method;

use super::closet::{Closet, PART, came};
use super::turntable::{Turning, frames, next};
use super::*;
use crate::bazaar::market::client::{Refusal, Server};
use crate::bazaar::market::deal::PAYMENT_POLL;
use crate::bazaar::market::lot::Paying;
use crate::bazaar::shelf::arranged_of;

const CATALOGUE: &str = r#"{"items":[
    {"id":"agent-skin-sakura","target":"agent","category":"skin",
     "title":"Sakura","summary":"Soft pinks.","priceCents":99,
     "currency":"usd","version":"1.0.0","frames":24,
     "previewUrl":"/v1/assets/agent-skin-sakura/preview/{frame}?v=1.0.0",
     "owned":false,"updatedAt":"2026-10-01T00:00:00Z"},
    {"id":"agent-emote-wave","target":"agent","category":"emote",
     "title":"Wave","summary":"Waves.","priceCents":0,
     "version":"1.2.0","frames":0,"previewUrl":"","owned":true},
    {"id":"hamlet-mesh-well","target":"hamlet","category":"mesh",
     "title":"Well","summary":"A well for each house.",
     "priceCents":500,"version":"2.0.0"}]}"#;

fn catalogue() -> Vec<Asset> {
    let answer = Ok((200, CATALOGUE.as_bytes().to_vec()));
    read::<Page<Asset>>(answer).unwrap().items
}

#[test]
fn the_catalogue_is_read_as_the_server_sends_it_and_found() {
    let all = catalogue();
    let sakura = &all[0];
    assert_eq!(
        (sakura.target, sakura.category),
        (Target::Agent, Category::Skin)
    );
    assert_eq!(
        sakura.frame(3),
        "/v1/assets/agent-skin-sakura/preview/3?v=1.0.0"
    );
    assert!(all[1].owned);
    // What the server leaves out is none.
    assert_eq!((all[2].frames, all[2].preview_url.as_str()), (0, ""));

    let titles = |filter: (Option<Target>, Option<Category>),
                  words: &str,
                  order: Order| {
        arranged_of(&all, words, &filter, order)
            .iter()
            .map(|asset| asset.title.as_str())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        titles((None, None), "", Order::Kind),
        ["Wave", "Sakura", "Well"]
    );
    assert_eq!(
        titles((None, None), "", Order::Dear),
        ["Well", "Sakura", "Wave"]
    );
    assert_eq!(
        titles((Some(Target::Agent), None), "", Order::Cheap),
        ["Wave", "Sakura"]
    );
    assert_eq!(
        titles((None, None), "skin pinks", Order::Kind),
        ["Sakura"]
    );

    let server = Server {
        base: "http://bazaar".into(),
        token: Some("tk".into()),
    };
    let part =
        server.asset_file_part("agent-skin-sakura", PART, PART);
    assert_eq!(part.method, Method::GET);
    assert_eq!(
        part.url,
        "http://bazaar/v1/assets/agent-skin-sakura/file?offset=524288\
         &limit=524288"
    );
    assert_eq!(part.token.as_deref(), Some("tk"));
    let buy = server.asset_purchase("w");
    assert_eq!(
        (buy.method, buy.url.as_str()),
        (Method::POST, "http://bazaar/v1/assets/w/purchase")
    );
    assert_eq!(
        server.asset_purchase_state("ap_1").url,
        "http://bazaar/v1/asset-purchases/ap_1"
    );
}

fn looks(skin: Option<&str>, emote: Option<&str>) -> protocol::Looks {
    protocol::Looks {
        skin: skin.map(String::from),
        mesh: None,
        emote: emote.map(String::from),
    }
}

#[test]
fn who_wears_an_asset_is_found_by_its_id_whatever_its_version() {
    let agent = |name: &str| Who::Agent {
        hamlet: "home".into(),
        name: name.into(),
    };
    let dressed = Dressed(vec![
        Dresser {
            who: agent("ada"),
            words: "ada".into(),
            looks: looks(
                Some("agent-skin-sakura@1.0.0"),
                Some("agent-emote-wave@1.2.0"),
            ),
        },
        Dresser {
            who: agent("bo"),
            words: "bo".into(),
            looks: looks(Some("agent-skin-sakura@0.9.0"), None),
        },
        Dresser {
            who: Who::Hamlet("home".into()),
            words: "Pear Tree".into(),
            looks: protocol::Looks::default(),
        },
    ]);
    let wearers = dressed
        .wearers("agent-skin-sakura")
        .into_iter()
        .map(|(one, category)| (one.words.as_str(), category))
        .collect::<Vec<_>>();
    assert_eq!(
        wearers,
        [("ada", Category::Skin), ("bo", Category::Skin)]
    );
    assert_eq!(
        dressed.files(),
        [
            "agent-emote-wave@1.2.0",
            "agent-skin-sakura@0.9.0",
            "agent-skin-sakura@1.0.0"
        ]
    );
    assert_eq!(dressed.of(Target::Hamlet).count(), 1);
}

#[test]
fn an_asset_is_owned_once_claimed_or_paid() {
    let now = 1_000;
    let free = pay(
        Deal::Buying,
        Event::Bought(Ok(Checkout {
            owned: true,
            purchase_id: None,
            checkout_url: None,
        })),
        now,
    );
    assert_eq!(free, Paid::Owned);
    let owned_already = Refusal {
        status: Some(409),
        code: "conflict".into(),
        message: "Owned.".into(),
    };
    assert_eq!(
        pay(Deal::Buying, Event::Bought(Err(owned_already)), now),
        Paid::Owned
    );
    let Paid::Going(paying, Some(Act::Open(url))) = pay(
        Deal::Buying,
        Event::Bought(Ok(Checkout {
            owned: false,
            purchase_id: Some("ap_1".into()),
            checkout_url: Some("https://stripe/pay".into()),
        })),
        now,
    ) else {
        panic!("a page to pay on");
    };
    assert_eq!(url, "https://stripe/pay");
    let later = now + PAYMENT_POLL;
    let Paid::Going(paying, Some(Act::Poll(purchase))) =
        pay(paying, Event::Tick(later), later)
    else {
        panic!("asked whether it was paid");
    };
    assert_eq!(purchase, "ap_1");
    assert_eq!(
        pay(paying, Event::Paid(Ok(Paying::Paid)), later),
        Paid::Owned
    );
}

/// An app with a wardrobe and a closet, signed in to the bazaar,
/// that answers nothing it asks.
fn closet_app() -> App {
    let mut app = App::new();
    app.init_resource::<Wardrobe>()
        .init_resource::<Dressed>()
        .init_resource::<Closet>()
        .init_resource::<Looks>()
        .init_resource::<Outbox>()
        .insert_resource(Market::new("http://bazaar".into()));
    app.world_mut().resource_mut::<Market>().server.token =
        Some("tk".into());
    app
}

#[test]
fn a_file_worn_comes_in_parts_and_is_put_in_the_realm_whole() {
    let file = "agent-skin-sakura@1.0.0".to_string();
    let bytes = (0..PART as usize * 2 + 10)
        .map(|at| (at % 251) as u8)
        .collect::<Vec<_>>();
    let mut download = closet::Download::of(file.clone());
    let mut parts = 0;
    while !download.took(
        bytes[download.from() as usize..]
            .iter()
            .take(PART as usize)
            .copied()
            .collect(),
    ) {
        parts += 1;
    }
    assert_eq!((parts, download.from()), (2, bytes.len() as u64));
    // A file of whole parts ends with an empty one.
    let mut whole = closet::Download::of(file.clone());
    assert!(!whole.took(vec![0; PART as usize]));
    assert!(whole.took(Vec::new()));

    // The last part puts it in the realm.
    let mut app = closet_app();
    let world = app.world_mut();
    world.resource_mut::<Closet>().download =
        Some(closet::Download::of(file.clone()));
    came(world, file.clone(), 0, Ok((206, vec![7; 10])));
    assert!(world.resource::<Closet>().download.is_none());
    assert_eq!(world.resource::<Looks>().files[&file], vec![7; 10]);

    // A part that does not follow is let be; one that fails is
    // missed, and asked for again later.
    world.resource_mut::<Closet>().download =
        Some(closet::Download::of("x@1".into()));
    came(world, "x@1".into(), 9, Ok((206, vec![1])));
    assert!(world.resource::<Closet>().download.is_some());
    came(world, "x@1".into(), 0, Err("down".into()));
    assert!(world.resource::<Closet>().missed("x@1"));
}

#[test]
fn a_turntable_wants_its_pictures_while_it_turns() {
    let sakura = catalogue().remove(0);
    let mut turning = Turning::default();
    turning.spin(&sakura, true);
    let all = frames(&sakura, &turning);
    assert_eq!(all.len(), 24);
    // Turning, it shows the next picture that came, and waits on the
    // one it shows while none other did.
    let came = [all[0].clone(), all[5].clone()];
    let has = |url: &str| came.iter().any(|c| c == url);
    assert_eq!(next(&sakura, 0, has), 5);
    assert_eq!(next(&sakura, 5, has), 0);
    assert_eq!(next(&sakura, 5, |_| false), 5);
    // Stopped and turned by hand, only the one shown and the first
    // are wanted.
    turning.step(&sakura, -1);
    assert!(!turning.moving());
    assert_eq!(
        frames(&sakura, &turning),
        [sakura.frame(23), sakura.frame(0)]
    );
    // An asset without pictures wants none.
    let well = catalogue().remove(2);
    assert!(frames(&well, &Turning::default()).is_empty());
}
