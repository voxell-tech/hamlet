//! The folders a hamlet works on, on the hamlet's page.

use std::path::Path;

use bevy::app::App;
use bevy::ecs::entity::Entity;
use bevy::ecs::observer::On;
use bevy::ecs::system::Commands;
use bevy::ecs::world::World;
use bevy::ui::{AlignItems, UiRect, percent, px};
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, column, frame, label, row,
};
use bevy_fynix::{ScopedExt, ViewExt, derived, keyed};
use hamlet_client::realm::Works;
use hamlet_core::pick::{Switched, Thing, select};
use hamlet_core::roll::hamlets_moved;
use hamlet_desktop::folder::{ChooseFolder, FolderChosen};
use hamlet_ui::styles::{
    ICON, Looks, action, bare, buttons, description,
    description_rules, heading_line, lettered, link_row, note,
};
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::{AnyView, View, with_cx};

use crate::overview::tell;

/// The most earlier folders offered again.
const EARLIER: usize = 3;

pub(crate) fn plugin(app: &mut App) {
    app.add_observer(switched).add_observer(chosen);
}

/// Shows the hamlet's page, where its folders and permissions are.
fn switched(switched: On<Switched>, mut commands: Commands) {
    let hamlet = switched.hamlet;
    commands.queue(move |world: &mut World| {
        select(world, Some(Thing::Common(hamlet)));
    });
}

fn chosen(chosen: On<FolderChosen>, mut commands: Commands) {
    let FolderChosen { asker, folder } = chosen.event().clone();
    commands.queue(move |world: &mut World| {
        work_on(world, asker, &folder);
    });
}

fn work_on(world: &mut World, hamlet: Entity, folder: &Path) {
    tell(world, hamlet, format!("/folder add {}", folder.display()));
}

/// `part` of what `hamlet` works on, rebuilt when it changes.
fn of_works(
    hamlet: Entity,
    part: fn(Entity, &Works) -> AnyView,
) -> impl View {
    let work = derived(move |world: &World| {
        world.get::<Works>(hamlet).cloned()
    })
    .when(hamlets_moved());
    keyed(work, move |work| match work {
        Some(work) => part(hamlet, work),
        None => column(()).boxed(),
    })
    .within(bare())
}

/// The folders `hamlet` works on and the ones it worked on before.
pub(crate) fn section(hamlet: Entity) -> impl View {
    of_works(hamlet, works_on)
}

fn works_on(hamlet: Entity, work: &Works) -> AnyView {
    let choose = action(
        Glyph::Folder,
        if work.folders.is_empty() {
            "Choose a Folder"
        } else {
            "Add a Folder"
        },
        move |world| {
            world.trigger(ChooseFolder {
                asker: hamlet,
                title: "Choose a folder this hamlet works on".into(),
            });
        },
    )
    .tooltip(|| note("A path on the server's machine"))
    .boxed();
    let now = if work.folders.is_empty() {
        description("No folder selected.").boxed()
    } else {
        column(
            work.folders
                .iter()
                .map(|folder| held_row(hamlet, folder.clone()))
                .collect::<Vec<_>>(),
        )
        .gap(0.0)
        .width(percent(100.0))
        .boxed()
    };
    let earlier = work
        .before
        .iter()
        .take(EARLIER)
        .map(|folder| earlier_row(hamlet, folder.clone()))
        .collect::<Vec<_>>();
    let more = if earlier.is_empty() {
        Vec::new()
    } else {
        vec![column(earlier).gap(0.0).width(percent(100.0)).boxed()]
    };
    bylaw(
        Glyph::Folder,
        "Works on",
        buttons(vec![choose]).boxed(),
        now,
        more,
    )
}

/// A folder the hamlet works on, with the way to stop.
fn held_row(hamlet: Entity, folder: String) -> AnyView {
    let dropped = folder.clone();
    row((
        glyph(Glyph::Folder, ICON),
        label(folder).rules(description_rules),
        frame().grow(1.0),
        lettered("Drop")
            .variant(Variant::Default)
            .on_activate(move |world| {
                tell(
                    world,
                    hamlet,
                    format!("/folder drop {dropped}"),
                );
            })
            .tooltip(|| note("Stop working on this folder")),
    ))
    .gap(8.0)
    .grow(1.0)
    .align(AlignItems::Center)
    .boxed()
}

/// A bylaw's name and `control`, what it is now, then `more`.
fn bylaw(
    which: Glyph,
    name: &'static str,
    control: AnyView,
    now: AnyView,
    more: Vec<AnyView>,
) -> AnyView {
    with_cx(move |cx| {
        let space = &cx.theme().space;
        let named = column((heading_line(which, name, control), now))
            .gap(space.gap / 2.0);
        let parts = std::iter::once(named.boxed())
            .chain(more)
            .collect::<Vec<_>>();
        column(parts).gap(space.gap)
    })
    .boxed()
}

/// A folder worked on before, pressed to work on it again.
fn earlier_row(hamlet: Entity, folder: String) -> AnyView {
    bevy_fynix::views::button(
        row((
            glyph(Glyph::History, ICON),
            label(folder.clone()).rules(description_rules),
            frame().grow(1.0),
            label("Use").wrap(false),
        ))
        .gap(8.0)
        .grow(1.0)
        .align(AlignItems::Center),
    )
    .padding(UiRect::axes(px(4.0), px(4.0)))
    .rules(link_row)
    .tagged(Variant::Text)
    .on_activate(move |world| {
        work_on(world, hamlet, Path::new(&folder));
    })
    .boxed()
}
