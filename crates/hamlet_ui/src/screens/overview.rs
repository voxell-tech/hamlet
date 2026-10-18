//! The Inspector with no agent selected: every hamlet, to search and
//! to open, and one hamlet with everyone in it.

mod hamlet;
mod record;
mod rows;

use bevy::app::{App, Startup, Update};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::Children;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Query, ResMut};
use bevy::ecs::world::World;
use bevy::text::EditableText;
use bevy::ui::{AlignItems, UiRect, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, label, row, scroll, text_field,
};
use bevy_fynix::{ScopedExt, ViewExt, derived, keyed};
use hamlet_core::agents::{Activity, LivesIn, Settlement, Treasury};
use hamlet_core::core::HomeDir;
use hamlet_core::oversight::Waiting;
use hamlet_realm::pick::{Thing, focus_on, select};
use hamlet_rules::realm::Realm;
pub(crate) use hamlet_sdk::cards::heading;

pub use self::hamlet::{Naming, Page, hamlet};
use self::rows::{hamlet_rows, pressable};
pub(crate) use self::rows::{stale, standing};
use crate::icons::Glyph;
use crate::looks::{
    Fills, action, bad_action, bare, good_action, note, title, well,
};
use crate::platform::DisplayText;
use crate::roll::hamlets_moved;
use crate::world_menu::Deed;
use crate::{AnyView, chat, tabs, world_menu};

pub fn plugin(app: &mut App) {
    app.init_resource::<Search>()
        .init_resource::<Naming>()
        .init_resource::<Page>()
        .add_systems(Startup, register)
        .add_systems(Update, (read_search, tabs::mark::<Page>));
}

/// The words typed in the search over the hamlets.
#[derive(Resource, Default, PartialEq)]
pub struct Search(String);

/// The search field's root, to find its editable child.
#[derive(Component)]
pub struct SearchField;

/// Registers the components these views read, so that a shared
/// reference can query them before anything has them.
pub fn register(world: &mut World) {
    hamlet_core::agents::register(world);
    world.register_component::<Settlement>();
    world.register_component::<LivesIn>();
    world.register_component::<Activity>();
    world.register_component::<Treasury>();
    world.register_component::<Waiting>();
}

/// Follows the search as it is typed.
pub fn read_search(
    root: Query<&Children, With<SearchField>>,
    texts: Query<&EditableText>,
    mut search: ResMut<Search>,
) {
    // With no field the search stays as it is.
    let Some(text) = root
        .iter()
        .flat_map(|children| children.iter().copied())
        .find_map(|child| texts.get(child).ok())
    else {
        return;
    };
    if text.value() != search.0.as_str() {
        search.0 = text.value().to_string();
    }
}

/// The hamlets that were closed down, each to be brought back.
fn closed() -> AnyView {
    let names = derived(|world: &World| {
        let home = &world.resource::<HomeDir>().0.root;
        // Each by the name commands know it by, and by what its
        // chief called it.
        Realm::open(home)
            .map(|realm| realm.closed())
            .unwrap_or_default()
            .into_iter()
            .map(|place| {
                let title =
                    place.title.unwrap_or_else(|| place.name.clone());
                (place.name, title)
            })
            .collect::<Vec<_>>()
    })
    .when(hamlets_moved());
    keyed(names, |names| {
        if names.is_empty() {
            return column(()).boxed();
        }
        let mut parts = vec![heading(
            Glyph::Gone,
            format!("CLOSED DOWN ({})", names.len()),
        )];
        parts.extend(names.iter().map(|(name, title)| {
            let back = format!("/restore-hamlet {name}");
            let gone = name.clone();
            row((
                row((label(title.to_uppercase())
                    .size(13.0)
                    .bold(true)
                    .tagged(DisplayText),))
                .fills_width(),
                good_action(
                    Glyph::Restore,
                    "RESTORE",
                    move |world| chat::send(world, back.clone()),
                ),
                bad_action(Glyph::Delete, "DELETE", move |world| {
                    world_menu::ask(
                        world,
                        Deed::EraseHamlet(gone.clone()),
                    )
                }),
            ))
            .gap(12.0)
            .align(AlignItems::Center)
            .boxed()
        }));
        column(parts)
            .gap(6.0)
            .padding(UiRect::top(px(10.0)))
            .boxed()
    })
    .within(bare())
    .boxed()
}

/// Every hamlet, with a search over them.
pub fn overview() -> AnyView {
    let shown = derived(|world: &World| {
        let search =
            world.resource::<Search>().0.trim().to_lowercase();
        hamlet_rows(world)
            .into_iter()
            .filter(|row| {
                search.is_empty()
                    || row.title.to_lowercase().contains(&search)
                    || row.id.contains(&search)
                    || row.mayor.contains(&search)
            })
            .collect::<Vec<_>>()
    })
    .when(stale());
    let list = keyed(shown, |rows| {
        if rows.is_empty() {
            return label("No hamlet by that name.")
                .tone(Tone::Dim)
                .boxed();
        }
        let cards = rows
            .iter()
            .map(|row| {
                let hamlet = row.hamlet;
                let mut lines = vec![
                    format!("Mayor: {}", row.mayor),
                    row.account.clone(),
                ];
                if row.gone > 0 {
                    lines.push(format!("{} gone", row.gone));
                }
                pressable(
                    Thing::Common(hamlet),
                    row.title.to_uppercase(),
                    row.state(),
                    lines,
                    move |world| {
                        let thing = Thing::Common(hamlet);
                        select(world, Some(thing));
                        focus_on(world, thing, false);
                    },
                )
            })
            .collect::<Vec<_>>();
        column(cards).gap(4.0).boxed()
    })
    .within(bare());
    column((
        row((
            title("HAMLETS"),
            note("Click a hamlet to see who lives there."),
        ))
        .gap(12.0)
        .align(AlignItems::Center),
        row((
            text_field(String::new(), |_, _| {})
                .grow(1.0)
                .rules(well)
                .tagged(SearchField),
            action(
                Glyph::Plus,
                "FOUND A HAMLET",
                world_menu::ask_found,
            ),
        ))
        .gap(8.0)
        .align(AlignItems::Center),
        label("Search by a hamlet's name or its mayor's.")
            .size(11.0)
            .tone(Tone::Dim),
        scroll((list, closed())).fills_height(),
        row((
            action(
                Glyph::Hearth,
                "YOUR HEARTH",
                crate::chief::go_home,
            ),
            note("Where you live, and how you look."),
        ))
        .gap(10.0)
        .align(AlignItems::Center),
    ))
    .fills_height()
    .gap(8.0)
    .boxed()
}

/// Gives a chief's command to `hamlet`, whoever the chat is with.
pub(crate) fn tell(world: &mut World, hamlet: Entity, text: String) {
    let Some(name) = world
        .get::<Settlement>(hamlet)
        .map(|settlement| settlement.name.clone())
    else {
        return;
    };
    chat::decree(world, &name, text);
}
