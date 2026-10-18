//! The choices a plugin has, and the verdicts they give.

use bevy::ecs::component::Component;
use bevy::ecs::lifecycle::Insert;
use bevy::ecs::observer::On;
use bevy::ecs::system::{Commands, Query};

use super::ask::Asks;
use super::choice::Choice;
use super::name::Capability;

/// What a host function does with a call that needs a capability the
/// manifest names.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    Allowed,
    /// Refused until the chief is asked.
    Ask,
    Denied,
}

/// On the entity of an installed plugin: the chief's choice for each
/// capability, and the ones let through for this run.
#[derive(Component, Clone, PartialEq, Eq, Default, Debug)]
#[component(immutable)]
pub struct Choices {
    set: Vec<(Capability, Choice)>,
    this_run: Vec<Capability>,
}

impl Choices {
    /// The choices of a chief who allowed all of `capabilities`.
    pub fn allowing(capabilities: &[Capability]) -> Self {
        capabilities.iter().fold(Self::default(), |choices, c| {
            choices.decided(*c, Choice::Allow, false)
        })
    }

    /// The chief's choice for `capability`, if it was answered.
    pub fn choice(&self, capability: Capability) -> Option<Choice> {
        self.set
            .iter()
            .find(|(held, _)| *held == capability)
            .map(|(_, choice)| *choice)
    }

    /// Every capability that was answered, with its choice.
    pub fn answered(&self) -> &[(Capability, Choice)] {
        &self.set
    }

    /// Whether `capability` is allowed for this run only.
    pub fn lent(&self, capability: Capability) -> bool {
        self.this_run.contains(&capability)
    }

    /// These choices with `capability` chosen. `this_run` lets an
    /// [`Choice::AskNextTime`] through until the app quits; any other
    /// choice takes back what was let through.
    pub fn decided(
        mut self,
        capability: Capability,
        choice: Choice,
        this_run: bool,
    ) -> Self {
        self.set.retain(|(held, _)| *held != capability);
        self.set.push((capability, choice));
        self.this_run.retain(|held| *held != capability);
        if this_run && choice == Choice::AskNextTime {
            self.this_run.push(capability);
        }
        self
    }

    /// These choices, less the ones for capabilities not in `asked`.
    pub fn within(mut self, asked: &[Capability]) -> Self {
        self.set.retain(|(held, _)| asked.contains(held));
        self.this_run.retain(|held| asked.contains(held));
        self
    }

    fn verdict(&self, capability: Capability) -> Verdict {
        match self.choice(capability) {
            Some(Choice::Allow) => Verdict::Allowed,
            Some(Choice::Deny) => Verdict::Denied,
            _ if self.lent(capability) => Verdict::Allowed,
            _ => Verdict::Ask,
        }
    }
}

/// On the entity of an installed plugin: the verdict for each
/// capability its manifest names, rebuilt from [`Choices`]. A host
/// function checks it on each call, so a component put in its place
/// is seen by the next call.
#[derive(Component, Clone, PartialEq, Eq, Default, Debug)]
#[component(immutable)]
pub struct Granted(Vec<(Capability, Verdict)>);

impl Granted {
    /// The verdicts for what `asks` names, from `choices`.
    pub fn of(asks: &Asks, choices: &Choices) -> Self {
        Self(
            asks.0
                .iter()
                .map(|ask| {
                    (ask.capability, choices.verdict(ask.capability))
                })
                .collect(),
        )
    }

    /// A plugin that is allowed all of `capabilities`.
    pub fn allowing(capabilities: &[Capability]) -> Self {
        Self(
            capabilities
                .iter()
                .map(|capability| (*capability, Verdict::Allowed))
                .collect(),
        )
    }

    /// The verdict for `capability`, or `None` where the manifest
    /// does not name it. Nothing unnamed can be used or asked for.
    pub fn verdict(&self, capability: Capability) -> Option<Verdict> {
        self.0
            .iter()
            .find(|(held, _)| *held == capability)
            .map(|(_, verdict)| *verdict)
    }

    pub fn holds(&self, capability: Capability) -> bool {
        self.verdict(capability) == Some(Verdict::Allowed)
    }

    /// The verdicts, in the order of the manifest.
    pub fn all(&self) -> &[(Capability, Verdict)] {
        &self.0
    }
}

/// Puts the [`Granted`] of a plugin back when its asks or choices
/// are put in.
pub(super) fn rebuild(
    insert: On<Insert, (Asks, Choices)>,
    plugins: Query<(&Asks, &Choices)>,
    mut commands: Commands,
) {
    if let Ok((asks, choices)) = plugins.get(insert.entity) {
        commands
            .entity(insert.entity)
            .insert(Granted::of(asks, choices));
    }
}
