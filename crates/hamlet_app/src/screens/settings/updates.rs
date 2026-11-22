//! The Updates page: this copy's version, and the channel it takes
//! updates from.

use bevy::ecs::world::World;
use bevy_fynix::views::label;
use bevy_fynix::{ViewExt, derived, keyed};
use hamlet_core::roll::{either, moved};
use hamlet_ui::styles::{description, parts, segmented_words};
use hamlet_ui::{AnyView, View};
use hamlet_update::Channel;

use super::{alone, field, told};
use crate::updates::{UpdateChannel, Updates};

pub(super) fn page() -> impl View {
    parts((
        field(
            "Version",
            alone(label(hamlet_update::current().to_string())),
        ),
        channel(),
    ))
}

/// The channel followed, picked among all three, and what it holds.
fn channel() -> AnyView {
    let shown = derived(|world: &World| {
        let looking = world
            .get_resource::<Updates>()
            .is_some_and(|updates| updates.looking);
        let channel = world.get_resource::<UpdateChannel>().copied();
        (looking, channel.unwrap_or_default().followed())
    })
    .when(either(moved::<Updates>(), moved::<UpdateChannel>()));
    keyed(shown, |(looking, followed)| {
        if !*looking {
            return field(
                "Channel",
                description(
                    "This copy was built from source and does not \
                     update itself.",
                ),
            )
            .boxed();
        }
        field(
            "Channel",
            told(
                segmented_words(
                    Channel::ALL.map(Channel::name),
                    Channel::ALL
                        .iter()
                        .position(|channel| channel == followed)
                        .unwrap_or_default(),
                    |world, at| {
                        world.resource_mut::<UpdateChannel>().0 =
                            Some(Channel::ALL[at]);
                    },
                ),
                description(about(*followed)),
            ),
        )
        .boxed()
    })
    .boxed()
}

/// What `channel` holds, and how leaving for a steadier one goes.
fn about(channel: Channel) -> &'static str {
    match channel {
        Channel::Stable => {
            "Tested releases. From a newer channel, the update waits \
             for a stable release newer than this copy."
        }
        Channel::Beta => {
            "The next release while it is tested, then the release. \
             From Nightly, the update waits for a beta newer than \
             this copy."
        }
        Channel::Nightly => {
            "Every change to Hamlet once its checks pass. Some break \
             things."
        }
    }
}
