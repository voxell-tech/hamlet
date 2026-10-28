//! The seller's desk, in place of a stall's page: the chief's
//! listings, a new listing, and one listing's page with its versions.
//! Below [`NARROW`] the pages stack what they lay side by side.

mod actions;
mod description;
mod details;
mod files;
mod form;
mod items;
mod list;
mod listing;
mod look;
mod new_listing;
mod tag_field;
mod tags;

use bevy::app::{App, Update};
use bevy::ecs::change_detection::DetectChangesMut;
use bevy::ecs::component::Component;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Query, ResMut};
use bevy::ecs::world::World;
use bevy::ui::ComputedNode;
use bevy_fynix::views::BehaviorExt;
use bevy_fynix::{ViewExt, derived, keyed};
use hamlet_ui::AnyView;
use hamlet_ui::styles::filling;

pub(crate) use self::form::Form;
use crate::bazaar::covers::Pick;
use crate::bazaar::market::drafts::{self, Which};
use crate::bazaar::market::lot::{Kind, Lot};
use crate::bazaar::market::sell::Step;
use crate::bazaar::market::versions::{
    Version, before, newest_first,
};
use crate::bazaar::market::{Market, Selling, ask_mine, read_terms};
use crate::roll::{either, moved};

/// The width, in the interface's pixels, below which the desk stacks
/// what it lays side by side.
const NARROW: f32 = 560.0;

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Desk>()
        .init_resource::<Room>()
        .add_systems(
            Update,
            (
                measure,
                follow,
                tag_field::typing,
                description::shape,
                new_listing::searching,
            ),
        );
}

/// What the chief is doing at the seller's desk.
#[derive(Resource, Clone, PartialEq, Debug, Default)]
pub(crate) struct Desk {
    /// The desk's page open in place of the stall's, if one is.
    pub page: Option<Page>,
    pub new: Choosing,
    /// The version chosen on a listing's page; its newest when
    /// `None`.
    pub version: Option<String>,
    /// The details of the draft chosen, as they are typed.
    pub form: Option<Form>,
    /// The files opened or shut against how they start, by listing,
    /// version and path.
    pub flipped: Vec<(String, String, String)>,
    /// A sale started here, to be followed to its listing.
    pub selling: bool,
    /// The cover's picture is being chosen.
    pub covering: bool,
    /// Why what was asked could not be done.
    pub problem: Option<String>,
}

/// A page of the desk.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) enum Page {
    List,
    New,
    /// The listing of this id.
    Listing(String),
}

/// What a new listing sells, as it is chosen.
#[derive(Clone, PartialEq, Debug, Default)]
pub(crate) struct Choosing {
    pub kind: Option<Kind>,
    pub search: String,
    /// The name of the agent, or the id of the hamlet, chosen.
    pub picked: Option<String>,
}

/// Whether the desk is narrow.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) struct Room {
    pub narrow: bool,
}

/// The node the desk's width is read from.
#[derive(Component)]
struct Measured;

/// Opens the chief's listings.
pub(crate) fn open_list(world: &mut World) {
    let mut desk = world.resource_mut::<Desk>();
    desk.page = Some(Page::List);
    desk.problem = None;
    ask_mine(world);
    drafts::ask_fees(world);
}

/// Opens a new listing, of `kind` if one is given, after the seller
/// terms the first time.
pub(crate) fn open_new(world: &mut World, kind: Option<Kind>) {
    let mut desk = world.resource_mut::<Desk>();
    desk.page = Some(Page::New);
    desk.new = Choosing {
        kind: kind.filter(|kind| *kind != Kind::Plugin),
        ..Choosing::default()
    };
    desk.problem = None;
    drafts::ask_fees(world);
    let agreed = world
        .resource::<Market>()
        .user()
        .is_some_and(|user| user.seller_terms.is_some());
    if !agreed {
        read_terms(world);
    }
}

/// Opens the page of the chief's listing `id` at its newest version.
pub(crate) fn open_listing(world: &mut World, id: &str) {
    open_version(world, id, None);
}

fn open_version(
    world: &mut World,
    id: &str,
    version: Option<String>,
) {
    let mut desk = world.resource_mut::<Desk>();
    desk.page = Some(Page::Listing(id.to_string()));
    desk.version = version;
    desk.form = None;
    desk.covering = false;
    desk.problem = None;
    *world.resource_mut::<Pick>() = Pick::default();
    world.resource_mut::<drafts::Drafts>().said = None;
    drafts::ask_fees(world);
}

/// Chooses `version` of the listing open.
pub(crate) fn choose(world: &mut World, version: &str) {
    let Some(Page::Listing(id)) =
        world.resource::<Desk>().page.clone()
    else {
        return;
    };
    open_version(world, &id, Some(version.to_string()));
}

pub(crate) fn close(world: &mut World) {
    world.resource_mut::<Desk>().page = None;
}

/// The listing open and its version chosen, as the world stands.
pub(crate) fn chosen(world: &World) -> Option<(Lot, Version)> {
    let desk = world.resource::<Desk>();
    let Some(Page::Listing(id)) = &desk.page else {
        return None;
    };
    let lot = mine(world, id)?;
    let all = newest_first(&lot);
    let version = desk
        .version
        .as_ref()
        .and_then(|version| {
            all.iter().find(|one| one.version == *version)
        })
        .or(all.first())?
        .clone();
    Some((lot, version))
}

/// The chief's listing `id`, as they last came.
pub(crate) fn mine(world: &World, id: &str) -> Option<Lot> {
    world
        .resource::<Market>()
        .mine
        .iter()
        .find(|lot| lot.id == id)
        .cloned()
}

/// The desk's page open, if one is, else the stall's page `stall`
/// makes. `back` names the stall on the way back to it.
pub(crate) fn or(
    back: &'static str,
    stall: impl Fn() -> AnyView + Send + Sync + 'static,
) -> AnyView {
    let page = derived(|world: &World| {
        let terms = world.resource::<Market>().terms.is_some();
        let page = world.resource::<Desk>().page.clone();
        (page.map(|page| std::mem::discriminant(&page)), terms)
    })
    .when(either(moved::<Desk>(), moved::<Market>()));
    keyed(page, move |(page, terms)| match (page, terms) {
        (_, true) => list::terms(back),
        (Some(_), false) => pages(back),
        (None, false) => stall(),
    })
    .within(filling().tagged(Measured))
    .boxed()
}

fn pages(back: &'static str) -> AnyView {
    let page = derived(|world: &World| {
        world.resource::<Desk>().page.clone()
    })
    .when(moved::<Desk>());
    keyed(page, move |page| match page {
        Some(Page::New) => new_listing::page(),
        Some(Page::Listing(id)) => listing::page(id.clone()),
        _ => list::page(back),
    })
    .within(filling())
    .boxed()
}

/// Reads how wide the desk is.
fn measure(
    nodes: Query<&ComputedNode, With<Measured>>,
    mut room: ResMut<Room>,
) {
    let Some(node) = nodes.iter().next() else {
        return;
    };
    let wide = node.size().x * node.inverse_scale_factor();
    if wide > 0.0 {
        room.set_if_neq(Room {
            narrow: wide < NARROW,
        });
    }
}

/// Keeps the listing's page in step with the world: the draft's form
/// filled from its details, its files asked for, and a sale started
/// here followed to its listing once it is uploaded.
fn follow(world: &mut World) {
    if world.resource::<Desk>().selling {
        let step = world
            .resource::<Selling>()
            .0
            .as_ref()
            .map(|sale| sale.step.clone());
        if let Some(Step::Done(id, version)) = step {
            world.resource_mut::<Desk>().selling = false;
            open_version(world, &id, Some(version));
        }
    }
    let Some((lot, version)) = chosen(world) else {
        return;
    };
    let which = Which {
        id: lot.id.clone(),
        version: version.version.clone(),
    };
    let against =
        before(&lot, &which.version).map(|one| one.version.clone());
    drafts::ask_files(
        world,
        &which.id,
        &which.version,
        against.as_deref(),
    );
    let desk = world.resource::<Desk>();
    let stale =
        desk.form.as_ref().is_none_or(|form| form.which != which);
    if version.state == crate::bazaar::market::versions::State::Draft
        && stale
    {
        world.resource_mut::<Desk>().form =
            Some(Form::of(which, &version.details));
    }
}
