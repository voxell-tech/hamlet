//! The bazaar's server as the plugin reads it, its deals step by
//! step, and packages in parts.

use hamlet_engine_bevy::api::Method;
use hamlet_engine_bevy::api::agents::Destination;
use hamlet_engine_bevy::{Failure, FetchReply, Reply};

use crate::market::account::{Asking, Polled, polled};
use crate::market::client::{Server, bytes, read};
use crate::market::deal::{
    Act, Deal, Event, Order, PART_BYTES, PAYMENT_PATIENCE,
    PAYMENT_POLL, parts_of,
};
use crate::market::lot::{
    Checkout, Detail, Kind, Lot, Page, Paying, PluginMeta, Status,
};
use crate::market::sell::part_at;
use crate::market::session::{Session, token_for};

fn answer(status: u16, body: &str) -> Result<Reply, Failure> {
    Ok(Reply::Fetched(FetchReply {
        status,
        content_type: "application/json".into(),
        body: body.as_bytes().to_vec(),
    }))
}

const LISTING: &str = r#"{"id":"l_1","kind":"plugin","title":"Cal",
    "summary":"s","description":"","category":"Data","tags":["a"],
    "priceCents":250,"currency":"usd","status":"listed",
    "seller":{"id":"u","login":"octo","name":null,"avatarUrl":null,
    "verified":true},"coverUrl":"/v1/listings/l_1/cover?v=2",
    "latest":{"version":"1.0.0","changes":"","size":9,"sha256":"x",
    "createdAt":"2026-10-01T00:00:00Z","meta":{"tools":[{"name":"cal",
    "description":"d","permissions":["run_program","network"]}]}},
    "owners":3,"stars":4.5,"reviews":2,
    "createdAt":"2026-09-01T00:00:00Z",
    "updatedAt":"2026-10-01T00:00:00Z"}"#;

#[test]
fn a_listing_is_read_as_the_server_sends_it() {
    let lot = serde_json::from_str::<Lot>(LISTING).unwrap();
    assert_eq!(lot.id, "l_1");
    assert_eq!(lot.kind, Kind::Plugin);
    assert_eq!(lot.status, Status::Listed);
    assert_eq!(lot.seller.shown(), "octo");
    assert_eq!(lot.updated(), "2026-10-01");
    assert_eq!(
        lot.cover_url.as_deref(),
        Some("/v1/listings/l_1/cover?v=2")
    );
    assert_eq!(lot.latest.as_ref().unwrap().size, 9);
    let tools = lot.meta::<PluginMeta>().tools;
    assert_eq!(tools[0].permissions, ["run_program", "network"]);
    let page = format!(r#"{{"items":[{LISTING}],"total":1}}"#);
    let page = read::<Page<Lot>>(answer(200, &page)).unwrap();
    assert_eq!(page.items.len(), 1);
    let detail = format!(
        r#"{{"listing":{LISTING},"versions":[],"reviews":[{{"by":
        {{"login":"ann","avatarUrl":""}},"stars":5,"words":"Good.",
        "createdAt":""}}],"owned":true,"mine":false}}"#
    );
    let detail = read::<Detail>(answer(200, &detail)).unwrap();
    assert!(detail.owned);
    assert_eq!(detail.reviews[0].by.login, "ann");
}

#[test]
fn a_refusal_says_the_servers_words_and_code() {
    let refused = bytes(answer(
        409,
        r#"{"error":{"code":"conflict","message":"you own this already"}}"#,
    ))
    .unwrap_err();
    assert_eq!(refused.code, "conflict");
    assert_eq!(refused.words(), "You own this already.");
    assert!(bytes(answer(401, "{}")).unwrap_err().signed_out());
    let held = bytes(Err(Failure::OutOfBudget("4 fetches".into())));
    assert_eq!(held.unwrap_err().code, "busy");
    let unreached = bytes(Err(Failure::Refused("not https".into())));
    assert_eq!(unreached.unwrap_err().code, "unreachable");
}

#[test]
fn a_sign_in_is_asked_after_until_it_is_approved() {
    let pending =
        r#"{"error":{"code":"pending","message":"not yet"}}"#;
    assert_eq!(polled(answer(428, pending)), Polled::Pending);
    let slow = r#"{"error":{"code":"slow_down","message":"slow"}}"#;
    assert_eq!(polled(answer(429, slow)), Polled::SlowDown);
    let expired = r#"{"error":{"code":"expired","message":"gone"}}"#;
    assert!(matches!(
        polled(answer(410, expired)),
        Polled::Failed(_)
    ));
    let approved = r#"{"token":"hms_1","user":{"login":"ann"}}"#;
    match polled(answer(200, approved)) {
        Polled::Approved(approved) => {
            assert_eq!(approved.token, "hms_1")
        }
        other => panic!("{other:?}"),
    }
    let mut asking = Asking::new(0, 5_000, 60_000);
    assert!(!asking.due(4_999));
    assert!(asking.due(5_000));
    asking.out = true;
    assert!(!asking.due(9_000));
    asking.heard(9_000);
    assert!(asking.due(14_000));
    assert!(asking.over(60_001));
}

#[test]
fn a_call_carries_its_token_in_the_query_and_names_its_method() {
    let server = Server {
        base: "http://localhost:8787".into(),
        token: Some("hms_1".into()),
    };
    let part =
        server.package_part("l_1", "1.0.0", 393_216, PART_BYTES);
    assert_eq!(part.method, Method::Get);
    assert_eq!(
        part.url,
        "http://localhost:8787/v1/listings/l_1/versions/1.0.0/package\
         ?offset=393216&limit=393216&token=hms_1"
    );
    let out = server.sign_out();
    assert_eq!(out.method, Method::Post);
    assert_eq!(
        out.url,
        "http://localhost:8787/v1/auth/session?_method=DELETE&token=hms_1"
    );
    let poll = server.poll("dev ice");
    assert_eq!(
        poll.content_type.as_deref(),
        Some("application/json")
    );
    assert_eq!(poll.body, br#"{"deviceCode":"dev ice"}"#);
    let up = server.upload_part("l_1", "up_2", 3, vec![1, 2]);
    assert!(up.url.ends_with(
        "/v1/listings/l_1/uploads/up_2/parts/3?token=hms_1"
    ));
    assert_eq!(up.body, [1, 2]);
}

#[test]
fn a_session_is_kept_only_for_its_server() {
    let session = Session {
        server: "http://localhost:8787".into(),
        token: "hms_1".into(),
    };
    assert_eq!(
        token_for(&session, "http://localhost:8787").as_deref(),
        Some("hms_1")
    );
    assert_eq!(token_for(&session, "https://elsewhere"), None);
    let out = Session {
        token: String::new(),
        ..session
    };
    assert_eq!(token_for(&out, "http://localhost:8787"), None);
}

fn order(size: u64) -> Order {
    Order {
        id: "l_1".into(),
        kind: Kind::Agent,
        title: "Ada".into(),
        version: "1.0.0".into(),
        size,
        to: Destination::Hamlet("mill".into()),
    }
}

fn checkout(owned: bool) -> Checkout {
    Checkout {
        owned,
        purchase_id: (!owned).then(|| "p_1".into()),
        checkout_url: (!owned)
            .then(|| "https://checkout.stripe.com/x".into()),
    }
}

#[test]
fn a_free_listing_is_streamed_part_by_part_and_settled() {
    // Ten megabytes, the server's most: 27 parts, the last short.
    let size = 10 * 1024 * 1024;
    let order = order(size);
    let parts = parts_of(size);
    assert_eq!(parts, 27);
    let (mut deal, mut act) = Deal::Buying.next(
        &order,
        Event::Bought(Ok(checkout(true))),
        0,
    );
    for part in 0..parts {
        let from = u64::from(part) * PART_BYTES;
        assert_eq!(act, Some(Act::Download(from)));
        let wanted = (size - from).min(PART_BYTES) as usize;
        (deal, act) =
            deal.next(&order, Event::Part(Ok(vec![b' '; wanted])), 0);
        let Some(Act::Settle {
            part: at,
            parts: of,
            bytes,
        }) = &act
        else {
            panic!("{act:?}");
        };
        assert_eq!((*at, *of, bytes.len()), (part, parts, wanted));
        if part + 1 < parts {
            (deal, act) = deal.next(&order, Event::Stored, 0);
        }
    }
    // The last part is answered with what came of it, not Stored.
    let (stored, act2) = deal.clone().next(&order, Event::Stored, 0);
    assert_eq!((&stored, act2), (&deal, None));
    let (deal, act) = deal.next(
        &order,
        Event::Settled(Ok("Ada moved in.".into())),
        0,
    );
    assert_eq!(
        (deal, act),
        (Deal::Done("Ada moved in.".into()), None)
    );
}

#[test]
fn a_part_that_comes_short_or_long_fails() {
    let order = order(PART_BYTES + 100);
    let fetching = Deal::Fetching(0);
    let short = vec![0; PART_BYTES as usize - 1];
    let (deal, _) =
        fetching.clone().next(&order, Event::Part(Ok(short)), 0);
    assert!(matches!(deal, Deal::Failed(_)));
    let (deal, _) = Deal::Fetching(PART_BYTES).next(
        &order,
        Event::Part(Ok(vec![0; 101])),
        0,
    );
    assert!(matches!(deal, Deal::Failed(_)));
    // A part settled before the last does not end the deal.
    let (deal, _) = Deal::Settling(PART_BYTES).next(
        &order,
        Event::Settled(Ok("early".into())),
        0,
    );
    assert_eq!(deal, Deal::Settling(PART_BYTES));
}

#[test]
fn a_paid_listing_waits_for_stripe_then_downloads() {
    let order = order(9);
    let (deal, act) = Deal::Buying.next(
        &order,
        Event::Bought(Ok(checkout(false))),
        0,
    );
    assert_eq!(
        act,
        Some(Act::Open("https://checkout.stripe.com/x".into()))
    );
    assert!(matches!(deal, Deal::Paying { .. }));
    let (deal, act) =
        deal.next(&order, Event::Tick(PAYMENT_POLL - 1), 0);
    assert_eq!(act, None);
    let (deal, act) = deal.next(&order, Event::Tick(PAYMENT_POLL), 0);
    assert_eq!(act, Some(Act::Poll("p_1".into())));
    // Asked once at a time.
    let (deal, act) =
        deal.next(&order, Event::Tick(PAYMENT_POLL + 1), 0);
    assert_eq!(act, None);
    let (deal, act) = deal.next(
        &order,
        Event::Paid(Ok(Paying::Pending)),
        PAYMENT_POLL,
    );
    assert!(matches!(deal, Deal::Paying { .. }) && act.is_none());
    let (deal, act) =
        deal.next(&order, Event::Paid(Ok(Paying::Paid)), 0);
    assert_eq!(
        (deal, act),
        (Deal::Fetching(0), Some(Act::Download(0)))
    );
}

#[test]
fn a_payment_not_made_or_not_finished_fails() {
    let order = order(9);
    let (paying, _) = Deal::Buying.next(
        &order,
        Event::Bought(Ok(checkout(false))),
        0,
    );
    let (deal, _) = paying.clone().next(
        &order,
        Event::Paid(Ok(Paying::Expired)),
        0,
    );
    assert!(matches!(deal, Deal::Failed(_)));
    let (deal, _) =
        paying.next(&order, Event::Tick(PAYMENT_PATIENCE + 1), 0);
    assert!(matches!(deal, Deal::Failed(_)));
}

#[test]
fn a_listing_owned_already_is_only_installed() {
    let refused = bytes(answer(
        409,
        r#"{"error":{"code":"conflict","message":"owned"}}"#,
    ))
    .unwrap_err();
    let (deal, act) =
        Deal::Buying.next(&order(9), Event::Bought(Err(refused)), 0);
    assert_eq!(
        (deal, act),
        (Deal::Fetching(0), Some(Act::Download(0)))
    );
}

#[test]
fn what_does_not_belong_to_its_step_changes_nothing() {
    let (deal, act) = Deal::Settling(9).next(
        &order(9),
        Event::Part(Ok(vec![1])),
        0,
    );
    assert_eq!((deal, act), (Deal::Settling(9), None));
}

#[test]
fn a_package_goes_up_in_parts() {
    let size = 2 * PART_BYTES + 1;
    assert_eq!(parts_of(size), 3);
    assert_eq!(part_at(size, 0), (0, PART_BYTES));
    assert_eq!(part_at(size, 2), (2 * PART_BYTES, 1));
    assert_eq!(part_at(size, 3).1, 0);
    assert_eq!(parts_of(PART_BYTES), 1);
}
