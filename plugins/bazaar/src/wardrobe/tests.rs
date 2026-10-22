//! The Asset stall's catalogue as the server sends it, what is worn
//! as the realm is told it, buying, and the pictures of a turntable.

use std::collections::BTreeMap;

use hamlet_engine_bevy::api::Method;
use hamlet_engine_bevy::{Failure, FetchReply, Reply};
use looks::wardrobe::{Asset, Category, Target};
use looks::{Outfit, Worn};

use super::turntable::{Turning, frames, next};
use super::*;
use crate::covers::{Covers, MEMORY, wanted};
use crate::market::Paid;
use crate::market::client::{Refusal, Server};
use crate::market::lot::Paying;
use crate::picture::tests::png;
use crate::shelf::arranged;

fn answer(status: u16, body: &str) -> Result<Reply, Failure> {
    Ok(Reply::Fetched(FetchReply {
        status,
        content_type: "application/json".into(),
        body: body.as_bytes().to_vec(),
    }))
}

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
    read::<Page<Asset>>(answer(200, CATALOGUE)).unwrap().items
}

#[test]
fn the_catalogue_is_read_as_the_server_sends_it_and_found() {
    let all = catalogue();
    assert_eq!(all.len(), 3);
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
        arranged(&all, words, &filter, order)
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
        titles((None, Some(Category::Mesh)), "", Order::Kind),
        ["Well"]
    );
    assert_eq!(
        titles((None, None), "skin pinks", Order::Kind),
        ["Sakura"]
    );

    let server = Server {
        base: "http://bazaar".into(),
        token: Some("t k".into()),
    };
    let part =
        server.asset_file_part("agent-skin-sakura", 458_752, 458_752);
    assert_eq!(part.method, Method::Get);
    assert_eq!(
        part.url,
        "http://bazaar/v1/assets/agent-skin-sakura/file?offset=458752\
         &limit=458752&token=t%20k"
    );
    assert_eq!(
        server.assets().url,
        "http://bazaar/v1/assets?token=t%20k"
    );
    assert_eq!(
        server.asset_purchase("w").url,
        "http://bazaar/v1/assets/w/purchase?token=t%20k"
    );
    assert_eq!(server.asset_purchase("w").method, Method::Post);
    assert_eq!(
        server.asset_purchase_state("ap_1").url,
        "http://bazaar/v1/asset-purchases/ap_1?token=t%20k"
    );
}

/// A wardrobe owning the catalogue's agent skin and emote, and the
/// hamlet's mesh at another version than the shelf's.
fn wardrobe() -> Wardrobe {
    let mut wardrobe = Wardrobe::default();
    for mut asset in catalogue() {
        if asset.target == Target::Hamlet {
            asset.version = "1.0.0".into();
        }
        wardrobe.owned.insert(asset.id.clone(), asset);
    }
    wardrobe
}

#[test]
fn the_realm_is_told_what_is_owned_worn_and_whole_in_the_store() {
    let mut wardrobe = wardrobe();
    let wearing = &mut wardrobe.wearing;
    wearing.wear(
        Target::Agent,
        "ada",
        Category::Skin,
        Some("agent-skin-sakura".into()),
    );
    wearing.wear(
        Target::Agent,
        "ada",
        Category::Emote,
        Some("agent-emote-wave".into()),
    );
    wearing.wear(
        Target::Agent,
        "bo",
        Category::Skin,
        Some("agent-skin-sakura".into()),
    );
    wearing.wear(
        Target::Hamlet,
        "home",
        Category::Mesh,
        Some("hamlet-mesh-well".into()),
    );
    // One not owned is chosen, and not worn.
    wearing.wear(
        Target::Hamlet,
        "home",
        Category::Skin,
        Some("hamlet-skin-moss".into()),
    );
    let skin = Worn::file("agent-skin-sakura", "1.0.0");
    let wave = Worn::file("agent-emote-wave", "1.2.0");
    let well = Worn::file("hamlet-mesh-well", "1.0.0");
    assert_eq!(
        wardrobe.wanted().into_keys().collect::<Vec<_>>(),
        [wave.clone(), skin.clone(), well.clone()]
    );
    assert_eq!(wardrobe.worn(&BTreeMap::new()), Worn::default());

    // Only what is whole in the store is told.
    let stored =
        BTreeMap::from([(skin.clone(), 2), (well.clone(), 1)]);
    let worn = wardrobe.worn(&stored);
    let on = |file: &str| Some(file.to_string());
    assert_eq!(
        worn.agents["ada"],
        Outfit {
            skin: on(&skin),
            ..Outfit::default()
        }
    );
    assert_eq!(worn.agents["bo"].skin, on(&skin));
    assert_eq!(
        worn.hamlets["home"],
        Outfit {
            mesh: on(&well),
            ..Outfit::default()
        }
    );
    assert_eq!(worn.parts, stored);

    // Taken off, it is gone, and so is one that wears nothing.
    wardrobe
        .wearing
        .wear(Target::Agent, "bo", Category::Skin, None);
    assert!(wardrobe.wearing.of(Target::Agent, "bo").is_none());
    assert_eq!(
        wardrobe.wearing.wearers("agent-skin-sakura"),
        [(Target::Agent, "ada".to_string(), Category::Skin)]
    );
    let worn = wardrobe.worn(&stored);
    assert!(!worn.agents.contains_key("bo"));
    // Not owned any more, nothing of it is worn or its parts kept.
    wardrobe.owned.remove("hamlet-mesh-well");
    let worn = wardrobe.worn(&stored);
    assert!(worn.hamlets.is_empty());
    assert_eq!(worn.parts.keys().collect::<Vec<_>>(), [&skin]);

    let kept = serde_json::to_vec(&wardrobe.kept()).unwrap();
    let kept = serde_json::from_slice::<Kept>(&kept).unwrap();
    assert_eq!(kept.wearing, wardrobe.wearing);
    assert_eq!(kept.owned.len(), 2);
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
    let later = now + crate::market::PAYMENT_POLL;
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

fn spun() -> (Asset, Turning) {
    let sakura = catalogue().remove(0);
    let mut turning = Turning::default();
    turning.spin(&sakura, true);
    (sakura, turning)
}

#[test]
fn a_turntable_wants_its_pictures_while_shown_and_lets_them_go() {
    let (sakura, mut turning) = spun();
    let all = frames(&sakura, &turning);
    assert_eq!(all.len(), 24);
    // Turning, it shows the next picture that came, and waits on the
    // one it shows while none other did.
    let came = [all[0].clone(), all[5].clone()];
    assert_eq!(
        next(&sakura, 0, |url| came.iter().any(|c| c == url)),
        5
    );
    assert_eq!(
        next(&sakura, 5, |url| came.iter().any(|c| c == url)),
        0
    );
    assert_eq!(next(&sakura, 5, |_| false), 5);

    let mut covers = Covers::default();
    covers.want(wanted(all.clone()));
    for url in &all {
        covers.came(url, Ok(png(256, 256))).unwrap();
    }
    assert_eq!(covers.memory(), 24 * 256 * 256 * 4);
    assert!(covers.memory() <= MEMORY);

    // Stopped and turned by hand, only the one shown and the first
    // are kept; the rest are let go of.
    turning.step(&sakura, -1);
    assert!(!turning.moving());
    let shown = frames(&sakura, &turning);
    assert_eq!(shown, [sakura.frame(23), sakura.frame(0)]);
    covers.want(wanted(shown));
    assert_eq!(covers.memory(), 2 * 256 * 256 * 4);
    assert!(covers.get(&sakura.frame(5)).is_none());
    // Its page closed, none are.
    covers.want(Vec::new());
    assert_eq!(covers.memory(), 0);
    // An asset without pictures wants none.
    let well = catalogue().remove(2);
    assert!(frames(&well, &Turning::default()).is_empty());
}
