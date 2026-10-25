use super::client::{Server, escaped, read};
use super::deal::{Act, Deal, Event, Order};
use super::lot::{Checkout, Kind, Paying, next_version, semver};
use super::*;

fn order() -> Order {
    Order {
        id: "l_1".into(),
        kind: Kind::Agent,
        title: "Scribe".into(),
        version: "1.0.0".into(),
        size: 10,
        into: Some("home".into()),
    }
}

#[test]
fn a_paid_deal_waits_for_stripe_then_downloads_and_settles() {
    let order = order();
    let checkout = Checkout {
        owned: false,
        purchase_id: Some("p_1".into()),
        checkout_url: Some("https://checkout".into()),
    };
    let (deal, act) =
        Deal::Buying.next(&order, Event::Bought(Ok(checkout)), 0);
    assert_eq!(act, Some(Act::Open("https://checkout".into())));
    assert!(matches!(deal, Deal::Paying { .. }));
    // Not due yet; then due, and asked once.
    let (deal, act) = deal.next(&order, Event::Tick(1), 1);
    assert_eq!(act, None);
    let (deal, act) = deal.next(&order, Event::Tick(10_000), 10_000);
    assert_eq!(act, Some(Act::Poll("p_1".into())));
    let (deal, act) = deal.next(&order, Event::Tick(20_000), 20_000);
    assert_eq!(act, None, "an ask is already out");
    let (deal, act) =
        deal.next(&order, Event::Paid(Ok(Paying::Paid)), 21_000);
    assert_eq!((&deal, act), (&Deal::Fetching, Some(Act::Download)));
    // A package of another size than the bazaar said is refused.
    let wrong = deal.clone().next(&order, Event::Package(Ok(9)), 0);
    assert!(matches!(wrong.0, Deal::Failed(_)));
    let (deal, act) = deal.next(&order, Event::Package(Ok(10)), 0);
    assert_eq!((&deal, act), (&Deal::Settling, Some(Act::Settle)));
    let (deal, _) = deal.next(
        &order,
        Event::Settled(Ok("scribe moved into home.".into())),
        0,
    );
    assert_eq!(deal, Deal::Done("scribe moved into home.".into()));
}

#[test]
fn a_free_or_owned_listing_goes_straight_to_its_download() {
    let order = order();
    let free = Checkout {
        owned: true,
        purchase_id: None,
        checkout_url: None,
    };
    let (deal, act) =
        Deal::Buying.next(&order, Event::Bought(Ok(free)), 0);
    assert_eq!((deal, act), (Deal::Fetching, Some(Act::Download)));
    let owned = client::Refusal {
        status: Some(409),
        code: "conflict".into(),
        message: "Owned.".into(),
    };
    let (deal, _) =
        Deal::Buying.next(&order, Event::Bought(Err(owned)), 0);
    assert_eq!(deal, Deal::Fetching);
    let (deal, _) = Deal::Settling.next(
        &order,
        Event::Settled(Err("taken".into())),
        0,
    );
    assert!(
        matches!(deal, Deal::Failed(why) if why.contains("taken"))
    );
}

#[test]
fn the_server_is_asked_with_its_token_as_a_header_and_its_methods() {
    let server = Server {
        base: client::server_of(Some(" https://bazaar.dev/ ".into())),
        token: Some("hms_1".into()),
    };
    let detail = server.detail("l 1");
    assert_eq!(detail.url, "https://bazaar.dev/v1/listings/l%201");
    assert_eq!(detail.token.as_deref(), Some("hms_1"));
    assert_eq!(server.sign_out().method, reqwest::Method::DELETE);
    let draft = lot::Draft::default();
    assert_eq!(
        server.change("l_1", &draft).method,
        reqwest::Method::PATCH
    );
    let release =
        server.release("l_1", "1.0.0", "", br#"{"format":1}"#);
    assert!(release.long);
    let (kind, body) = release.body.unwrap();
    assert_eq!(kind, "application/json");
    let body =
        serde_json::from_slice::<serde_json::Value>(&body).unwrap();
    assert_eq!(body["package"]["format"], 1);
    assert_eq!(client::server_of(None), client::DEFAULT_SERVER);
    assert_eq!(escaped("a/b?"), "a%2Fb%3F");
}

#[test]
fn an_answer_is_read_or_said_in_words() {
    let fault =
        br#"{"error":{"code":"pending","message":"not yet"}}"#;
    let refused =
        read::<Checkout>(Ok((428, fault.to_vec()))).unwrap_err();
    assert_eq!(refused.code, "pending");
    assert_eq!(refused.words(), "Not yet.");
    let unreached =
        read::<Checkout>(Err("refused".into())).unwrap_err();
    assert_eq!(unreached.code, "unreachable");
    assert_eq!(semver("1.2.3"), Some((1, 2, 3)));
    assert_eq!(semver("1.2"), None);
    assert_eq!(next_version("1.2.3"), "1.2.4");
    assert_eq!(next_version(""), "1.0.0");
}
