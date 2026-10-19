//! The three-state control of one capability of one plugin.

use bevy_fynix::views::{BehaviorExt, FrameProps, column, label};
use bevy_fynix::{ScopedExt, ViewExt};
use hamlet_sdk::capability::{Capability, Choice, Choices, Decide};
use hamlet_sdk::plugin::Manifest;

use crate::feel::Mood;
use crate::looks::{brass, buttons, grim, lamp, lettered, note};
use crate::platform::DisplayText;
use crate::{AnyView, Cx};

/// What the chief chose for one target of a scoped capability, such
/// as one plugin that a plugin may use.
#[derive(Clone, PartialEq, Debug)]
pub(super) struct Target {
    pub name: String,
    /// The chief's choice, if it was answered.
    pub choice: Option<Choice>,
    /// Allowed for this run only.
    pub lent: bool,
}

/// The choices for each target `capability` is chosen for one by
/// one: those `manifest` names, then those the chief answered for
/// besides, such as a folder chosen in a dialog.
pub(super) fn targets(
    manifest: Option<&Manifest>,
    choices: &Choices,
    capability: Capability,
) -> Vec<Target> {
    let mut names = manifest
        .map(|manifest| manifest.targets(capability))
        .unwrap_or_default();
    for (held, name, _) in choices.answered_targets() {
        if *held == capability && !names.contains(name) {
            names.push(name.clone());
        }
    }
    names
        .into_iter()
        .map(|name| Target {
            choice: choices.choice_for(capability, &name),
            lent: choices.lent_for(capability, &name),
            name,
        })
        .collect()
}

/// Allow, Ask next time and Don't allow side by side, the choice
/// `now` lit. A press sets the plugin's choice at once, for `target`
/// where the capability is scoped.
pub(super) fn control(
    plugin: &str,
    capability: Capability,
    target: Option<&str>,
    now: Option<Choice>,
) -> AnyView {
    let choices = Choice::ALL
        .into_iter()
        .map(|choice| {
            let (rules, mood): (fn(&mut Cx), Mood) =
                match (now == Some(choice), choice) {
                    (true, Choice::Deny) => (grim, Mood::Bad),
                    (true, _) => (lamp, Mood::Good),
                    (false, _) => (brass, Mood::Plain),
                };
            let plugin = plugin.to_string();
            let target = target.map(str::to_string);
            lettered(choice.words())
                .rules(rules)
                .tagged(mood)
                .on_activate(move |world| {
                    world.trigger(Decide {
                        plugin: plugin.clone(),
                        capability,
                        target: target.clone(),
                        choice,
                        this_run: false,
                    });
                    world.flush();
                })
                .boxed()
        })
        .collect::<Vec<_>>();
    buttons(choices).boxed()
}

/// What a choice that is not Allow or Don't allow means, in words.
pub(super) fn state(
    choice: Option<Choice>,
    lent: bool,
) -> &'static str {
    match (choice, lent) {
        (_, true) => "Allowed until Hamlet quits.",
        (None, _) => {
            "Not answered yet. It is asked at the first use."
        }
        (Some(Choice::AskNextTime), _) => {
            "Asked at the next use after Hamlet starts."
        }
        _ => "",
    }
}

/// A target of a scoped capability, its name over its control and
/// what the choice means.
pub(super) fn target(
    plugin: &str,
    capability: Capability,
    target: &Target,
) -> AnyView {
    let mut parts = vec![
        label(target.name.clone())
            .size(12.0)
            .bold(true)
            .tagged(DisplayText)
            .boxed(),
        control(
            plugin,
            capability,
            Some(&target.name),
            target.choice,
        ),
    ];
    let words = state(target.choice, target.lent);
    if !words.is_empty() {
        parts.push(note(words).boxed());
    }
    column(parts).gap(4.0).boxed()
}
