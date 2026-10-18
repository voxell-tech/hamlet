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
/// capability, and the ones let through for this run. A scoped
/// capability, such as `plugin.use`, has a choice for each target
/// besides.
#[derive(Component, Clone, PartialEq, Eq, Default, Debug)]
#[component(immutable)]
pub struct Choices {
    set: Vec<(Capability, Choice)>,
    this_run: Vec<Capability>,
    targets: Vec<(Capability, String, Choice)>,
    targets_this_run: Vec<(Capability, String)>,
}

impl Choices {
    /// The choices of a chief who allowed all of `capabilities`, and
    /// left it to the first use to ask for each target of the scoped
    /// ones.
    pub fn allowing(capabilities: &[Capability]) -> Self {
        capabilities.iter().fold(Self::default(), |choices, c| {
            let choice = match c.scoped() {
                true => Choice::AskNextTime,
                false => Choice::Allow,
            };
            choices.decided(*c, choice, false)
        })
    }

    /// The chief's choice for `target` of the scoped `capability`, if
    /// it was answered.
    pub fn choice_for(
        &self,
        capability: Capability,
        target: &str,
    ) -> Option<Choice> {
        self.targets
            .iter()
            .find(|(held, to, _)| *held == capability && to == target)
            .map(|(.., choice)| *choice)
    }

    /// Whether `target` of `capability` is allowed for this run only.
    pub fn lent_for(
        &self,
        capability: Capability,
        target: &str,
    ) -> bool {
        self.targets_this_run
            .iter()
            .any(|(held, to)| *held == capability && to == target)
    }

    /// Every target that was answered, with its capability and
    /// choice.
    pub fn answered_targets(
        &self,
    ) -> &[(Capability, String, Choice)] {
        &self.targets
    }

    /// These choices with `target` of the scoped `capability` chosen.
    /// `this_run` is as for [`Choices::decided`].
    pub fn decided_for(
        mut self,
        capability: Capability,
        target: &str,
        choice: Choice,
        this_run: bool,
    ) -> Self {
        self.targets.retain(|(held, to, _)| {
            !(*held == capability && to == target)
        });
        self.targets.push((capability, target.into(), choice));
        self.targets_this_run.retain(|(held, to)| {
            !(*held == capability && to == target)
        });
        if this_run && choice == Choice::AskNextTime {
            self.targets_this_run.push((capability, target.into()));
        }
        self
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
        self.targets.retain(|(held, ..)| asked.contains(held));
        self.targets_this_run
            .retain(|(held, _)| asked.contains(held));
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

    fn target_verdict(
        &self,
        capability: Capability,
        target: &str,
        choice: Choice,
    ) -> Verdict {
        match choice {
            Choice::Allow => Verdict::Allowed,
            Choice::Deny => Verdict::Denied,
            Choice::AskNextTime
                if self.lent_for(capability, target) =>
            {
                Verdict::Allowed
            }
            Choice::AskNextTime => Verdict::Ask,
        }
    }
}

/// On the entity of an installed plugin: the verdict for each
/// capability its manifest names, rebuilt from [`Choices`]. A host
/// function checks it on each call, so a component put in its place
/// is seen by the next call.
#[derive(Component, Clone, PartialEq, Eq, Default, Debug)]
#[component(immutable)]
pub struct Granted {
    capabilities: Vec<(Capability, Verdict)>,
    /// The verdicts the chief gave for a target of a scoped
    /// capability.
    targets: Vec<(Capability, String, Verdict)>,
    /// The targets a scoped ask is limited to. Empty is no limit.
    scopes: Vec<(Capability, Vec<String>)>,
}

impl Granted {
    /// The verdicts for what `asks` names, from `choices`.
    pub fn of(asks: &Asks, choices: &Choices) -> Self {
        let named = |capability| asks.names(capability);
        Self {
            capabilities: asks
                .0
                .iter()
                .map(|ask| {
                    (ask.capability, choices.verdict(ask.capability))
                })
                .collect(),
            targets: choices
                .targets
                .iter()
                .filter(|(capability, ..)| named(*capability))
                .map(|(capability, target, choice)| {
                    (
                        *capability,
                        target.clone(),
                        choices.target_verdict(
                            *capability,
                            target,
                            *choice,
                        ),
                    )
                })
                .collect(),
            scopes: asks
                .0
                .iter()
                .map(|ask| (ask.capability, ask.scope.0.clone()))
                .collect(),
        }
    }

    /// A plugin that is allowed all of `capabilities`.
    pub fn allowing(capabilities: &[Capability]) -> Self {
        Self {
            capabilities: capabilities
                .iter()
                .map(|capability| (*capability, Verdict::Allowed))
                .collect(),
            ..Self::default()
        }
    }

    /// This, with `targets` of the scoped `capability` allowed.
    pub fn allowing_targets(
        mut self,
        capability: Capability,
        targets: &[&str],
    ) -> Self {
        if !self
            .capabilities
            .iter()
            .any(|(held, _)| *held == capability)
        {
            self.capabilities.push((capability, Verdict::Ask));
        }
        self.targets.extend(targets.iter().map(|target| {
            (capability, target.to_string(), Verdict::Allowed)
        }));
        self
    }

    /// The verdict for `capability`, or `None` where the manifest
    /// does not name it. Nothing unnamed can be used or asked for.
    pub fn verdict(&self, capability: Capability) -> Option<Verdict> {
        self.capabilities
            .iter()
            .find(|(held, _)| *held == capability)
            .map(|(_, verdict)| *verdict)
    }

    /// The verdict for `target` of the scoped `capability`, or `None`
    /// where the manifest does not name the capability or limits it
    /// to targets that do not include this one. A target the chief
    /// has not answered for is [`Verdict::Ask`].
    pub fn verdict_for(
        &self,
        capability: Capability,
        target: &str,
    ) -> Option<Verdict> {
        if self.verdict(capability)? == Verdict::Denied {
            return Some(Verdict::Denied);
        }
        let within = self
            .scopes
            .iter()
            .find(|(held, _)| *held == capability)
            .is_none_or(|(_, scope)| {
                scope.is_empty()
                    || scope.iter().any(|to| to == target)
            });
        if !within {
            return None;
        }
        Some(
            self.targets
                .iter()
                .find(|(held, to, _)| {
                    *held == capability && to == target
                })
                .map_or(Verdict::Ask, |(.., verdict)| *verdict),
        )
    }

    pub fn holds(&self, capability: Capability) -> bool {
        self.verdict(capability) == Some(Verdict::Allowed)
    }

    /// The verdicts, in the order of the manifest.
    pub fn all(&self) -> &[(Capability, Verdict)] {
        &self.capabilities
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
