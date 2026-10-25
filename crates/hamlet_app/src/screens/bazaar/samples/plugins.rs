//! Plugins: extensions to Hamlet itself.

use std::cmp::Ordering;

use crate::bazaar::samples::{
    ORDERS, Order, Price, Ranked, Seller, Standing, rank, seller,
    standing,
};
use crate::bazaar::shelf::Listing;

/// A plugin: something that adds to Hamlet itself.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Contraption {
    pub id: &'static str,
    pub name: &'static str,
    pub publisher: Seller,
    /// The day it last changed, as `YYYY-MM-DD`.
    pub updated: &'static str,
    pub price: Price,
    /// Its owners are those who installed it.
    pub standing: Standing,
    /// What it is, in a line.
    pub summary: &'static str,
    pub description: &'static str,
    pub kind: Kind,
    pub adds: &'static [Addition],
    pub asks: &'static [Permission],
    /// The latest version and what it changed.
    pub latest: (&'static str, &'static str),
}

/// The shelf of the stall a plugin sits on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Kind {
    Drawing,
    Planning,
    Data,
    Language,
    Maps,
}

impl Kind {
    pub const ALL: [Kind; 5] = [
        Kind::Drawing,
        Kind::Planning,
        Kind::Data,
        Kind::Language,
        Kind::Maps,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Kind::Drawing => "Diagrams",
            Kind::Planning => "Productivity",
            Kind::Data => "Data",
            Kind::Language => "Language",
            Kind::Maps => "Integrations",
        }
    }
}

/// One thing a plugin adds to Hamlet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Addition {
    pub part: Part,
    pub name: &'static str,
    pub words: &'static str,
}

/// The part of Hamlet an addition is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Part {
    /// A panel of the dock.
    Panel,
    /// A tool the agents can call.
    Tool,
    /// A command of the command line.
    Command,
    /// A way of drawing something in the chat.
    InChat,
}

impl Part {
    pub fn name(self) -> &'static str {
        match self {
            Part::Panel => "Panel",
            Part::Tool => "Tool for agents",
            Part::Command => "Command",
            Part::InChat => "In the chat",
        }
    }
}

/// Something a plugin asks to be let do.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Permission {
    ReadWorkspace,
    WriteWorkspace,
    RunCommands,
    /// To reach this host.
    Network(&'static str),
    Notifications,
}

impl Permission {
    pub fn words(self) -> String {
        match self {
            Permission::ReadWorkspace => {
                "Read the files of the workspace".into()
            }
            Permission::WriteWorkspace => {
                "Write files in the workspace".into()
            }
            Permission::RunCommands => {
                "Run commands on this computer".into()
            }
            Permission::Network(host) => format!("Reach {host}"),
            Permission::Notifications => {
                "Send you notifications".into()
            }
        }
    }

    /// True when it lets the plugin change what is the chief's.
    pub fn grave(self) -> bool {
        matches!(
            self,
            Permission::WriteWorkspace | Permission::RunCommands
        )
    }
}

impl Contraption {
    fn ranked(&self) -> Ranked {
        (self.price, self.standing, self.updated)
    }
}

impl Listing for Contraption {
    type Id = String;
    type Filter = Option<Kind>;
    type Order = Order;

    const ORDERS: &'static [(Order, &'static str)] = ORDERS;

    fn id(&self) -> String {
        self.id.to_string()
    }

    fn told(&self) -> String {
        let adds = self.adds.iter().map(|addition| addition.name);
        format!(
            "{} {} {} {} {}",
            self.name,
            self.publisher.name,
            self.summary,
            self.kind.name(),
            adds.collect::<Vec<_>>().join(" ")
        )
    }

    fn admits(&self, kind: &Option<Kind>) -> bool {
        kind.is_none_or(|kind| self.kind == kind)
    }

    fn order(&self, other: &Contraption, order: Order) -> Ordering {
        rank(self.ranked(), other.ranked(), order)
            .then_with(|| self.name.cmp(other.name))
    }
}

const fn adds(
    part: Part,
    name: &'static str,
    words: &'static str,
) -> Addition {
    Addition { part, name, words }
}

/// The plugins on offer.
pub(crate) static CONTRAPTIONS: [Contraption; 4] = [
    Contraption {
        id: "excalidraw",
        name: "Excalidraw Whiteboard",
        publisher: seller("Mara Lindqvist", true),
        updated: "2026-10-06",
        price: Price::Free,
        standing: standing(4.9, 1240, 48_200),
        summary: "A whiteboard where agents draw diagrams and you \
                  edit them.",
        description: "Adds an Excalidraw whiteboard panel. Agents \
                      can draw flowcharts, architecture sketches \
                      and mind maps while they work, and you can \
                      move, edit or annotate anything they drew. \
                      Boards are saved as .excalidraw files in the \
                      workspace.",
        kind: Kind::Drawing,
        adds: &[
            adds(
                Part::Panel,
                "Whiteboard",
                "An Excalidraw canvas docked beside the world.",
            ),
            adds(
                Part::Tool,
                "whiteboard.draw",
                "Lets agents add shapes, arrows and text.",
            ),
            adds(Part::Command, "/whiteboard", "Opens a new board."),
        ],
        asks: &[
            Permission::ReadWorkspace,
            Permission::WriteWorkspace,
        ],
        latest: ("2.4.0", "Agents can turn a table into a chart."),
    },
    Contraption {
        id: "mermaid",
        name: "Mermaid Diagrams",
        publisher: seller("Diagramming Co.", true),
        updated: "2026-09-28",
        price: Price::Free,
        standing: standing(4.7, 860, 61_400),
        summary: "Renders Mermaid code blocks in chat as diagrams.",
        description: "Any mermaid block an agent writes is drawn as \
                      a diagram right in the chat: flowcharts, \
                      sequence diagrams, Gantt charts and more.",
        kind: Kind::Drawing,
        adds: &[adds(
            Part::InChat,
            "Mermaid blocks",
            "Draws diagrams instead of code.",
        )],
        asks: &[],
        latest: ("1.8.2", "Supports Mermaid 11 syntax."),
    },
    Contraption {
        id: "github",
        name: "GitHub",
        publisher: seller("Octoplane Labs", true),
        updated: "2026-10-11",
        price: Price::Free,
        standing: standing(4.8, 2210, 92_300),
        summary: "Issues and pull requests in a panel, and GitHub \
                  tools for agents.",
        description: "Sign in once and see your issues and pull \
                      requests in a panel. Agents can read issues, \
                      open branches, comment on and review pull \
                      requests, and check CI runs. Every write \
                      action waits for your approval.",
        kind: Kind::Maps,
        adds: &[
            adds(
                Part::Panel,
                "Pull requests",
                "Your open PRs and review requests.",
            ),
            adds(
                Part::Tool,
                "github.issues",
                "Search, read and comment on issues.",
            ),
        ],
        asks: &[
            Permission::Network("api.github.com"),
            Permission::ReadWorkspace,
            Permission::RunCommands,
        ],
        latest: ("3.1.0", "Agents can read failing CI logs."),
    },
    Contraption {
        id: "linear",
        name: "Linear",
        publisher: seller("Northbeam Studio", false),
        updated: "2026-08-30",
        price: Price::Monthly(3.0),
        standing: standing(4.5, 310, 8_900),
        summary: "Turn chats into Linear issues and track them from \
                  Hamlet.",
        description: "Agents can create and update Linear issues, \
                      move them between states and link them to the \
                      work they did. A panel lists the issues \
                      assigned to you.",
        kind: Kind::Planning,
        adds: &[
            adds(
                Part::Panel,
                "My issues",
                "Your assigned issues by cycle.",
            ),
            adds(
                Part::Tool,
                "linear.issue",
                "Create and update issues.",
            ),
        ],
        asks: &[
            Permission::Network("api.linear.app"),
            Permission::Notifications,
        ],
        latest: ("1.2.0", "Supports sub-issues."),
    },
];
