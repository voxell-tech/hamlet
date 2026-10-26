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
    assert_eq!(server.drop_sealed().method, reqwest::Method::DELETE);
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
fn a_listing_is_read_as_the_server_sends_it() {
    let listing = serde_json::json!({
        "id": "l_1", "kind": "agent", "title": "Scribe",
        "description": "", "tags": [], "priceCents": 0,
        "currency": "usd", "status": "listed", "statusReason": null,
        "seller": {"id": "u_1", "username": "ada"}, "coverUrl": null,
        "latest": null, "owners": 0, "stars": 0, "reviews": 0,
        "createdAt": "2026-10-01T00:00:00Z",
        "updatedAt": "2026-10-01T00:00:00Z",
    });
    let lot = serde_json::from_value::<lot::Lot>(listing).unwrap();
    assert_eq!(lot.seller.username, "ada");
}

#[test]
fn an_answer_is_read_or_said_in_words() {
    let fault =
        br#"{"error":{"code":"pending","message":"not yet"}}"#;
    let refused =
        read::<Checkout>(Ok((428, fault.to_vec()))).unwrap_err();
    assert_eq!(refused.code, "pending");
    assert_eq!(refused.words(), "Not yet.");
    let bare = br#"{"code":"EMAIL_NOT_VERIFIED","message":"Email not verified"}"#;
    let refused =
        read::<Checkout>(Ok((403, bare.to_vec()))).unwrap_err();
    assert_eq!(refused.code, "EMAIL_NOT_VERIFIED");
    let unreached =
        read::<Checkout>(Err("refused".into())).unwrap_err();
    assert_eq!(unreached.code, "unreachable");
    assert_eq!(semver("1.2.3"), Some((1, 2, 3)));
    assert_eq!(semver("1.2"), None);
    assert_eq!(next_version("1.2.3"), "1.2.4");
    assert_eq!(next_version(""), "1.0.0");
}

fn account_world() -> World {
    let mut world = World::new();
    world.insert_resource(Market::new("http://bazaar.test".into()));
    world.init_resource::<crate::account::Form>();
    world.init_resource::<Stock<Wares<Agents>>>();
    world.init_resource::<Stock<Wares<Tools>>>();
    world.init_resource::<Stock<Wares<Hamlets>>>();
    world
}

fn reply(status: u16, body: serde_json::Value) -> Fetched {
    Ok((status, serde_json::to_vec(&body).unwrap()))
}

#[test]
fn signing_up_asks_for_the_emailed_code_then_signs_in_with_it() {
    use crate::account::{Form, Page, Place, open};
    let mut world = account_world();
    open(&mut world, Place::Settings, Page::SignUp);
    world.resource_mut::<Form>().email = "ada@example.com".into();
    account::answered(
        &mut world,
        account::Call::SignUp,
        reply(200, serde_json::json!({"token": null, "user": {}})),
    );
    let form = world.resource::<Form>();
    assert_eq!(form.at(Place::Settings), Page::Code);
    assert!(form.said.as_ref().unwrap().contains("ada@example.com"));
    let user = serde_json::json!({
        "id": "u_1", "email": "ada@example.com", "username": "ada",
    });
    account::answered(
        &mut world,
        account::Call::Verify,
        reply(200, serde_json::json!({"token": "t_1", "user": user})),
    );
    let market = world.resource::<Market>();
    assert_eq!(market.server.token.as_deref(), Some("t_1"));
    assert_eq!(market.user().map(lot::User::name), Some("ada"));
    assert_eq!(world.resource::<Form>().page, Page::Shut);
}

#[test]
fn an_unverified_email_turns_to_its_code_and_a_nameless_account_to_its_username()
 {
    use crate::account::{Form, Page, Place, open};
    let mut world = account_world();
    open(&mut world, Place::Bazaar, Page::SignIn);
    let unverified = serde_json::json!({
        "code": "EMAIL_NOT_VERIFIED", "message": "Email not verified",
    });
    account::answered(
        &mut world,
        account::Call::SignIn,
        reply(403, unverified),
    );
    assert_eq!(world.resource::<Form>().page, Page::Code);
    let user =
        serde_json::json!({"id": "u_1", "email": "ada@example.com"});
    account::answered(
        &mut world,
        account::Call::Verify,
        reply(200, serde_json::json!({"token": "t_1", "user": user})),
    );
    assert_eq!(world.resource::<Form>().page, Page::Username);
    let wrong = serde_json::json!({
        "code": "INVALID_EMAIL_OR_PASSWORD",
        "message": "Invalid email or password",
    });
    open(&mut world, Place::Bazaar, Page::SignIn);
    account::answered(
        &mut world,
        account::Call::SignIn,
        reply(401, wrong),
    );
    let form = world.resource::<Form>();
    assert_eq!(form.page, Page::SignIn);
    assert!(form.problem.is_some());
}

#[test]
fn signing_out_forgets_the_token_and_data_key_here() {
    let mut world = account_world();
    {
        let mut market = world.resource_mut::<Market>();
        market.server.token = Some("t_1".into());
        market.data_key =
            Some(hamlet_protocol::vault::DataKey::new());
        market.account = Account::In(lot::User::default());
    }
    sign_out(&mut world);
    let market = world.resource::<Market>();
    assert_eq!(market.server.token, None);
    assert!(market.data_key.is_none());
    assert!(!market.signed_in());
}

#[test]
fn agreeing_to_the_seller_terms_sets_up_payouts_only_when_asked_to() {
    let mut world = account_world();
    world.resource_mut::<Market>().account =
        Account::In(lot::User::default());
    world.resource_mut::<Market>().paying = false;
    account::answered(
        &mut world,
        account::Call::Agree("1".into()),
        Ok((204, Vec::new())),
    );
    let market = world.resource::<Market>();
    let agreed =
        market.user().and_then(|user| user.seller_terms.clone());
    assert_eq!(
        agreed.map(|agreed| agreed.version).as_deref(),
        Some("1")
    );
    assert!(market.said.as_ref().unwrap().0.contains("publish now"));
}
