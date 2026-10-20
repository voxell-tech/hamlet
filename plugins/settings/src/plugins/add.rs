//! ADD PLUGIN...: a plugin added from a folder the chief picks, the
//! question asked on the way, the build and how it went.

use bevy::app::{App, Update};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::schedule::common_conditions::resource_exists_and_changed;
use bevy::ecs::system::{Res, ResMut};
use bevy::ecs::world::World;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{BehaviorExt, FrameProps, column, label};
use bevy_fynix::{ViewExt, keyed, resource};
use hamlet_sdk::capability::{
    AddAnswer, AddPlugin, Adding, Question,
};

use crate::icons::Glyph;
use crate::looks::{
    action, bad_action, bare, buttons, good_action, note,
};
use crate::platform::{DisplayText, MonoText};
use crate::{AnyView, View};

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Shown>().add_systems(
        Update,
        show.run_if(resource_exists_and_changed::<Adding>),
    );
}

/// What a build is warned of before it runs.
const WARNING: &str = "Building runs code from this folder on your \
                       computer with your access. Build only \
                       plugins you trust.";

/// How adding a plugin goes, where some host adds plugins.
#[derive(Resource, Default, PartialEq)]
struct Shown(Option<Adding>);

fn show(adding: Res<Adding>, mut shown: ResMut<Shown>) {
    if shown.0.as_ref() != Some(&*adding) {
        shown.0 = Some(adding.clone());
    }
}

/// The button, and what became of the last folder or what waits for
/// the chief.
pub(super) fn view() -> impl View {
    keyed(resource::<Shown, _>(|shown| shown.0.clone()), |adding| {
        match adding {
            None => column(()).boxed(),
            Some(adding) => part(adding),
        }
    })
    .within(bare())
}

fn part(adding: &Adding) -> AnyView {
    match adding {
        Adding::Idle => start(
            note(
                "A folder with a built plugin, or with the source \
                 of one to build.",
            )
            .boxed(),
        ),
        Adding::Added(words) => start(note(words.clone()).boxed()),
        Adding::Refused { why, log } => {
            let mut lines = vec![
                label(format!("Not added: {why}"))
                    .size(12.0)
                    .tone(Tone::Critical)
                    .boxed(),
            ];
            if !log.is_empty() {
                lines.push(
                    note(log.trim_end().to_string())
                        .tagged(MonoText)
                        .boxed(),
                );
            }
            start(column(lines).gap(4.0).boxed())
        }
        Adding::Choosing => {
            note("Choose the plugin's folder in the dialog.").boxed()
        }
        Adding::Reading => note("Reading the folder...").boxed(),
        Adding::Asking(question) => asking(question),
        Adding::Building { name, line } => column((
            note(format!("Building {name}: {line}")),
            buttons(vec![bad_action(Glyph::Stop, "STOP", |world| {
                answer(world, false)
            })]),
        ))
        .gap(6.0)
        .boxed(),
    }
}

/// The button that starts adding, over `under`.
fn start(under: AnyView) -> AnyView {
    column((
        buttons(vec![action(
            Glyph::Plus,
            "ADD PLUGIN...",
            |world| {
                world.trigger(AddPlugin);
                world.flush();
            },
        )]),
        under,
    ))
    .gap(6.0)
    .boxed()
}

/// The question put before a build, or before a folder takes the
/// place of a plugin that is there.
fn asking(question: &Question) -> AnyView {
    let named = format!("{} {}", question.name, question.version);
    let (head, verb) = match question.build {
        true => (format!("BUILD {}?", named.to_uppercase()), "BUILD"),
        false => (
            format!("REPLACE WITH {}?", named.to_uppercase()),
            "REPLACE",
        ),
    };
    let mut lines = vec![
        label(head)
            .size(13.0)
            .bold(true)
            .tagged(DisplayText)
            .boxed(),
    ];
    if question.build {
        lines.push(
            label(WARNING).size(12.0).tone(Tone::Critical).boxed(),
        );
    }
    if let Some(was) = &question.replaces {
        let then = match question.native {
            true => {
                "It loads when Hamlet starts again, which asks you \
                 first."
            }
            false => "Anything new it asks for is asked again.",
        };
        lines.push(
            note(format!("It takes the place of {was}. {then}"))
                .boxed(),
        );
    }
    lines.push(
        buttons(vec![
            good_action(Glyph::Check, verb, |world| {
                answer(world, true)
            }),
            action(Glyph::Cross, "CANCEL", |world| {
                answer(world, false)
            }),
        ])
        .boxed(),
    );
    column(lines).gap(6.0).boxed()
}

fn answer(world: &mut World, go: bool) {
    world.trigger(AddAnswer { go });
    world.flush();
}
