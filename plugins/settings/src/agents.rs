//! The Agents page: how often they reflect, whether their tools reach
//! the network, whether the computer stays awake while they work, and
//! the permissions they were given for good.

use hamlet_engine_bevy::DesktopRequest;
use hamlet_engine_bevy::api::agents::{AgentsRequest, Grant};
use hamlet_engine_bevy::ui::{
    AnyView, ButtonStyle, FrameProps, Size, Space, UiExt, ViewExt,
    button, column, field, keyed, label, resource, row, toggle,
};

use crate::parts::{note, problem, setting};
use crate::{Known, Page, SettingsWorldExt};

/// The most turns between two reflections.
const MOST_REFLECT: u32 = 1000;

pub(crate) fn page() -> AnyView {
    let reflect = field(resource::<Known, _>(|known| {
        known
            .settings
            .as_ref()
            .map(|settings| settings.reflect_every.to_string())
            .unwrap_or_default()
    }))
    .placeholder("0 to 1000")
    .on_commit(|world, typed| {
        match typed
            .trim()
            .parse::<u32>()
            .ok()
            .filter(|turns| *turns <= MOST_REFLECT)
        {
            Some(turns) => world.ask(
                Page::Agents,
                AgentsRequest::SetReflectEvery(turns),
            ),
            None => world.refuse(
                Page::Agents,
                format!(
                    "Reflection is every 0 to {MOST_REFLECT} turns."
                ),
            ),
        }
    })
    .boxed();
    let net = column((
        toggle(resource::<Known, _>(allows_net)).on_toggle(
            |world, on| {
                world.ask(
                    Page::Agents,
                    AgentsRequest::SetAllowNet(on),
                );
            },
        ),
        note(resource::<Known, _>(|known| {
            if allows_net(known) {
                "Agents' commands can reach the internet.".to_string()
            } else {
                "Agents' commands cannot reach the internet."
                    .to_string()
            }
        })),
    ))
    .space(Space::Half)
    .boxed();
    let awake = column((
        // On until the desktop service says, as it is by default.
        toggle(resource::<Known, _>(|known| {
            known.keep_awake.unwrap_or(true)
        }))
        .on_toggle(|world, on| {
            world.ask_desktop(
                Page::Agents,
                DesktopRequest::SetKeepAwake(on),
            );
        }),
        note("The computer stays awake while an agent works."),
    ))
    .space(Space::Half)
    .boxed();
    let held = keyed(
        resource::<Known, _>(|known| known.grants.clone()),
        |grants| permissions(grants),
    )
    .boxed();
    column((
        setting(
            "Reflect every N turns",
            reflect,
            Some("0 means never."),
        ),
        setting("Tools network access", net, None),
        setting("Keep awake", awake, None),
        setting("Lasting permissions", held, None),
        problem(Page::Agents),
    ))
    .boxed()
}

fn allows_net(known: &Known) -> bool {
    known
        .settings
        .as_ref()
        .is_some_and(|settings| settings.allow_net)
}

/// The permissions agents were given for good, each to take back.
fn permissions(grants: &[Grant]) -> AnyView {
    if grants.is_empty() {
        return note(
            "None yet. Choosing Always Allow on a petition adds one.",
        )
        .boxed();
    }
    let rows = grants
        .iter()
        .map(|grant| {
            let says = format!(
                "{} may use: {} (given by {})",
                grant.name, grant.permission, grant.by
            );
            let (agent, permission) =
                (grant.name.clone(), grant.permission.clone());
            row((
                label(says).size(Size::Small),
                button("REVOKE")
                    .style(ButtonStyle::Critical)
                    .on_click(move |world| {
                        world.ask(
                            Page::Agents,
                            AgentsRequest::Revoke {
                                agent: agent.clone(),
                                permission: permission.clone(),
                            },
                        );
                    }),
            ))
            .boxed()
        })
        .collect::<Vec<_>>();
    column(rows).space(Space::Half).boxed()
}
