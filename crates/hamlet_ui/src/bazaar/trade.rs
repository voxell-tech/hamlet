//! Trading at the stalls that sell what chiefs made: signing in to
//! the bazaar with GitHub, how a stall's shelves stand, and the box a
//! listing is bought, installed or brought up to date from.

use bevy::app::{App, Update};
use bevy::ecs::change_detection::DetectChanges;
use bevy::ecs::hierarchy::ChildOf;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Local, Res};
use bevy::ecs::world::World;
use bevy::math::Vec2;
use bevy::ui::widget::{ImageNode, NodeImageMode};
use bevy::ui::{
    AlignItems, BackgroundColor, BorderColor, BorderRadius, Node,
    UiRect, percent, px,
};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, dropdown, label, row,
};
use bevy_fynix::{ViewExt, derived, keyed, resource};
use hamlet_core::core::HomeDir;
use hamlet_core::market::{
    Account, AskDetail, AskShelves, Buy, Deal, Install, Market,
    SignInBazaar, SignOutBazaar, Target, default_target, kept_target,
};
use hamlet_model::market::lot::{Kind, semver};
use hamlet_model::market::{Price, Seller};
use hamlet_rules::realm::Realm;
use hamlet_world::bazaar::Traded;

use super::artisans::{deal_box, terms};
use super::desk;
use crate::icons::Glyph;
use crate::looks::{
    ICON, action, bare, buttons, good_action, note, stated,
};
use crate::pictures::{self, Drawn, Pictures, Source, State};
use crate::platform::{DisplayText, Wraps};
use crate::roll::{either, moved};
use crate::{AnyView, Cx, Me, with_cx};

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Destination>()
        .add_systems(Update, cheer);
}

/// Where the chief chose to put what they buy next.
#[derive(Resource, Default, Clone, PartialEq, Debug)]
pub(super) struct Destination {
    /// The hamlet an agent moves into.
    hamlet: Option<String>,
    /// The house, as hamlet and agent, a plugin's tools go to.
    house: Option<(String, String)>,
}

/// The merchant in the world cheers each deal that went through.
fn cheer(
    market: Option<Res<Market>>,
    mut seen: Local<Option<(String, Deal)>>,
    mut commands: Commands,
) {
    let Some(market) = market else {
        return;
    };
    if !market.is_changed() {
        return;
    }
    if let Some((id, Deal::Done(words))) = &market.deal {
        let now = Some((id.clone(), Deal::Done(words.clone())));
        if *seen != now {
            *seen = now;
            commands.trigger(Traded);
        }
    }
}

/// A stall that sells listings of `kind` from the bazaar's server:
/// who is signed in over the desk's page, or the stall's own that
/// `page` makes.
pub(super) fn stall(kind: Kind, page: fn() -> AnyView) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        // Drawn without the bazaar's plugins in the gallery.
        cx.world.init_resource::<Market>();
        cx.world.init_resource::<Destination>();
        cx.build(
            column((account(Some(kind)), desk::or(page))).gap(14.0),
        )
    })
}

/// The widest a cover is drawn.
const COVER_WIDTH: f32 = 640.0;

/// The cover of a listing, at `path` on the bazaar's server, once it
/// has come; nothing while it comes or when it cannot.
pub(super) fn cover(path: Option<String>) -> AnyView {
    let Some(path) = path else {
        return bare().boxed();
    };
    AnyView::new(move |cx: &mut Cx| {
        let server = cx
            .world
            .get_resource::<Market>()
            .map(|market| market.server.clone())
            .unwrap_or_default();
        let address =
            format!("{}{path}", server.trim_end_matches('/'));
        let source = Source::Web(address.clone());
        let asked = cx
            .world
            .get_resource_or_init::<Pictures>()
            .state(&source);
        if asked == State::Unasked {
            pictures::fetch(cx.world, &address);
        }
        let of = source.clone();
        let state =
            resource::<Pictures, _>(move |all| all.state(&of));
        let view = keyed(state, |state| match state {
            State::Ready(drawn) => framed(drawn.clone()),
            _ => bare().boxed(),
        })
        .within(bare().width(percent(100.0)));
        let node = cx.build(view);
        cx.world.entity_mut(node).insert(source);
        node
    })
}

/// A picture drawn as wide as there is room for, up to
/// [`COVER_WIDTH`], in a frame.
fn framed(drawn: Drawn) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let within = cx.build(bare().width(percent(100.0)));
        let theme = cx.theme();
        let size = drawn.size.as_vec2().max(Vec2::ONE);
        cx.world.spawn((
            Node {
                width: percent(100.0),
                max_width: px(COVER_WIDTH),
                aspect_ratio: Some(size.x / size.y),
                border: UiRect::all(px(1.0)),
                border_radius: BorderRadius::all(px(theme
                    .space
                    .round)),
                ..Node::default()
            },
            BackgroundColor(theme.palette.plate),
            BorderColor::all(theme.palette.brass_6),
            ImageNode::new(drawn.image.clone())
                .with_mode(NodeImageMode::Stretch),
            ChildOf(within),
        ));
        within
    })
}

/// Opens the listing `id` on a page of its own, asking the bazaar
/// for all of it.
pub(super) fn ask(world: &mut World, id: &str) {
    world.trigger(AskDetail(id.to_string()));
}

/// Who is signed in to the bazaar, and the ways to sign in, out, sell
/// and see what is the chief's. Selling begins at a listing of
/// `selling`; nothing is sold at a stall of `None`.
pub(super) fn account(selling: Option<Kind>) -> AnyView {
    let now = derived(|world: &World| {
        let market = world.resource::<Market>();
        (market.account.clone(), market.said.clone())
    })
    .when(moved::<Market>());
    keyed(now, move |(account, said)| {
        let mut parts = vec![match account {
            Account::Out => buttons(vec![
                good_action(
                    Glyph::Signpost,
                    "SIGN IN WITH GITHUB",
                    |world| world.trigger(SignInBazaar),
                ),
                note(match selling {
                    Some(_) => "to buy, and to sell what you made.",
                    None => "to buy.",
                })
                .boxed(),
            ])
            .boxed(),
            Account::Asking => stated(
                Glyph::Clock,
                "Asking GitHub...",
                Tone::Dim,
                12.0,
            )
            .boxed(),
            Account::Code(device) => column((
                row((
                    note("Enter").boxed(),
                    label(device.user_code.clone())
                        .size(16.0)
                        .bold(true)
                        .tagged(DisplayText)
                        .boxed(),
                    note(format!("at {}", device.verification_uri))
                        .boxed(),
                ))
                .gap(8.0)
                .align(AlignItems::Center)
                .tagged(Wraps),
                buttons(vec![
                    action(Glyph::Signpost, "OPEN GITHUB", {
                        let uri = device.verification_uri.clone();
                        move |_| {
                            hamlet_desktop::open::browse(&uri);
                        }
                    }),
                    action(Glyph::Cross, "CANCEL", |world| {
                        world.trigger(SignOutBazaar)
                    }),
                ]),
            ))
            .gap(6.0)
            .boxed(),
            Account::In(user) => {
                let mut all = vec![
                    stated(
                        Glyph::Check,
                        format!("@{}", user.login),
                        Tone::Faint,
                        12.0,
                    )
                    .boxed(),
                ];
                if let Some(kind) = selling {
                    all.push(good_action(
                        Glyph::Plus,
                        "SELL",
                        move |world| {
                            desk::open_form(world, kind, None)
                        },
                    ));
                    all.push(action(
                        Glyph::Book,
                        "YOURS",
                        desk::open_yours,
                    ));
                }
                all.push(action(Glyph::Power, "SIGN OUT", |world| {
                    world.trigger(SignOutBazaar)
                }));
                buttons(all).boxed()
            }
            Account::Failed(why) => column((
                label(why.clone()).size(12.0).tone(Tone::Critical),
                buttons(vec![good_action(
                    Glyph::Signpost,
                    "SIGN IN WITH GITHUB",
                    |world| world.trigger(SignInBazaar),
                )]),
            ))
            .gap(6.0)
            .boxed(),
        }];
        if let Some((words, wrong)) = said {
            let tone =
                if *wrong { Tone::Critical } else { Tone::Accent };
            parts.push(
                label(words.clone()).size(12.0).tone(tone).boxed(),
            );
        }
        column(parts).gap(8.0).boxed()
    })
    .within(bare())
    .boxed()
}

/// How the shelves of listings of `kind` stand, when that is worth
/// saying: asked for, out of reach, or bare.
pub(super) fn shelves(kind: Kind) -> AnyView {
    let now = derived(move |world: &World| {
        let market = world.resource::<Market>();
        let shelves = &market.shelves;
        (
            shelves.asking,
            shelves.asked,
            shelves.problem.clone(),
            market.lots(kind).len(),
            market.server.clone(),
        )
    })
    .when(moved::<Market>());
    keyed(now, |(asking, asked, problem, count, server)| {
        if let Some(why) = problem {
            return column((
                label(format!(
                    "The bazaar at {server} could not be reached: \
                     {why}"
                ))
                .size(12.0)
                .tone(Tone::Critical),
                buttons(vec![action(
                    Glyph::Restore,
                    "TRY AGAIN",
                    |world| world.trigger(AskShelves),
                )]),
            ))
            .gap(6.0)
            .boxed();
        }
        match (*count, *asking, *asked) {
            (0, true, _) | (0, false, false) => {
                note("Asking the bazaar...").boxed()
            }
            (0, false, true) => note(
                "Nothing is for sale here yet. Sign in and press \
                 SELL to put up the first.",
            )
            .boxed(),
            _ => bare().boxed(),
        }
    })
    .within(bare())
    .boxed()
}

/// Where a listing of the chief's stands with them.
#[derive(Clone, PartialEq, Debug)]
enum Stand {
    /// They sell it.
    Mine,
    /// Written into the home at `version`; `newer` is out.
    Kept {
        version: String,
        newer: Option<String>,
    },
    /// Bought and not written into the home.
    Owned,
    ForSale,
}

/// All the box of a listing is drawn from.
#[derive(Clone, PartialEq, Debug)]
struct Now {
    stand: Stand,
    /// The deal of this listing, if one is going or just went.
    deal: Option<Deal>,
    latest: Option<String>,
    /// The places it can go, by name, and the one chosen.
    places: Vec<(String, Target)>,
    chosen: usize,
}

fn now(world: &World, kind: Kind, id: &str) -> Now {
    let market = world.resource::<Market>();
    let lot = market.lot(id);
    let latest = lot.map(|lot| lot.version().to_string());
    let mine = match (lot, market.user()) {
        (Some(lot), Some(user)) => lot.seller.id == user.id,
        _ => false,
    };
    let stand = if let Some(version) = market.kept_version(id) {
        let newer = latest.clone().filter(|latest| {
            semver(latest) > semver(version) && !latest.is_empty()
        });
        Stand::Kept {
            version: version.to_string(),
            newer,
        }
    } else if mine {
        Stand::Mine
    } else if market.owns(id) {
        Stand::Owned
    } else {
        Stand::ForSale
    };
    let deal = match &market.deal {
        Some((dealt, deal)) if dealt == id => Some(deal.clone()),
        _ => None,
    };
    let places = places(world, kind);
    let destination = world.resource::<Destination>();
    let chosen = places
        .iter()
        .position(|(_, target)| match target {
            Target::Hamlet(hamlet) => {
                destination.hamlet.as_ref() == Some(hamlet)
            }
            Target::House { hamlet, agent } => {
                destination.house.as_ref()
                    == Some(&(hamlet.clone(), agent.clone()))
            }
            Target::Realm => true,
        })
        .or_else(|| {
            let fallback = default_target(world, kind);
            places.iter().position(|(_, target)| *target == fallback)
        })
        .unwrap_or(0);
    Now {
        stand,
        deal,
        latest: latest.filter(|latest| !latest.is_empty()),
        places,
        chosen,
    }
}

/// The places a listing of `kind` can be written to: the hamlets of
/// the realm for an agent, every agent's house for a plugin.
fn places(world: &World, kind: Kind) -> Vec<(String, Target)> {
    let Some(home) = world.get_resource::<HomeDir>() else {
        return vec![("the realm".into(), Target::Realm)];
    };
    let realm = Realm::new(home.0.root.clone());
    match kind {
        Kind::Hamlet => vec![("the realm".into(), Target::Realm)],
        Kind::Agent => realm
            .hamlets()
            .iter()
            .map(|place| {
                (
                    format!("into {}", place.shown()),
                    Target::Hamlet(place.name.clone()),
                )
            })
            .collect(),
        Kind::Plugin => {
            let mut all = Vec::new();
            for place in realm.hamlets() {
                let Ok(hamlet) = realm.hamlet(&place.name) else {
                    continue;
                };
                for agent in hamlet.members() {
                    all.push((
                        format!(
                            "for {} of {}",
                            agent.name,
                            place.shown()
                        ),
                        Target::House {
                            hamlet: place.name.clone(),
                            agent: agent.name,
                        },
                    ));
                }
            }
            all
        }
    }
}

fn choose(world: &mut World, target: &Target) {
    let mut destination = world.resource_mut::<Destination>();
    match target {
        Target::Hamlet(hamlet) => {
            destination.hamlet = Some(hamlet.clone())
        }
        Target::House { hamlet, agent } => {
            destination.house = Some((hamlet.clone(), agent.clone()))
        }
        Target::Realm => {}
    }
}

/// The choice of where a listing goes.
fn place_choice(now: &Now) -> AnyView {
    let names = now
        .places
        .iter()
        .map(|(name, _)| name.clone())
        .collect::<Vec<_>>();
    let targets = now
        .places
        .iter()
        .map(|(_, target)| target.clone())
        .collect::<Vec<_>>();
    row((
        note("Goes").boxed(),
        dropdown(
            names,
            now.chosen,
            Glyph::Chevron.image(ICON),
            move |world, at| {
                if let Some(target) = targets.get(at) {
                    choose(world, target);
                }
            },
        )
        .min_width(px(180.0))
        .boxed(),
    ))
    .gap(8.0)
    .align(AlignItems::Center)
    .boxed()
}

fn by(world: &World) -> String {
    world
        .get_resource::<Me>()
        .map_or_else(|| "chief".into(), |me| me.user.clone())
}

/// The box the listing `id` of `kind` is had from: bought, installed,
/// or brought up to its newest version, with where it goes.
pub(super) fn deal(
    kind: Kind,
    id: String,
    price: Price,
    seller: &Seller,
) -> AnyView {
    let terms = terms(price, seller);
    let read = id.clone();
    let state = derived(move |world: &World| now(world, kind, &read))
        .when(either(moved::<Market>(), moved::<Destination>()));
    keyed(state, move |now| {
        let deal = deal_part(&id, now, price);
        let mut under = Vec::new();
        if now.places.len() > 1
            && matches!(now.stand, Stand::Owned | Stand::ForSale)
            && !matches!(
                now.deal,
                Some(Deal::Buying | Deal::Paying(_) | Deal::Fetching)
            )
        {
            under.push(place_choice(now));
        }
        match &now.deal {
            Some(Deal::Done(words)) => under.push(
                label(words.clone())
                    .size(12.0)
                    .tone(Tone::Accent)
                    .boxed(),
            ),
            Some(Deal::Failed(why)) => under.push(
                label(why.clone())
                    .size(12.0)
                    .tone(Tone::Critical)
                    .boxed(),
            ),
            Some(Deal::Paying(url)) => {
                let url = url.clone();
                under.push(
                    column((
                        note(
                            "Finish paying on the page that opened \
                             in your browser. It is installed as \
                             soon as Stripe says it is paid.",
                        ),
                        buttons(vec![action(
                            Glyph::Coins,
                            "OPEN THE PAGE",
                            move |_| {
                                hamlet_desktop::open::browse(&url);
                            },
                        )]),
                    ))
                    .gap(6.0)
                    .boxed(),
                )
            }
            _ => {}
        }
        if let Stand::Kept {
            newer: Some(newer), ..
        } = &now.stand
        {
            under.push(note(update_words(kind, newer)).boxed());
        }
        let terms = match now.stand {
            Stand::ForSale => terms.clone(),
            _ => String::new(),
        };
        with_cx(move |cx| {
            let mut parts =
                vec![deal_box(cx, price, terms, deal).boxed()];
            parts.extend(under);
            column(parts).gap(8.0).boxed()
        })
    })
    .within(bare())
    .boxed()
}

fn update_words(kind: Kind, newer: &str) -> String {
    let keeps = match kind {
        Kind::Agent => {
            "Updating replaces its character and skills; its notes \
             stay its own."
        }
        Kind::Hamlet => {
            "Updating replaces its agents' characters and skills and \
             moves in any new ones; their notes stay their own."
        }
        Kind::Plugin => "Updating replaces its tools.",
    };
    format!("Version {newer} is out. Updating is up to you. {keeps}")
}

fn deal_part(id: &str, now: &Now, price: Price) -> AnyView {
    match &now.deal {
        Some(Deal::Buying) => {
            return stated(
                Glyph::Coins,
                "Asking the bazaar...",
                Tone::Accent,
                12.0,
            )
            .boxed();
        }
        Some(Deal::Paying(_)) => {
            return stated(
                Glyph::Coins,
                "Waiting for Stripe...",
                Tone::Accent,
                12.0,
            )
            .boxed();
        }
        Some(Deal::Fetching) => {
            return stated(
                Glyph::Clock,
                "Setting it up...",
                Tone::Accent,
                12.0,
            )
            .boxed();
        }
        _ => {}
    }
    let target =
        now.places.get(now.chosen).map(|(_, target)| target.clone());
    let id = id.to_string();
    match &now.stand {
        Stand::Mine => buttons(vec![
            stated(Glyph::Check, "YOURS", Tone::Accent, 12.0).boxed(),
            action(Glyph::Rename, "EDIT", move |world| {
                desk::open_edit(world, &id)
            }),
        ])
        .boxed(),
        Stand::Kept { version, newer } => {
            let mut all = vec![
                stated(
                    Glyph::Check,
                    format!("INSTALLED {version}"),
                    Tone::Accent,
                    12.0,
                )
                .boxed(),
            ];
            if let Some(newer) = newer.clone() {
                all.push(good_action(
                    Glyph::Restore,
                    "UPDATE",
                    move |world| {
                        let market = world.resource::<Market>();
                        let Some(target) = kept_target(market, &id)
                        else {
                            return;
                        };
                        let by = by(world);
                        world.trigger(Install {
                            id: id.clone(),
                            version: newer.clone(),
                            target,
                            by,
                        });
                    },
                ));
            }
            buttons(all).boxed()
        }
        Stand::Owned => {
            let latest = now.latest.clone();
            good_action(Glyph::Plus, "INSTALL", move |world| {
                let (Some(target), Some(version)) =
                    (target.clone(), latest.clone())
                else {
                    return;
                };
                let by = by(world);
                world.trigger(Install {
                    id: id.clone(),
                    version,
                    target,
                    by,
                });
            })
        }
        Stand::ForSale => {
            let get = move |world: &mut World| {
                let Some(target) = target.clone() else {
                    return;
                };
                let by = by(world);
                world.trigger(Buy {
                    id: id.clone(),
                    target,
                    by,
                });
            };
            match (now.latest.is_some(), price) {
                (false, _) => note("Not ready yet.").boxed(),
                (true, Price::Free) => {
                    good_action(Glyph::Plus, "GET", get)
                }
                (true, _) => good_action(Glyph::Coins, "BUY", get),
            }
        }
    }
}
