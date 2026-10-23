//! The status line's facts about the agents, left to right: the
//! sandbox, the model, the link, what the open session used, the
//! first hamlet's spend, the address under the pointer in the room
//! left, and whether the open session works. A fact with nothing to
//! say is hidden. How many things wait for the chief is the engine's
//! Petitions item.

use hamlet_engine_bevy::api::agents::{
    AgentsEvent, AgentsReply, AgentsRequest, Link, Status,
};
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::ui::{StatusItem, Tone, set_status};
use hamlet_engine_bevy::{DesktopEvent, DesktopRequest, Reply};

/// The agents' status items, in the order the line shows them.
const ITEMS: [&str; 6] =
    ["sandbox", "model", "link", "usage", "treasury", "work"];

/// The item of the address under the pointer, before `work`.
const POINTED: &str = "pointed";

pub(crate) fn plugin(app: &mut App) {
    for name in ITEMS {
        if name == "work" {
            app.add_growing_status_item(POINTED);
        }
        app.add_status_item(name);
    }
    app.add_systems(Declare, watch).add_systems(Update, show);
}

fn watch(mut commands: Commands) {
    commands.agents(&AgentsRequest::Watch);
    commands.agents(&AgentsRequest::Status);
    commands.desktop(&DesktopRequest::Watch);
}

/// Shows what the agents and the desktop services tell.
fn show(
    mut replies: MessageReader<Replied>,
    mut agents: MessageReader<AgentsEventReceived>,
    mut desktop: MessageReader<DesktopEventReceived>,
) {
    for replied in replies.read() {
        if let Ok(Reply::Agents(AgentsReply::Status(status))) =
            &replied.reply
        {
            status.show();
        }
    }
    for told in agents.read() {
        if let AgentsEvent::Status(status) = &told.event {
            status.show();
        }
    }
    for told in desktop.read() {
        if let DesktopEvent::Hovered(address) = &told.event {
            set_status(
                POINTED,
                &StatusItem {
                    icon: String::new(),
                    text: address.clone().unwrap_or_default(),
                    tone: Tone::Accent,
                    tip: String::new(),
                },
            );
        }
    }
}

/// The status line's methods on [`Status`].
trait StatusExt {
    /// Shows each fact in its item.
    fn show(&self);
}

impl StatusExt for Status {
    fn show(&self) {
        let fact = |icon: &str, text: &str, tone: Tone, tip: &str| {
            StatusItem {
                icon: icon.into(),
                text: text.into(),
                tone,
                tip: tip.into(),
            }
        };
        let (link, link_tone, link_tip) = match &self.link {
            Link::Untested => {
                ("Connection not tested".into(), Tone::Dim, "")
            }
            Link::Testing => {
                ("Testing connection...".into(), Tone::Dim, "")
            }
            Link::Connected(models) => (
                format!(
                    "Connected, {} {}",
                    models.len(),
                    if models.len() == 1 {
                        "model"
                    } else {
                        "models"
                    }
                ),
                Tone::Dim,
                "",
            ),
            Link::Failed(why) => {
                ("Not connected".into(), Tone::Critical, why.as_str())
            }
        };
        let work = if self.working {
            fact("bolt", "Working", Tone::Accent, "")
        } else {
            fact("moon", "Running, idle", Tone::Dim, "")
        };
        let facts = [
            fact(
                "shield",
                &self.sandbox,
                Tone::Dim,
                "The sandbox agents' commands run in",
            ),
            fact("chip", &self.model, Tone::Dim, "Model"),
            fact("signal", &link, link_tone, link_tip),
            fact(
                "coins",
                &self.usage,
                Tone::Dim,
                "Used by this session",
            ),
            fact(
                "meter",
                &self.treasury,
                Tone::Dim,
                "Spent of the hamlet's budget",
            ),
            work,
        ];
        for (name, fact) in ITEMS.into_iter().zip(&facts) {
            set_status(name, fact);
        }
    }
}
