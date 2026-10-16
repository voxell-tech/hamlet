//! The Hamlet Store's wares: whole hamlets other chiefs put up at the
//! bazaar's server, each shown as a diorama of its town, and bought
//! to stand in the realm with its mayor, its agents and all they
//! know.

use bevy::app::{App, Update};
use bevy::asset::Handle;
use bevy::camera::visibility::InheritedVisibility;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::ChildOf;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::schedule::common_conditions::resource_changed;
use bevy::ecs::system::{Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::image::Image;
use bevy::math::Vec3;
use bevy::time::Time;
use bevy::transform::components::GlobalTransform;
use bevy::ui::widget::{ImageNode, NodeImageMode};
use bevy::ui::{
    AlignItems, BackgroundColor, BorderColor, BorderRadius,
    ComputedNode, Node, UiRect, percent, px,
};
use bevy::window::RequestRedraw;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, button, column, label, row,
};
use bevy_fynix::{ScopedExt, ViewExt, derived, keyed, resource};
use hamlet_core::agents::{Charter, Settlement};
use hamlet_core::market::{Deal, Market};
use hamlet_home::limits::Hamlets;
use hamlet_model::market::hamlets::{Listing, Settler};
use hamlet_model::market::lot::Kind;
use hamlet_model::market::{Price, Seller, Standing};
use hamlet_rules::hamlet::HOME;
use hamlet_world::diorama::{self, Dioramas, Model};
use hamlet_world::pick::{Thing, focus_on, select};

use super::trade;
use crate::icons::Glyph;
use crate::looks::{
    action, bare, buttons, note, page_head, plate, stated, title,
};
use crate::roll::{either, moved};
use crate::{AnyView, Cx, Roll};

/// Seconds the world is given to raise a bought hamlet's town before
/// the camera stops waiting to show it.
const SEEK: f32 = 6.0;

/// The width of the still of a hamlet in the list, and the share its
/// pictures are wider than tall.
const STILL_WIDTH: f32 = 136.0;
const ASPECT: f32 = 1.6;

/// The widest the turning picture is drawn.
const TURNING_WIDTH: f32 = 640.0;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Store>()
        .init_resource::<Holdings>()
        .add_systems(
            Update,
            (
                count.run_if(resource_changed::<Roll>),
                turn,
                arrived.run_if(resource_changed::<Market>),
                seek.run_if(|store: Res<Store>| {
                    store.seeking.is_some()
                }),
            ),
        );
}

/// What the chief is doing at the Hamlet Store.
#[derive(Resource, Default, Clone, PartialEq, Debug)]
pub struct Store {
    /// The hamlet open on a page of its own, by its listing's id.
    pub open: Option<String>,
    /// A hamlet just bought, whose town the camera waits to show
    /// until the time given.
    seeking: Option<(Entity, f32)>,
    /// The listing whose arrival was last shown.
    shown: Option<String>,
}

/// The hamlets of the realm by name, and the most the chief's limits
/// allow.
#[derive(Resource, Clone, PartialEq, Debug)]
struct Holdings {
    hamlets: Vec<(String, Entity)>,
    most: usize,
}

impl Default for Holdings {
    fn default() -> Self {
        Holdings {
            hamlets: Vec::new(),
            most: Hamlets::default().most,
        }
    }
}

impl Holdings {
    fn owns(&self, name: &str) -> Option<Entity> {
        self.hamlets
            .iter()
            .find(|(owned, _)| owned == name)
            .map(|(_, hamlet)| *hamlet)
    }

    fn full(&self) -> bool {
        self.hamlets.len() >= self.most
    }
}

/// The picture of a hamlet's diorama that turns while it is seen.
#[derive(Component)]
struct Turning(Model);

/// Keeps [`Holdings`] in step with the hamlets of the realm.
fn count(
    settlements: Query<(Entity, &Settlement, Option<&Charter>)>,
    mut holdings: ResMut<Holdings>,
) {
    let mut now = Holdings::default();
    for (hamlet, settlement, charter) in &settlements {
        now.hamlets.push((settlement.name.clone(), hamlet));
        if settlement.name == HOME
            && let Some(Charter(Some(limits))) = charter
        {
            now.most = limits.hamlets.most;
        }
    }
    now.hamlets.sort();
    if *holdings != now {
        *holdings = now;
    }
}

/// Turns the diorama of the hamlet whose picture is in sight, and
/// none while none is.
fn turn(
    pictures: Query<(&Turning, &ComputedNode, &InheritedVisibility)>,
    dioramas: Option<ResMut<Dioramas>>,
) {
    let Some(mut dioramas) = dioramas else {
        return;
    };
    // A tab that is not in front is laid out with no size.
    let wanted = pictures
        .iter()
        .find(|(_, node, shown)| shown.get() && node.size().x > 0.0)
        .map(|(turning, ..)| turning.0);
    if dioramas.turning != wanted {
        dioramas.turning = wanted;
    }
}

/// The diorama of `listing`'s town.
fn model(listing: &Listing) -> Model {
    Model {
        seed: listing.seed,
        agents: listing.agents.len() as u32 + 1,
    }
}

fn now(world: &World) -> f32 {
    world.resource::<Time>().elapsed_secs()
}

/// Why no more hamlets can be bought.
fn full(holdings: &Holdings) -> String {
    format!(
        "Your realm holds as many hamlets as your limits allow, {} \
         of {}. Raise Most hamlets in Settings, under Realm, or \
         close a hamlet to make room.",
        holdings.hamlets.len(),
        holdings.most
    )
}

/// Selects a hamlet just bought, once it stands in the realm, and
/// brings its town into view.
fn arrived(world: &mut World) {
    let market = world.resource::<Market>();
    let Some((id, Deal::Done(_))) = market.deal.clone() else {
        return;
    };
    let Some(kept) =
        market.kept.get(&id).filter(|k| k.kind == Kind::Hamlet)
    else {
        return;
    };
    let name = kept.names.first().cloned().unwrap_or_default();
    if world.resource::<Store>().shown.as_ref() == Some(&id) {
        return;
    }
    let hamlet = world
        .query::<(Entity, &Settlement)>()
        .iter(world)
        .find(|(_, settlement)| settlement.name == name)
        .map(|(hamlet, _)| hamlet);
    let Some(hamlet) = hamlet else {
        return;
    };
    world.resource_mut::<Store>().shown = Some(id);
    select(world, Some(Thing::Common(hamlet)));
    let until = now(world) + SEEK;
    world.resource_mut::<Store>().seeking = Some((hamlet, until));
    crate::dock::show_tab(world, "world");
}

/// Brings the town of the hamlet just bought into view once the
/// world has raised it.
fn seek(world: &mut World) {
    let Some((hamlet, until)) = world.resource::<Store>().seeking
    else {
        return;
    };
    let thing = Thing::Common(hamlet);
    // A town just raised is placed once its transform has been
    // propagated, and no town stands at the world's middle.
    let there = world
        .query::<(&Thing, &GlobalTransform)>()
        .iter(world)
        .any(|(t, at)| *t == thing && at.translation() != Vec3::ZERO);
    if there {
        focus_on(world, thing, false);
    }
    if there || now(world) > until {
        world.resource_mut::<Store>().seeking = None;
    } else {
        world.write_message(RequestRedraw);
    }
}

/// Opens the town of the hamlet `hamlet` in the world.
fn visit(world: &mut World, hamlet: Entity) {
    let thing = Thing::Common(hamlet);
    select(world, Some(thing));
    crate::dock::show_tab(world, "world");
    focus_on(world, thing, false);
}

/// A count with its thousands set apart.
fn thousands(count: u32) -> String {
    let digits = count.to_string();
    let mut out = String::new();
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

fn seller_words(seller: &Seller) -> String {
    match seller.verified {
        true => format!("{}, verified seller", seller.name),
        false => format!("{}, not verified", seller.name),
    }
}

fn standing_words(standing: &Standing) -> String {
    format!(
        "{:.1} of 5 from {} reviews · {} own it",
        standing.stars,
        thousands(standing.reviews),
        thousands(standing.owners)
    )
}

fn price_words(price: Price) -> String {
    match price {
        Price::Free => "Free".into(),
        Price::Once(_) => format!("{}, paid once", price.words()),
    }
}

/// A picture of a diorama, sized by `node`, in a frame. It turns
/// while it is seen when it is `turning`.
fn picture(
    cx: &mut Cx,
    image: Option<Handle<Image>>,
    node: Node,
    turning: Option<Model>,
) -> Entity {
    let theme = cx.theme();
    let (ground, edge, round) = (
        theme.palette.well,
        theme.palette.brass_6,
        theme.space.round,
    );
    let within = cx.build(
        bare()
            .width(node.width)
            .max_width(node.max_width)
            .shrink(0.0),
    );
    let mut framed = cx.world.spawn((
        Node {
            border: UiRect::all(px(1.0)),
            border_radius: BorderRadius::all(px(round)),
            width: percent(100.0),
            max_width: bevy::ui::Val::Auto,
            ..node
        },
        BackgroundColor(ground),
        BorderColor::all(edge),
        ChildOf(within),
    ));
    if let Some(image) = image {
        framed.insert(
            ImageNode::new(image).with_mode(NodeImageMode::Stretch),
        );
    }
    if let Some(model) = turning {
        framed.insert(Turning(model));
    }
    within
}

/// The still of `model`, small, for a row of the list.
fn still(model: Model) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let image = diorama::still(cx.world, model);
        let node = Node {
            width: px(STILL_WIDTH),
            aspect_ratio: Some(ASPECT),
            ..Node::default()
        };
        picture(cx, image, node, None)
    })
}

/// The diorama of `model`, turning, as wide as there is room for.
fn turning(model: Model) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let image = diorama::turning(cx.world);
        let node = Node {
            width: percent(100.0),
            max_width: px(TURNING_WIDTH),
            aspect_ratio: Some(ASPECT),
            ..Node::default()
        };
        picture(cx, image, node, Some(model))
    })
}

/// One hamlet of the list, as it is drawn.
#[derive(Clone, PartialEq)]
struct Row {
    id: String,
    title: String,
    kind: String,
    model: Model,
    price: String,
    agents: String,
    mark: Option<&'static str>,
}

fn rows_now(world: &World) -> (Vec<Row>, String) {
    let holdings = world.resource::<Holdings>();
    let market = world.resource::<Market>();
    let rows = market
        .lots(Kind::Hamlet)
        .iter()
        .map(|lot| {
            let listing = Listing::of(lot);
            Row {
                id: listing.id.clone(),
                title: listing.title.clone(),
                kind: listing.kind.clone(),
                model: model(&listing),
                price: listing.price.words(),
                agents: format!(
                    "{} agents",
                    listing.agents.len() + 1
                ),
                mark: super::artisans::mark(market, &listing.id),
            }
        })
        .collect();
    let held = match holdings.full() {
        true => full(holdings),
        false => String::new(),
    };
    (rows, held)
}

/// A hamlet's row, which opens its page.
fn shown_row(shown: &Row) -> AnyView {
    let id = shown.id.clone();
    let mut top = vec![
        row((label(shown.title.clone()).bold(true),))
            .grow(1.0)
            .min_width(px(0.0))
            .boxed(),
    ];
    if let Some(mark) = shown.mark {
        top.push(
            stated(Glyph::Check, mark, Tone::Accent, 11.0).boxed(),
        );
    }
    button(
        row((
            still(shown.model),
            column((
                row(top).gap(10.0).align(AlignItems::Center),
                note(shown.kind.clone()),
                label(format!("{}  {}", shown.price, shown.agents))
                    .size(12.0)
                    .tone(Tone::Faint),
            ))
            .gap(3.0)
            .grow(1.0)
            .min_width(px(0.0)),
        ))
        .gap(12.0)
        .align(AlignItems::Center)
        .grow(1.0)
        .min_width(px(0.0)),
    )
    .padding(UiRect::axes(px(8.0), px(8.0)))
    .rules(plate)
    .on_activate(move |world| {
        world.resource_mut::<Store>().open = Some(id.clone());
        trade::ask(world, &id);
    })
    .boxed()
}

/// Every hamlet on the shelves.
fn list() -> AnyView {
    let now = derived(rows_now)
        .when(either(moved::<Holdings>(), moved::<Market>()));
    let body = keyed(now, |(rows, held)| {
        let mut parts = Vec::new();
        if !held.is_empty() {
            parts.push(note(held.clone()).boxed());
        }
        parts.push(
            column(rows.iter().map(shown_row).collect::<Vec<_>>())
                .gap(2.0)
                .boxed(),
        );
        column(parts).gap(10.0).boxed()
    })
    .within(bare());
    column((trade::shelves(Kind::Hamlet), body))
        .gap(12.0)
        .boxed()
}

/// The way to buy `listing`, or to go to it once it stands in the
/// realm; or why the realm has no room for it.
fn deal_view(listing: &Listing) -> AnyView {
    let (id, name) = (listing.id.clone(), listing.name.clone());
    let read = name.clone();
    let now = derived(move |world: &World| {
        let holdings = world.resource::<Holdings>();
        let market = world.resource::<Market>();
        let kept = market.kept_version(&id).is_some();
        let full = (!kept && holdings.full()).then(|| full(holdings));
        (holdings.owns(&read).filter(|_| kept), full)
    })
    .when(either(moved::<Holdings>(), moved::<Market>()));
    let deal = trade::deal(
        Kind::Hamlet,
        listing.id.clone(),
        listing.price,
        &listing.seller,
    );
    let around = keyed(now, move |(owned, full)| {
        let mut parts = Vec::new();
        if let Some(hamlet) = *owned {
            parts.push(
                buttons(vec![action(
                    Glyph::Aim,
                    "GO TO IT",
                    move |world| visit(world, hamlet),
                )])
                .boxed(),
            );
        }
        if let Some(full) = full {
            parts.push(
                label(full.clone())
                    .size(12.0)
                    .tone(Tone::Critical)
                    .boxed(),
            );
        }
        column(parts).gap(6.0).boxed()
    })
    .within(bare());
    column((deal, around)).gap(8.0).boxed()
}

/// One who comes with the hamlet: its name and what it does.
fn settler(settler: &Settler) -> AnyView {
    let mut knows = Vec::new();
    if !settler.skills.is_empty() {
        knows.push(format!("skills: {}", settler.skills.join(", ")));
    }
    if settler.notes {
        knows.push("with its notes".to_string());
    }
    let mut parts = vec![
        label(settler.name.clone()).size(12.0).bold(true).boxed(),
        note(settler.purpose.clone()).boxed(),
    ];
    if !knows.is_empty() {
        parts.push(note(knows.join("; ")).size(11.0).boxed());
    }
    column(parts).gap(1.0).boxed()
}

/// The hamlet of the listing `id` on a page of its own: its town
/// turning, what it is, who comes with it and what they know, and
/// the way to buy it.
fn hamlet(id: &str) -> AnyView {
    let back = page_head(
        "HAMLETS",
        |world| world.resource_mut::<Store>().open = None,
        Glyph::Hamlet,
        "HAMLET",
    );
    let read = id.to_string();
    let found = derived(move |world: &World| {
        let market = world.resource::<Market>();
        market.lot(&read).map(Listing::of)
    })
    .when(moved::<Market>());
    let body = keyed(found, |listing| {
        let Some(listing) = listing else {
            return note("This hamlet is no longer on the shelves.")
                .boxed();
        };
        let mut parts = vec![
            column((
                title(listing.title.clone()),
                note(format!(
                    "{}  {}",
                    standing_words(&listing.standing),
                    seller_words(&listing.seller)
                )),
            ))
            .gap(4.0)
            .boxed(),
            turning(model(listing)),
            note(price_words(listing.price)).boxed(),
            deal_view(listing),
            trade::cover(listing.cover.clone()),
            label(listing.summary.clone()).size(13.0).boxed(),
        ];
        if !listing.description.trim().is_empty() {
            parts.push(
                label(listing.description.clone())
                    .size(12.0)
                    .tone(Tone::Dim)
                    .boxed(),
            );
        }
        let mut team = vec![
            label("Comes with")
                .size(12.0)
                .bold(true)
                .tone(Tone::Faint)
                .boxed(),
            settler(&listing.mayor),
        ];
        team.extend(listing.agents.iter().map(settler));
        parts.push(column(team).gap(8.0).boxed());
        parts.push(
            note(format!("Version {}", listing.version))
                .size(11.0)
                .boxed(),
        );
        column(parts).gap(14.0).boxed()
    })
    .within(bare());
    column((back, body)).gap(16.0).boxed()
}

/// What the stall sells: the hamlets on its shelves, or one of them
/// on its own page.
pub(super) fn page() -> AnyView {
    AnyView::new(|cx: &mut Cx| {
        cx.world.init_resource::<Store>();
        cx.world.init_resource::<Holdings>();
        cx.world.init_resource::<Market>();
        let open = resource::<Store, _>(|store| store.open.clone());
        cx.build(
            keyed(open, |open| match open {
                Some(id) => hamlet(id),
                None => list(),
            })
            .within(bare()),
        )
    })
}

#[cfg(test)]
mod tests {
    use bevy_fynix::dock::DockTree;
    use hamlet_core::agents::{self, AgentsPlugin};
    use hamlet_core::core::HomeDir;
    use hamlet_model::market::package::{self, Agent};
    use hamlet_rules::market::Keeping;
    use hamlet_rules::realm::Realm;
    use hamlet_testing::Scratch;
    use hamlet_world::pick::{Also, Selected};

    use super::*;

    fn agent(name: &str) -> Agent {
        Agent {
            name: name.into(),
            purpose: "Works.".into(),
            model: None,
            files: Vec::new(),
        }
    }

    #[test]
    fn a_hamlet_bought_stands_selected_and_a_full_realm_says_why() {
        let dir = Scratch::new("ui-store");
        std::fs::write(
            hamlet_home::limits::path(&dir),
            "[hamlets]\nmost = 2\n",
        )
        .unwrap();
        let mut app = App::new();
        app.add_plugins(AgentsPlugin {
            home: dir.to_path_buf(),
        })
        .insert_resource(HomeDir(hamlet_home::Home {
            root: dir.to_path_buf(),
            workspace: dir.join("workspace"),
        }))
        .init_resource::<Store>()
        .init_resource::<Holdings>()
        .init_resource::<Market>()
        .init_resource::<Time>()
        .init_resource::<Selected>()
        .init_resource::<Also>()
        .init_resource::<DockTree>();
        let world = app.world_mut();
        world.run_system_cached(count).unwrap();
        assert!(!world.resource::<Holdings>().full());

        // As the core leaves it once a hamlet bought is founded.
        let bought = package::Hamlet {
            name: "lamplight".into(),
            title: "Lamplight Press".into(),
            kind: "Press".into(),
            purpose: "Makes books.".into(),
            seed: "1".into(),
            mayor: agent("publisher"),
            agents: vec![agent("editor")],
        };
        Realm::new(dir.to_path_buf())
            .found_bought(&bought, "alice")
            .unwrap();
        agents::sync(world);
        let mut market = world.resource_mut::<Market>();
        market.kept.0.insert(
            "l_lamplight".into(),
            Keeping {
                kind: Kind::Hamlet,
                title: "Lamplight Press".into(),
                version: "1.0.0".into(),
                names: vec!["lamplight".into()],
                hamlet: None,
                house: None,
            },
        );
        market.deal = Some((
            "l_lamplight".into(),
            Deal::Done("Founded.".into()),
        ));
        world.run_system_cached(arrived).unwrap();
        world.run_system_cached(count).unwrap();
        let hamlet =
            world.resource::<Holdings>().owns("lamplight").unwrap();
        assert_eq!(
            world.resource::<Selected>().0,
            Some(Thing::Common(hamlet))
        );
        assert!(world.resource::<Store>().seeking.is_some());

        // The realm holds as many as the limits allow.
        let holdings = world.resource::<Holdings>();
        assert!(holdings.full());
        assert!(full(holdings).contains("2 of 2"));
    }
}
