//! Plugins: extensions to Hamlet itself. Mock data for now.

use serde::{Deserialize, Serialize};

use super::{Listing, Price, Seller, Standing};

/// A plugin: something that adds to Hamlet itself.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Contraption {
    pub id: String,
    pub name: String,
    pub publisher: Seller,
    pub version: String,
    /// The day it last changed, as `YYYY-MM-DD`.
    pub updated: String,
    pub price: Price,
    /// Its owners are those who installed it.
    pub standing: Standing,
    /// What it is, in a line.
    pub summary: String,
    pub description: String,
    pub kind: Kind,
    pub adds: Vec<Addition>,
    pub asks: Vec<Permission>,
    /// The latest change first.
    pub changes: Vec<Change>,
}

/// The shelf of the stall a plugin sits on.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub enum Kind {
    Drawing,
    Planning,
    Data,
    Language,
    Maps,
    Looks,
}

impl Kind {
    pub const ALL: [Kind; 6] = [
        Kind::Drawing,
        Kind::Planning,
        Kind::Data,
        Kind::Language,
        Kind::Maps,
        Kind::Looks,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Kind::Drawing => "Drawing",
            Kind::Planning => "Planning",
            Kind::Data => "Data",
            Kind::Language => "Voice and language",
            Kind::Maps => "Maps",
            Kind::Looks => "Looks",
        }
    }
}

/// One thing a plugin adds to Hamlet.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Addition {
    pub part: Part,
    pub name: String,
    pub words: String,
}

/// The part of Hamlet an addition is.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
pub enum Part {
    /// A panel of the dock.
    Panel,
    /// A tool the agents can call.
    Tool,
    /// A command of the command line.
    Command,
    /// A way of drawing something in the chat.
    InChat,
    /// A look of the panels.
    Theme,
}

impl Part {
    pub fn name(self) -> &'static str {
        match self {
            Part::Panel => "Panel",
            Part::Tool => "Tool for agents",
            Part::Command => "Command",
            Part::InChat => "In the chat",
            Part::Theme => "Theme",
        }
    }
}

/// Something a plugin asks to be let do.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Permission {
    ReadWorkspace,
    WriteWorkspace,
    RunCommands,
    /// To reach this host.
    Network(String),
    Speakers,
    Microphone,
    Notifications,
}

impl Permission {
    pub fn words(&self) -> String {
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
            Permission::Speakers => "Play sound".into(),
            Permission::Microphone => {
                "Listen through the microphone".into()
            }
            Permission::Notifications => {
                "Send you notifications".into()
            }
        }
    }

    /// True when it lets the plugin change what is the chief's, or
    /// hear it.
    pub fn grave(&self) -> bool {
        matches!(
            self,
            Permission::WriteWorkspace
                | Permission::RunCommands
                | Permission::Microphone
        )
    }
}

/// What one version changed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Change {
    pub version: String,
    pub words: String,
}

impl Listing for Contraption {
    fn name(&self) -> &str {
        &self.name
    }

    fn told(&self) -> String {
        let adds = self
            .adds
            .iter()
            .map(|addition| addition.name.as_str())
            .collect::<Vec<_>>();
        format!(
            "{} {} {} {} {}",
            self.name,
            self.publisher.name,
            self.summary,
            self.kind.name(),
            adds.join(" ")
        )
    }

    fn price(&self) -> Price {
        self.price
    }

    fn standing(&self) -> Standing {
        self.standing
    }

    fn updated(&self) -> &str {
        &self.updated
    }
}

fn seller(name: &str, verified: bool) -> Seller {
    Seller {
        name: name.into(),
        verified,
    }
}

fn standing(stars: f32, reviews: u32, owners: u32) -> Standing {
    Standing {
        stars,
        reviews,
        owners,
    }
}

fn adds(part: Part, name: &str, words: &str) -> Addition {
    Addition {
        part,
        name: name.into(),
        words: words.into(),
    }
}

fn change(version: &str, words: &str) -> Change {
    Change {
        version: version.into(),
        words: words.into(),
    }
}

/// The plugins on offer.
pub fn contraptions() -> Vec<Contraption> {
    use Part::*;
    use Permission::*;
    vec![
        Contraption {
            id: "excalidraw".into(),
            name: "Excalidraw Canvas".into(),
            publisher: seller("Sketchwright & Daughters", true),
            version: "2.4.0".into(),
            updated: "2026-10-06".into(),
            price: Price::Free,
            standing: standing(4.9, 1240, 48_200),
            summary: "A hand-drawn canvas where agents sketch \
                      diagrams and graphs."
                .into(),
            description: "Opens an Excalidraw canvas in a panel of \
                          its own. Agents draw on it as they think: \
                          boxes and arrows for a plan, a graph of \
                          what they found, a map of the code. Drag, \
                          scribble and hand it back."
                .into(),
            kind: Kind::Drawing,
            adds: vec![
                adds(
                    Panel,
                    "Canvas",
                    "A drawing board beside the world.",
                ),
                adds(
                    Tool,
                    "draw",
                    "Agents draw shapes, arrows, graphs and text.",
                ),
                adds(Command, "/sketch", "Opens a new drawing."),
            ],
            asks: vec![ReadWorkspace, WriteWorkspace],
            changes: vec![
                change("2.4.0", "Graphs from a table of numbers."),
                change("2.3.0", "Agents can label their arrows."),
                change("2.2.1", "Faster with large drawings."),
            ],
        },
        Contraption {
            id: "mermaid".into(),
            name: "Mermaid Renderer".into(),
            publisher: seller("Tidewater Tools", true),
            version: "1.8.2".into(),
            updated: "2026-09-12".into(),
            price: Price::Free,
            standing: standing(4.7, 860, 31_400),
            summary: "Draws Mermaid diagrams right in the chat."
                .into(),
            description: "Flowcharts, sequence diagrams, timelines \
                          and family trees written in Mermaid are \
                          drawn where they are written, in the chat \
                          and in notes."
                .into(),
            kind: Kind::Drawing,
            adds: vec![
                adds(
                    InChat,
                    "Mermaid blocks",
                    "Draws ```mermaid blocks as diagrams.",
                ),
                adds(
                    Command,
                    "/diagram",
                    "Asks an agent for a diagram.",
                ),
            ],
            asks: vec![],
            changes: vec![
                change("1.8.2", "Dark diagrams match the panels."),
                change("1.8.0", "Timelines and mind maps."),
            ],
        },
        Contraption {
            id: "almanac".into(),
            name: "Almanac Calendar".into(),
            publisher: seller("Fairweather Clockworks", true),
            version: "3.0.1".into(),
            updated: "2026-10-11".into(),
            price: Price::Monthly(2.0),
            standing: standing(4.6, 410, 12_700),
            summary: "Your calendar in a panel, and agents that can \
                      book time."
                .into(),
            description: "Shows the week from your calendar. Agents \
                          can find a free hour, propose a meeting \
                          and remind you before it begins. Nothing \
                          is booked without your say."
                .into(),
            kind: Kind::Planning,
            adds: vec![
                adds(
                    Panel,
                    "Almanac",
                    "The week, the day and what is next.",
                ),
                adds(
                    Tool,
                    "calendar",
                    "Agents read and propose events.",
                ),
                adds(Command, "/today", "Tells what the day holds."),
            ],
            asks: vec![
                Network("calendar.google.com".into()),
                Notifications,
            ],
            changes: vec![
                change("3.0.1", "Time zones for travelling chiefs."),
                change("3.0.0", "Agents may propose, never book."),
            ],
        },
        Contraption {
            id: "kanban".into(),
            name: "Taskboard".into(),
            publisher: seller("Millpond Works", false),
            version: "1.2.0".into(),
            updated: "2026-08-03".into(),
            price: Price::Once(6.0),
            standing: standing(4.4, 190, 5_300),
            summary: "A kanban board the whole hamlet works from."
                .into(),
            description: "Cards in columns: to do, doing, done. \
                          Agents pick up cards, move them as they \
                          work, and leave a note on each when they \
                          finish."
                .into(),
            kind: Kind::Planning,
            adds: vec![
                adds(Panel, "Taskboard", "Columns of cards to drag."),
                adds(
                    Tool,
                    "board",
                    "Agents take, move and close cards.",
                ),
            ],
            asks: vec![ReadWorkspace, WriteWorkspace],
            changes: vec![
                change("1.2.0", "Swimlanes for each hamlet."),
                change("1.1.0", "Cards can wait on other cards."),
            ],
        },
        Contraption {
            id: "ledger".into(),
            name: "Ledger of Changes".into(),
            publisher: seller("Hearthforge Collective", true),
            version: "0.9.0".into(),
            updated: "2026-10-14".into(),
            price: Price::Free,
            standing: standing(4.8, 520, 19_900),
            summary: "Every change to the workspace, kept with git."
                .into(),
            description: "Keeps the workspace in git and shows its \
                          history as a ledger: who changed what, \
                          and why. Agents commit their own work \
                          with a message, and any day can be taken \
                          back."
                .into(),
            kind: Kind::Data,
            adds: vec![
                adds(
                    Panel,
                    "Ledger",
                    "The history of the workspace.",
                ),
                adds(
                    Tool,
                    "commit",
                    "Agents commit what they changed.",
                ),
                adds(
                    Command,
                    "/undo-day",
                    "Takes back a day's changes.",
                ),
            ],
            asks: vec![ReadWorkspace, WriteWorkspace, RunCommands],
            changes: vec![
                change("0.9.0", "Branches for each agent."),
                change("0.8.3", "Diffs of pictures, side by side."),
            ],
        },
        Contraption {
            id: "abacus".into(),
            name: "Abacus Sheets".into(),
            publisher: seller("Ledgerlight", false),
            version: "1.0.4".into(),
            updated: "2026-07-22".into(),
            price: Price::Once(4.0),
            standing: standing(4.3, 140, 4_100),
            summary: "Opens spreadsheets and CSV files in a panel."
                .into(),
            description: "Reads .csv, .tsv and .xlsx files from the \
                          workspace into a table you can sort and \
                          filter, and lets agents read a sheet by \
                          its columns."
                .into(),
            kind: Kind::Data,
            adds: vec![
                adds(Panel, "Sheets", "A table of any sheet."),
                adds(Tool, "sheet", "Agents read rows and columns."),
            ],
            asks: vec![ReadWorkspace],
            changes: vec![change(
                "1.0.4",
                "Opens sheets of a million rows.",
            )],
        },
        Contraption {
            id: "voice".into(),
            name: "Voices of the Hamlet".into(),
            publisher: seller("Bellwether Sound", true),
            version: "2.1.0".into(),
            updated: "2026-09-30".into(),
            price: Price::Monthly(5.0),
            standing: standing(4.5, 300, 7_600),
            summary: "Gives each agent a voice of its own.".into(),
            description: "Agents read their replies aloud, each in \
                          a voice that suits its character, and you \
                          may answer by speaking."
                .into(),
            kind: Kind::Language,
            adds: vec![
                adds(Tool, "speak", "Agents say a reply aloud."),
                adds(
                    Command,
                    "/listen",
                    "Takes what you say as a message.",
                ),
            ],
            asks: vec![
                Speakers,
                Microphone,
                Network("voice.bellwether.example".into()),
            ],
            changes: vec![
                change("2.1.0", "The mayor sounds grander."),
                change("2.0.0", "Speak back to your agents."),
            ],
        },
        Contraption {
            id: "babel".into(),
            name: "Babel Translator".into(),
            publisher: seller("Tidewater Tools", true),
            version: "1.5.1".into(),
            updated: "2026-06-08".into(),
            price: Price::Monthly(3.0),
            standing: standing(4.2, 95, 3_800),
            summary: "Translates the chat and notes as you read \
                      them."
                .into(),
            description: "Shows any message or note in the language \
                          you choose, and lets agents work in one \
                          language while you read in another."
                .into(),
            kind: Kind::Language,
            adds: vec![
                adds(
                    InChat,
                    "Translate",
                    "A message in your language.",
                ),
                adds(Tool, "translate", "Agents translate a text."),
            ],
            asks: vec![Network("api.babel.example".into())],
            changes: vec![change("1.5.1", "Forty more languages.")],
        },
        Contraption {
            id: "cartographer".into(),
            name: "Cartographer's Tiles".into(),
            publisher: seller("Compass Rose Guild", false),
            version: "0.6.0".into(),
            updated: "2026-09-05".into(),
            price: Price::Free,
            standing: standing(4.0, 60, 2_200),
            summary: "Maps of the real world, for places agents \
                      talk of."
                .into(),
            description: "A map panel of OpenStreetMap tiles. \
                          Agents pin the places they mention, draw \
                          a route, and look up what is nearby."
                .into(),
            kind: Kind::Maps,
            adds: vec![
                adds(Panel, "Map", "Tiles of the world, with pins."),
                adds(
                    Tool,
                    "map",
                    "Agents pin places and draw routes.",
                ),
            ],
            asks: vec![Network("tile.openstreetmap.org".into())],
            changes: vec![change(
                "0.6.0",
                "Routes on foot and by bicycle.",
            )],
        },
        Contraption {
            id: "lichen".into(),
            name: "Lantern & Lichen Themes".into(),
            publisher: seller("Inkwell Atelier", true),
            version: "1.3.0".into(),
            updated: "2026-10-12".into(),
            price: Price::Once(3.0),
            standing: standing(4.8, 230, 9_100),
            summary: "Three new looks for the panels.".into(),
            description: "Moonlit Harbour in blues and silver, \
                          Harvest Hall in wheat and rust, and \
                          Frostfern in pale green. Each keeps the \
                          lamplight warm."
                .into(),
            kind: Kind::Looks,
            adds: vec![
                adds(Theme, "Moonlit Harbour", "Blues and silver."),
                adds(Theme, "Harvest Hall", "Wheat and rust."),
                adds(Theme, "Frostfern", "Pale green and birch."),
            ],
            asks: vec![],
            changes: vec![change("1.3.0", "Frostfern, for winter.")],
        },
        Contraption {
            id: "crier".into(),
            name: "Town Crier".into(),
            publisher: seller("Fairweather Clockworks", true),
            version: "1.1.0".into(),
            updated: "2026-08-19".into(),
            price: Price::Monthly(1.5),
            standing: standing(4.4, 150, 6_000),
            summary: "Rings your phone when something needs you."
                .into(),
            description: "When an agent waits for your word, the \
                          Town Crier sends it to your phone, and \
                          your answer comes back the same way."
                .into(),
            kind: Kind::Planning,
            adds: vec![
                adds(Command, "/crier", "Chooses what is cried."),
                adds(Tool, "cry", "Agents send you a message."),
            ],
            asks: vec![
                Notifications,
                Network("push.crier.example".into()),
            ],
            changes: vec![change("1.1.0", "Quiet hours at night.")],
        },
        Contraption {
            id: "lore".into(),
            name: "Lore Search".into(),
            publisher: seller("The Thornfield Library", true),
            version: "2.0.0".into(),
            updated: "2026-10-02".into(),
            price: Price::Free,
            standing: standing(4.6, 280, 11_300),
            summary: "Finds any word in every note, chat and file."
                .into(),
            description: "Builds an index of the workspace, the \
                          notes and the chats, and finds a word \
                          among them as fast as you type it."
                .into(),
            kind: Kind::Data,
            adds: vec![
                adds(Panel, "Lore", "Search everything at once."),
                adds(
                    Tool,
                    "recall",
                    "Agents search what was written.",
                ),
                adds(
                    Command,
                    "/find",
                    "Searches from the command line.",
                ),
            ],
            asks: vec![ReadWorkspace],
            changes: vec![
                change("2.0.0", "Searches the chats too."),
                change("1.4.0", "Finds words by their meaning."),
            ],
        },
    ]
}
