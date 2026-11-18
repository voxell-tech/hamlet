//! The folder a hamlet works on and its mode, on the hamlet's page.

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
use hamlet_client::realm::{Mode, Works};
use hamlet_desktop::folder::{ChooseFolder, FolderChosen};
use hamlet_realm::pick::{Thing, select};
use hamlet_realm::works::Switched;
use hamlet_ui::styles::{
    ICON, Looks, action, bare, buttons, description,
    description_rules, lettered, link_row, note, segmented_words,
};
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::{AnyView, View, with_cx};

use crate::overview::{heading_line, tell};
use crate::roll::hamlets_moved;
use crate::words;

/// The most earlier folders offered again.
const EARLIER: usize = 3;

pub(crate) fn plugin(app: &mut App) {
    app.add_observer(switched).add_observer(chosen);
}

/// Switches a hamlet's mode, or shows its page to choose a folder.
fn switched(switched: On<Switched>, mut commands: Commands) {
    let hamlet = switched.hamlet;
    commands.queue(move |world: &mut World| {
        let Some(work) = world.get::<Works>(hamlet) else {
            return;
        };
        if work.folder.is_none() {
            select(world, Some(Thing::Common(hamlet)));
            return;
        }
        let to = match work.mode {
            Mode::Manual => Mode::Auto,
            Mode::Auto => Mode::Manual,
        };
        set_mode(world, hamlet, to);
    });
}

fn chosen(chosen: On<FolderChosen>, mut commands: Commands) {
    let FolderChosen { asker, folder } = chosen.event().clone();
    commands.queue(move |world: &mut World| {
        work_on(world, asker, &folder);
    });
}

fn work_on(world: &mut World, hamlet: Entity, folder: &Path) {
    tell(world, hamlet, format!("/folder {}", folder.display()));
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

/// The folder `hamlet` works on and the ones it worked on before.
pub(crate) fn section(hamlet: Entity) -> impl View {
    of_works(hamlet, works_on)
}

/// Whether `hamlet`'s agents ask before each edit and command.
pub(crate) fn approvals(hamlet: Entity) -> impl View {
    of_works(hamlet, approvals_of)
}

fn works_on(hamlet: Entity, work: &Works) -> AnyView {
    let choose = action(
        Glyph::Folder,
        match work.folder {
            Some(_) => "Choose Another",
            None => "Choose a Folder",
        },
        move |world| {
            world.trigger(ChooseFolder {
                asker: hamlet,
                title: "Choose the folder this hamlet works on"
                    .into(),
            });
        },
    )
    .tooltip(|| note("A path on the server's machine"))
    .boxed();
    let (now, acts) = match &work.folder {
        Some(folder) => (
            label(folder.clone()).boxed(),
            vec![
                choose,
                lettered("Clear")
                    .variant(Variant::Default)
                    .on_activate(move |world| {
                        tell(
                            world,
                            hamlet,
                            "/folder none".to_string(),
                        );
                    })
                    .tooltip(|| note("Stop working on this folder"))
                    .boxed(),
            ],
        ),
        None => {
            (description("No folder selected.").boxed(), vec![choose])
        }
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
    bylaw(Glyph::Folder, "Works on", buttons(acts).boxed(), now, more)
}

fn approvals_of(hamlet: Entity, work: &Works) -> AnyView {
    let mode = work.mode;
    bylaw(
        Glyph::Shield,
        "Approvals",
        segmented_words(
            [words::ASK_FIRST, words::NO_ASKING],
            match mode {
                Mode::Manual => 0,
                Mode::Auto => 1,
            },
            move |world, chosen| {
                let to = match chosen {
                    0 => Mode::Manual,
                    _ => Mode::Auto,
                };
                if to != mode {
                    set_mode(world, hamlet, to);
                }
            },
        )
        .boxed(),
        description(work.words.clone()).boxed(),
        Vec::new(),
    )
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

fn set_mode(world: &mut World, hamlet: Entity, to: Mode) {
    let word = match to {
        Mode::Manual => "manual",
        Mode::Auto => "auto",
    };
    tell(world, hamlet, format!("/mode {word}"));
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
