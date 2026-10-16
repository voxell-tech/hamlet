//! The Hamlet Store's wares: whole hamlets, each shown as a diorama
//! of its town, and bought to stand in the realm at once with its
//! mayor, its agents and all they know.

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
use hamlet_core::core::HomeDir;
use hamlet_home::limits::Hamlets;
use hamlet_model::market::hamlets::{self, Listing, Settler};
use hamlet_model::market::{Price, Seller, Standing};
use hamlet_rules::hamlet::HOME;
use hamlet_rules::realm::Realm;
use hamlet_world::diorama::{self, Dioramas, Model};
use hamlet_world::pick::{Thing, focus_on, select};

use super::shelf::small;
use crate::icons::Glyph;
use crate::looks::{
    action, bare, buttons, good_action, note, page_head, plate,
    stated, title,
};
use crate::roll::{either, moved};
use crate::{AnyView, Cx, Me, Roll, markdown};

/// Seconds the payment takes to go through, so that the chief sees
/// it go.
const PAYING: f32 = 0.9;

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
                settle.run_if(|store: Res<Store>| {
                    store.buying.is_some()
                }),
                seek.run_if(|store: Res<Store>| {
                    store.seeking.is_some()
                }),
            ),
        );
}

/// What the chief is doing at the Hamlet Store.
#[derive(Resource, Default, Clone, PartialEq, Debug)]
pub struct Store {
    /// The hamlet open on a page of its own, by its name.
    pub open: Option<String>,
    /// The hamlet being paid for, and the time the chief bought it.
    buying: Option<(String, f32)>,
    /// A hamlet just bought, whose town the camera waits to show
    /// until the time given.
    seeking: Option<(Entity, f32)>,
    /// What came of the last purchase, and whether it went wrong.
    said: Option<(String, bool)>,
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

fn say(world: &mut World, words: impl Into<String>, wrong: bool) {
    world.resource_mut::<Store>().said = Some((words.into(), wrong));
}

fn now(world: &World) -> f32 {
    world.resource::<Time>().elapsed_secs()
}

/// Pays for the hamlet `name`, which is founded once the payment
/// has gone through.
fn buy(world: &mut World, name: &str) {
    let holdings = world.resource::<Holdings>();
    let paying = world.resource::<Store>().buying.is_some();
    if paying || holdings.owns(name).is_some() {
        return;
    }
    if holdings.full() {
        let words = full(holdings);
        return say(world, words, true);
    }
    let Some(listing) = hamlets::find(name) else {
        return say(
            world,
            "That hamlet is no longer for sale.",
            true,
        );
    };
    // A free hamlet has nothing to pay.
    let paid = match listing.price {
        Price::Free => now(world) - PAYING,
        _ => now(world),
    };
    let mut store = world.resource_mut::<Store>();
    store.buying = Some((name.to_string(), paid));
    store.said = None;
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

/// Founds the hamlet being paid for once its payment has gone
/// through.
fn settle(world: &mut World) {
    let Some((name, paid)) = world.resource::<Store>().buying.clone()
    else {
        return;
    };
    if now(world) - paid < PAYING {
        world.write_message(RequestRedraw);
        return;
    }
    world.resource_mut::<Store>().buying = None;
    found(world, &name);
}

/// Founds the hamlet `name` with all it comes with, selects it and
/// brings its town into view.
fn found(world: &mut World, name: &str) {
    let Some(bought) = hamlets::find(name) else {
        return say(
            world,
            "That hamlet is no longer for sale.",
            true,
        );
    };
    let home = world.resource::<HomeDir>().0.root.clone();
    let user = world.resource::<Me>().user.clone();
    if let Err(why) = Realm::new(home).found_bought(&bought, &user) {
        let words = format!(
            "{} could not be founded: {why:#}.",
            bought.title
        );
        return say(world, words, true);
    }
    hamlet_core::agents::sync(world);
    // The merchant in the world cheers the sale.
    world.trigger(hamlet_world::bazaar::Traded);
    let hamlet = world
        .query::<(Entity, &Settlement)>()
        .iter(world)
        .find(|(_, settlement)| settlement.name == bought.name)
        .map(|(hamlet, _)| hamlet);
    if let Some(hamlet) = hamlet {
        select(world, Some(Thing::Common(hamlet)));
        let until = now(world) + SEEK;
        world.resource_mut::<Store>().seeking = Some((hamlet, until));
        crate::dock::show_tab(world, "world");
    }
    let words = format!(
        "{} stands in your realm, chief. {} keeps it, with {} and \
         all they know.",
        bought.title,
        bought.mayor.name,
        agents(bought.agents.len()),
    );
    say(world, words, false);
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

fn agents(count: usize) -> String {
    match count {
        1 => "one agent".into(),
        count => format!("{count} agents"),
    }
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
        Price::Monthly(_) => price.words(),
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

/// What came of the last purchase.
fn said() -> AnyView {
    keyed(resource::<Store, _>(|store| store.said.clone()), |said| {
        match said {
            Some((words, wrong)) => {
                let tone = if *wrong {
                    Tone::Critical
                } else {
                    Tone::Accent
                };
                label(words.clone()).size(12.0).tone(tone).boxed()
            }
            None => bare().boxed(),
        }
    })
    .within(bare())
    .boxed()
}

/// One hamlet of the list, as it is drawn.
#[derive(Clone, PartialEq)]
struct Row {
    name: String,
    title: String,
    kind: String,
    model: Model,
    seller: String,
    price: String,
    standing: String,
    agents: String,
    owned: bool,
}

fn rows_now(world: &World) -> (Vec<Row>, String) {
    let holdings = world.resource::<Holdings>();
    let rows = hamlets::for_sale()
        .iter()
        .map(|listing| Row {
            name: listing.name.clone(),
            title: listing.title.clone(),
            kind: listing.kind.clone(),
            model: model(listing),
            seller: seller_words(&listing.seller),
            price: listing.price.words(),
            standing: format!(
                "{:.1} of 5 · {} own it",
                listing.standing.stars,
                thousands(listing.standing.owners)
            ),
            agents: format!(
                "A mayor and {}",
                agents(listing.agents.len())
            ),
            owned: holdings.owns(&listing.name).is_some(),
        })
        .collect();
    let held = format!(
        "Your realm holds {} of the {} hamlets your limits allow.",
        holdings.hamlets.len(),
        holdings.most
    );
    (rows, held)
}

/// A hamlet's row, which opens its page.
fn shown_row(shown: &Row) -> AnyView {
    let name = shown.name.clone();
    let mut top = vec![
        row((label(shown.title.clone()).bold(true),))
            .grow(1.0)
            .min_width(px(0.0))
            .boxed(),
    ];
    if shown.owned {
        top.push(
            stated(Glyph::Check, "YOURS", Tone::Accent, 11.0).boxed(),
        );
    }
    button(
        row((
            still(shown.model),
            column((
                row(top).gap(10.0).align(AlignItems::Center),
                label(format!("{} · {}", shown.price, shown.kind))
                    .size(12.0)
                    .tone(Tone::Faint),
                note(shown.agents.clone()),
                note(shown.seller.clone()),
                note(shown.standing.clone()),
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
        let mut store = world.resource_mut::<Store>();
        store.open = Some(name.clone());
        store.said = None;
    })
    .boxed()
}

/// Every hamlet on the shelves.
fn list() -> AnyView {
    let now = derived(rows_now)
        .when(either(moved::<Holdings>(), moved::<Store>()));
    let body = keyed(now, |(rows, held)| {
        let mut parts = vec![note(held.clone()).boxed()];
        parts.push(
            column(rows.iter().map(shown_row).collect::<Vec<_>>())
                .gap(2.0)
                .boxed(),
        );
        column(parts).gap(10.0).boxed()
    })
    .within(bare());
    column((
        note(
            "Whole hamlets, kept and ready. Buy one and it is \
             founded in your realm at once: its mayor, its agents, \
             their characters, notes and skills.",
        ),
        said(),
        body,
    ))
    .gap(12.0)
    .boxed()
}

/// The state of the hamlet `name` at the store, for its buttons.
#[derive(Clone, PartialEq)]
enum Deal {
    /// For sale, at this price; with the reason it cannot be bought
    /// when the realm is full.
    Open(Price, Option<String>),
    /// Being paid for to this seller, or founded when it is free.
    Paying(String),
    Founding,
    /// In the realm, as this hamlet.
    Owned(Entity),
}

fn deal(world: &World, listing: &Listing) -> Deal {
    let store = world.resource::<Store>();
    let holdings = world.resource::<Holdings>();
    if let Some(hamlet) = holdings.owns(&listing.name) {
        return Deal::Owned(hamlet);
    }
    match &store.buying {
        Some((name, _)) if *name == listing.name => {
            match listing.price {
                Price::Free => Deal::Founding,
                _ => Deal::Paying(listing.seller.name.clone()),
            }
        }
        _ => Deal::Open(
            listing.price,
            holdings.full().then(|| full(holdings)),
        ),
    }
}

/// The ways to buy `listing`, or to see it once it is the chief's.
fn deal_view(listing: &Listing) -> AnyView {
    let (name, title) = (listing.name.clone(), listing.title.clone());
    let read = listing.clone();
    let now = derived(move |world: &World| deal(world, &read))
        .when(either(moved::<Holdings>(), moved::<Store>()));
    keyed(now, move |deal| match deal {
        Deal::Open(price, full) => {
            let name = name.clone();
            let mut parts = vec![
                buttons(vec![
                    good_action(
                        Glyph::Coins,
                        "BUY AND FOUND",
                        move |world| buy(world, &name),
                    ),
                    label(price_words(*price)).size(13.0).boxed(),
                ])
                .boxed(),
            ];
            if let Some(full) = full {
                parts.push(
                    label(full.clone())
                        .size(12.0)
                        .tone(Tone::Critical)
                        .boxed(),
                );
            }
            column(parts).gap(6.0).boxed()
        }
        Deal::Paying(seller) => stated(
            Glyph::Coins,
            format!("Paying {seller}..."),
            Tone::Accent,
            13.0,
        )
        .boxed(),
        Deal::Founding => stated(
            Glyph::Hamlet,
            format!("Founding {title}..."),
            Tone::Accent,
            13.0,
        )
        .boxed(),
        Deal::Owned(hamlet) => {
            let hamlet = *hamlet;
            buttons(vec![
                stated(
                    Glyph::Check,
                    "IN YOUR REALM",
                    Tone::Accent,
                    12.0,
                )
                .boxed(),
                action(Glyph::Aim, "SEE IT", move |world| {
                    visit(world, hamlet)
                }),
            ])
            .boxed()
        }
    })
    .within(bare())
    .boxed()
}

/// One who comes with the hamlet: its name, what it is for, how it
/// is, and the skills it keeps.
fn settler(settler: &Settler) -> AnyView {
    let skills = settler
        .skills
        .iter()
        .map(|skill| skill.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let mut parts = vec![
        label(settler.name.clone()).size(13.0).bold(true).boxed(),
        label(settler.purpose.clone()).size(13.0).boxed(),
        note(settler.character.clone()).boxed(),
    ];
    if !skills.is_empty() {
        parts.push(
            label(format!("Skills: {skills}"))
                .size(12.0)
                .tone(Tone::Faint)
                .boxed(),
        );
    }
    column(parts).gap(2.0).boxed()
}

/// The hamlet `name` on a page of its own: its town turning, what it
/// is, who comes with it and what they know, and the way to buy it.
fn hamlet(name: &str) -> AnyView {
    let back = page_head(
        "HAMLET STORE",
        |world| world.resource_mut::<Store>().open = None,
        Glyph::Hamlet,
        "HAMLET",
    );
    let Some(listing) = hamlets::find(name) else {
        return column((
            back,
            note("This hamlet is no longer on the shelves."),
        ))
        .gap(16.0)
        .boxed();
    };
    let mut parts = vec![
        column((
            title(listing.title.clone()),
            note(format!(
                "{} · {}",
                listing.kind,
                seller_words(&listing.seller)
            )),
            note(standing_words(&listing.standing)),
        ))
        .gap(4.0)
        .boxed(),
        column((
            turning(model(&listing)),
            note(format!(
                "Its town, drawn small: {} in the hall by the \
                 common, and a house and a robot for each of its \
                 agents.",
                listing.mayor.name
            )),
        ))
        .gap(6.0)
        .boxed(),
        deal_view(&listing),
        said(),
        label(listing.description.clone()).size(13.0).boxed(),
        column((
            small("WHAT IT IS FOR"),
            label(listing.purpose.clone()).size(13.0),
        ))
        .gap(4.0)
        .boxed(),
        column((small("ITS MAYOR"), settler(&listing.mayor)))
            .gap(6.0)
            .boxed(),
    ];
    let mut agents = vec![small("ITS AGENTS").boxed()];
    agents.extend(listing.agents.iter().map(settler));
    parts.push(column(agents).gap(10.0).boxed());
    parts.push(
        column((
            small("FROM THE MAYOR'S NOTES"),
            markdown::fixed(sample(&listing.mayor.notes), Tone::Body),
        ))
        .gap(6.0)
        .boxed(),
    );
    column((back, column(parts).gap(14.0))).gap(16.0).boxed()
}

/// Notes without the heading they open with.
fn sample(notes: &str) -> &str {
    match notes.trim_start().strip_prefix('#') {
        Some(headed) => {
            headed.split_once('\n').map_or("", |(_, rest)| rest)
        }
        None => notes,
    }
}

/// What the stall sells: the hamlets on its shelves, or one of them
/// on its own page.
pub(super) fn page() -> AnyView {
    AnyView::new(|cx: &mut Cx| {
        cx.world.init_resource::<Store>();
        cx.world.init_resource::<Holdings>();
        let open = resource::<Store, _>(|store| store.open.clone());
        cx.build(
            keyed(open, |open| match open {
                Some(name) => hamlet(name),
                None => list(),
            })
            .within(bare()),
        )
    })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy_fynix::dock::DockTree;
    use hamlet_core::agents::{self, AgentsPlugin};
    use hamlet_testing::Scratch;
    use hamlet_world::pick::{Also, Selected};

    use super::*;

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
        .init_resource::<Time>()
        .init_resource::<Selected>()
        .init_resource::<Also>()
        .init_resource::<DockTree>();
        let world = app.world_mut();
        let mayor = agents::mayor(world).unwrap();
        world.insert_resource(Me::of(mayor, mayor, "alice", None));
        world.run_system_cached(count).unwrap();

        buy(world, "inkwell");
        // Nothing is founded until the payment has gone through.
        world.run_system_cached(settle).unwrap();
        assert_eq!(world.resource::<Holdings>().hamlets.len(), 1);
        world
            .resource_mut::<Time>()
            .advance_by(Duration::from_secs_f32(PAYING));
        world.run_system_cached(settle).unwrap();
        world.run_system_cached(count).unwrap();
        let inkwell =
            world.resource::<Holdings>().owns("inkwell").unwrap();
        assert_eq!(
            world.resource::<Selected>().0,
            Some(Thing::Common(inkwell))
        );
        let store = world.resource::<Store>();
        assert!(store.said.as_ref().is_some_and(|(_, wrong)| !wrong));
        assert!(store.seeking.is_some());
        assert!(agents::named(world, "line-editor").is_some());

        // The realm holds as many as the limits allow.
        buy(world, "bramble");
        let said = world.resource::<Store>().said.clone().unwrap();
        assert!(said.1 && said.0.contains("2 of 2"), "{}", said.0);
        assert!(world.resource::<Store>().buying.is_none());
    }
}
