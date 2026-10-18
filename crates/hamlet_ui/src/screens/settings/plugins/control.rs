//! The three-state control of one capability of one plugin.

use bevy_fynix::views::BehaviorExt;
use bevy_fynix::{ScopedExt, ViewExt};
use hamlet_sdk::capability::{Capability, Choice, Decide};

use crate::feel::Mood;
use crate::looks::{brass, buttons, grim, lamp, lettered};
use crate::{AnyView, Cx};

/// Allow, Ask next time and Don't allow side by side, the choice
/// `now` lit. A press sets the plugin's choice at once.
pub(super) fn control(
    plugin: &str,
    capability: Capability,
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
            lettered(choice.words())
                .rules(rules)
                .tagged(mood)
                .on_activate(move |world| {
                    world.trigger(Decide {
                        plugin: plugin.clone(),
                        capability,
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
