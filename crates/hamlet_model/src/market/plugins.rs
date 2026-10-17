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
            Kind::Drawing => "Diagrams",
            Kind::Planning => "Productivity",
            Kind::Data => "Data",
            Kind::Language => "Voice and language",
            Kind::Maps => "Integrations",
            Kind::Looks => "Themes",
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

/// One plugin, as a listing gives it.
#[allow(clippy::too_many_arguments)]
fn plugin(
    (id, name, publisher): (&str, &str, Seller),
    (version, updated, price, standing): (
        &str,
        &str,
        Price,
        Standing,
    ),
    (summary, description): (&str, &str),
    kind: Kind,
    adds: Vec<Addition>,
    asks: Vec<Permission>,
    changes: Vec<Change>,
) -> Contraption {
    Contraption {
        id: id.into(),
        name: name.into(),
        publisher,
        version: version.into(),
        updated: updated.into(),
        price,
        standing,
        summary: summary.into(),
        description: description.into(),
        kind,
        adds,
        asks,
        changes,
    }
}

/// The plugins on offer.
pub fn contraptions() -> Vec<Contraption> {
    use Part::*;
    use Permission::*;
    vec![
        plugin(
            (
                "excalidraw",
                "Excalidraw Whiteboard",
                seller("Mara Lindqvist", true),
            ),
            (
                "2.4.0",
                "2026-10-06",
                Price::Free,
                standing(4.9, 1240, 48_200),
            ),
            (
                "A whiteboard where agents draw diagrams and you \
                 edit them.",
                "Adds an Excalidraw whiteboard panel. Agents can \
                 draw flowcharts, architecture sketches and mind \
                 maps while they work, and you can move, edit or \
                 annotate anything they drew. Boards are saved as \
                 .excalidraw files in the workspace.",
            ),
            Kind::Drawing,
            vec![
                adds(
                    Panel,
                    "Whiteboard",
                    "An Excalidraw canvas docked beside the world.",
                ),
                adds(
                    Tool,
                    "whiteboard.draw",
                    "Lets agents add shapes, arrows and text.",
                ),
                adds(Command, "/whiteboard", "Opens a new board."),
            ],
            vec![ReadWorkspace, WriteWorkspace],
            vec![
                change(
                    "2.4.0",
                    "Agents can turn a table into a chart.",
                ),
                change("2.3.0", "Export boards as PNG or SVG."),
            ],
        ),
        plugin(
            (
                "mermaid",
                "Mermaid Diagrams",
                seller("Diagramming Co.", true),
            ),
            (
                "1.8.2",
                "2026-09-28",
                Price::Free,
                standing(4.7, 860, 61_400),
            ),
            (
                "Renders Mermaid code blocks in chat as diagrams.",
                "Any ```mermaid block an agent writes is drawn as a \
                 diagram right in the chat: flowcharts, sequence \
                 diagrams, Gantt charts, ER diagrams and more. \
                 Click a diagram to copy it as SVG.",
            ),
            Kind::Drawing,
            vec![adds(
                InChat,
                "Mermaid blocks",
                "Draws diagrams instead of code.",
            )],
            vec![],
            vec![change("1.8.2", "Supports Mermaid 11 syntax.")],
        ),
        plugin(
            ("github", "GitHub", seller("Octoplane Labs", true)),
            (
                "3.1.0",
                "2026-10-11",
                Price::Free,
                standing(4.8, 2210, 92_300),
            ),
            (
                "Issues and pull requests in a panel, and GitHub \
                 tools for agents.",
                "Sign in once and see your issues and pull requests \
                 in a panel. Agents can read issues, open branches, \
                 comment on and review pull requests, and check CI \
                 runs. Every write action waits for your approval.",
            ),
            Kind::Maps,
            vec![
                adds(
                    Panel,
                    "Pull requests",
                    "Your open PRs and review requests.",
                ),
                adds(
                    Tool,
                    "github.issues",
                    "Search, read and comment on issues.",
                ),
                adds(
                    Tool,
                    "github.pulls",
                    "Open, review and comment on PRs.",
                ),
                adds(
                    Command,
                    "/pr",
                    "Opens the pull request for this branch.",
                ),
            ],
            vec![
                Network("api.github.com".into()),
                ReadWorkspace,
                RunCommands,
            ],
            vec![
                change("3.1.0", "Agents can read failing CI logs."),
                change("3.0.0", "New pull requests panel."),
            ],
        ),
        plugin(
            ("linear", "Linear", seller("Northbeam Studio", false)),
            (
                "1.2.0",
                "2026-08-30",
                Price::Monthly(3.0),
                standing(4.5, 310, 8_900),
            ),
            (
                "Turn chats into Linear issues and track them from \
                 Hamlet.",
                "Agents can create and update Linear issues, move \
                 them between states and link them to the work they \
                 did. A panel lists the issues assigned to you.",
            ),
            Kind::Planning,
            vec![
                adds(
                    Panel,
                    "My issues",
                    "Your assigned issues by cycle.",
                ),
                adds(
                    Tool,
                    "linear.issue",
                    "Create and update issues.",
                ),
            ],
            vec![Network("api.linear.app".into())],
            vec![change("1.2.0", "Supports sub-issues.")],
        ),
        plugin(
            ("gcal", "Google Calendar", seller("Tempo Tools", true)),
            (
                "2.0.3",
                "2026-09-14",
                Price::Free,
                standing(4.4, 540, 23_700),
            ),
            (
                "Lets agents check your calendar and schedule \
                 meetings.",
                "Agents can see when you are free, draft events and \
                 send invites once you approve. A panel shows today \
                 and the next seven days.",
            ),
            Kind::Planning,
            vec![
                adds(Panel, "Calendar", "Today and the week ahead."),
                adds(
                    Tool,
                    "calendar.events",
                    "Read and draft events.",
                ),
            ],
            vec![Network("www.googleapis.com".into()), Notifications],
            vec![change(
                "2.0.3",
                "Fixes time zones for all-day events.",
            )],
        ),
        plugin(
            ("notion", "Notion", seller("Paperlane", false)),
            (
                "1.5.1",
                "2026-09-02",
                Price::Once(9.0),
                standing(4.3, 280, 11_200),
            ),
            (
                "Search, read and write Notion pages from your \
                 agents.",
                "Connect a Notion workspace and agents can search \
                 pages, read databases and append notes or meeting \
                 summaries to the page you choose.",
            ),
            Kind::Planning,
            vec![
                adds(
                    Tool,
                    "notion.search",
                    "Search pages and databases.",
                ),
                adds(Tool, "notion.append", "Add blocks to a page."),
            ],
            vec![Network("api.notion.com".into())],
            vec![change("1.5.1", "Reads database rows as tables.")],
        ),
        plugin(
            ("sql", "SQL Browser", seller("Rowan Patel", true)),
            (
                "0.9.4",
                "2026-10-01",
                Price::Free,
                standing(4.6, 190, 7_400),
            ),
            (
                "Browse SQLite and Postgres databases, and let \
                 agents query them.",
                "Adds a table browser for SQLite files and Postgres \
                 connections. Agents can run read-only queries by \
                 default; writes need a setting and your approval.",
            ),
            Kind::Data,
            vec![
                adds(
                    Panel,
                    "Databases",
                    "Tables, columns and query results.",
                ),
                adds(Tool, "sql.query", "Run read-only SQL."),
            ],
            vec![ReadWorkspace, Network("your database host".into())],
            vec![change("0.9.4", "Postgres connections over SSH.")],
        ),
        plugin(
            ("csv", "CSV Viewer", seller("Gridline", false)),
            (
                "1.3.0",
                "2026-07-19",
                Price::Free,
                standing(4.5, 420, 19_800),
            ),
            (
                "Opens CSV and Excel files as sortable tables.",
                "Shows CSV, TSV and XLSX files from the workspace \
                 as tables you can sort and filter. Agents can read \
                 a sheet as structured rows instead of raw text.",
            ),
            Kind::Data,
            vec![
                adds(
                    Panel,
                    "Table",
                    "Sort, filter and search a sheet.",
                ),
                adds(Tool, "sheet.read", "Read rows and columns."),
            ],
            vec![ReadWorkspace],
            vec![change("1.3.0", "Opens .xlsx files.")],
        ),
        plugin(
            (
                "browser",
                "Browser Automation",
                seller("Headless Inc.", true),
            ),
            (
                "2.2.0",
                "2026-10-09",
                Price::Monthly(5.0),
                standing(4.2, 650, 15_300),
            ),
            (
                "Gives agents a real browser to click, fill forms \
                 and take screenshots.",
                "Agents get a sandboxed Chromium they can drive: \
                 open pages, click, type, fill forms and take \
                 screenshots. Every site visited is listed for you \
                 to review.",
            ),
            Kind::Maps,
            vec![
                adds(Tool, "browser.open", "Open and read a page."),
                adds(
                    Tool,
                    "browser.act",
                    "Click, type and submit forms.",
                ),
                adds(
                    Panel,
                    "Browser",
                    "Watch what the agent is doing.",
                ),
            ],
            vec![Network("any site".into()), RunCommands],
            vec![change("2.2.0", "Screenshots of the whole page.")],
        ),
        plugin(
            ("clipper", "Web Clipper", seller("Saved Pages", false)),
            (
                "1.0.6",
                "2026-06-22",
                Price::Free,
                standing(4.1, 130, 5_600),
            ),
            (
                "Save web pages as clean Markdown for your agents \
                 to read.",
                "Paste a link and the page is saved to the \
                 workspace as Markdown, without ads or menus, with \
                 its title, author and date.",
            ),
            Kind::Data,
            vec![adds(Command, "/clip", "Save a page as Markdown.")],
            vec![Network("any site".into()), WriteWorkspace],
            vec![change("1.0.6", "Keeps images as links.")],
        ),
        plugin(
            ("voice", "Voice Mode", seller("Hush Audio", true)),
            (
                "1.4.2",
                "2026-09-25",
                Price::Monthly(4.0),
                standing(4.4, 380, 12_100),
            ),
            (
                "Talk to your agents and hear their replies.",
                "Hold a key to speak to the agent you are chatting \
                 with, and have replies read aloud. Speech is \
                 transcribed on this computer; only the text is \
                 sent to the model.",
            ),
            Kind::Language,
            vec![
                adds(Command, "/talk", "Start talking to the agent."),
                adds(
                    InChat,
                    "Read aloud",
                    "A play button on each reply.",
                ),
            ],
            vec![Microphone, Speakers],
            vec![change("1.4.2", "Lower latency on Apple silicon.")],
        ),
        plugin(
            ("slack", "Slack Bridge", seller("Relay Works", true)),
            (
                "2.1.1",
                "2026-10-03",
                Price::Monthly(6.0),
                standing(4.3, 470, 9_800),
            ),
            (
                "Chat with your agents from Slack.",
                "Mention an agent in a Slack channel or DM it, and \
                 the conversation continues in Hamlet. Agents can \
                 post summaries back to a channel you pick.",
            ),
            Kind::Maps,
            vec![
                adds(
                    Tool,
                    "slack.post",
                    "Post a message to a channel.",
                ),
                adds(
                    Command,
                    "/slack",
                    "Link a channel to an agent.",
                ),
            ],
            vec![Network("slack.com".into()), Notifications],
            vec![change("2.1.1", "Threads stay in one chat.")],
        ),
        plugin(
            ("nord", "Nord Theme", seller("Arctic Ice Studio", true)),
            (
                "1.1.0",
                "2026-05-11",
                Price::Free,
                standing(4.8, 900, 34_000),
            ),
            (
                "The cool, calm Nord colour palette for Hamlet's \
                 panels.",
                "Recolours the panels in the Nord palette, with \
                 light and dark variants.",
            ),
            Kind::Looks,
            vec![adds(Theme, "Nord", "Light and dark.")],
            vec![],
            vec![change("1.1.0", "Adds a light variant.")],
        ),
    ]
}
