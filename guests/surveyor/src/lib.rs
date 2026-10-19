//! A plugin that surveys the realm: it lists the hamlets and the
//! agents and where each stands, marks the agents at work over the
//! world with a line to their hamlet, and brings the camera to an
//! agent chosen in its panel. Each is behind a grant, and what came
//! of the last ask is a line of the panel.

use hamlet_guest::draw::theme;
use hamlet_guest::overlay::{
    self, Mark, agent, circle, hamlet, line, text,
};
use hamlet_guest::ui::{
    View, button, column, divider, heading, label, note, row,
};
use hamlet_guest::world::{self, WorldView};
use hamlet_guest::{
    Incoming, Intent, Level, Plugin, RequestResult, State,
};
use serde::{Deserialize, Serialize};

const PANEL: &str = "main";

const READ: &str = "read.world";

/// The agents at work marked over the world at most.
const MARKED: usize = 12;

/// What the panel shows of the world.
#[derive(Default, PartialEq, Serialize, Deserialize)]
struct Seen {
    hour: String,
    hamlets: Vec<String>,
    /// Each agent's name and a line about it.
    agents: Vec<(String, String)>,
    selected: String,
}

/// What the plugin keeps between calls, which the host holds across a
/// reload.
#[derive(Default, Serialize, Deserialize)]
struct Model {
    /// The chief allowed `read.world`.
    reading: bool,
    /// The chief refused it.
    refused: bool,
    seen: Seen,
    /// The agents at work last marked, each with its hamlet.
    marked: Vec<(String, String)>,
    /// What came of the last thing asked.
    line: String,
}

struct Surveyor;

impl Plugin for Surveyor {
    fn entry() {
        hamlet_guest::panel(PANEL, "SURVEYOR", "globe");
        // The host took the marks away when the plugin stopped, so
        // they are sent again.
        let mut model = State::<Model>::load();
        model.marked.clear();
        if let Err(why) = State::save(&model) {
            hamlet_guest::log(Level::Warn, &why);
        }
    }

    fn step(inbox: Vec<Incoming>) -> Vec<Intent> {
        let mut model = State::<Model>::load();
        let mut out = Vec::new();
        for incoming in inbox {
            handle(incoming, &mut model, &mut out);
        }
        survey(&mut model, &mut out);
        if let Err(why) = State::save(&model) {
            hamlet_guest::log(Level::Warn, &why);
        }
        out
    }

    fn panel_view(_panel: &str) -> View {
        let model = State::<Model>::load();
        let seen = &model.seen;
        let mut rows = vec![heading("SURVEYOR")];
        if !model.reading {
            rows.push(note(
                "Waiting for the chief to allow read.world.",
            ));
        } else {
            rows.push(note(&seen.hour).id("hour"));
            rows.extend(seen.hamlets.iter().map(|line| label(line)));
            rows.push(divider());
            rows.extend(seen.agents.iter().map(|(name, line)| {
                row([
                    label(line),
                    button("Focus").id(&format!("focus.{name}")),
                ])
                .id(&format!("agent.{name}"))
            }));
            rows.push(note(&seen.selected).id("selected"));
        }
        if !model.line.is_empty() {
            rows.push(note(&model.line).id("line"));
        }
        column(rows).into()
    }
}

fn handle(
    incoming: Incoming,
    model: &mut Model,
    out: &mut Vec<Intent>,
) {
    match incoming {
        Incoming::Click(press) => {
            if let Some(name) = press.id.strip_prefix("focus.") {
                out.push(world::focus(world::agent(name)));
                model.line =
                    format!("Asked to bring the camera to {name}.");
                out.push(Intent::Redraw(PANEL.into()));
            }
        }
        Incoming::Granted(name) => {
            model.reading |= name == READ;
            // Marks refused before are sent again.
            model.marked.clear();
            model.line = format!("The chief allowed {name}.");
            out.push(Intent::Redraw(PANEL.into()));
        }
        Incoming::Denied(name) => {
            model.refused |= name == READ;
            model.line = format!("The chief refused {name}.");
            out.push(Intent::Redraw(PANEL.into()));
        }
        _ => {}
    }
}

/// Reads the world, while the chief allows it, and sends the marks
/// and draws the panel again only where they changed.
fn survey(model: &mut Model, out: &mut Vec<Intent>) {
    if !model.reading && !model.refused {
        let why = "To list the hamlets and the agents.";
        model.reading = hamlet_guest::request(READ, why)
            == RequestResult::Granted;
    }
    if !model.reading {
        return;
    }
    let Ok(view) = world::view() else {
        // The chief took it back.
        model.reading = false;
        out.push(Intent::Redraw(PANEL.into()));
        return;
    };
    let busy = view
        .agents
        .iter()
        .filter(|agent| agent.busy)
        .take(MARKED)
        .map(|agent| (agent.name.clone(), agent.hamlet.clone()))
        .collect::<Vec<_>>();
    if busy != model.marked {
        out.push(overlay::draw(marks(&busy)));
        model.marked = busy;
    }
    let seen = seen(&view);
    if seen != model.seen {
        model.seen = seen;
        out.push(Intent::Redraw(PANEL.into()));
    }
}

/// A lamp over each agent at work, its name beside it and a line down
/// to the common of its hamlet. The host keeps each where its agent
/// is seen, so they are sent again only when who works changes.
fn marks(busy: &[(String, String)]) -> Vec<Mark> {
    busy.iter()
        .flat_map(|(name, home)| {
            [
                line(
                    agent(name),
                    (0.0, -30.0),
                    Some(hamlet(home)),
                    (0.0, 0.0),
                    1.5,
                    theme("seam"),
                ),
                circle(agent(name), 0.0, -40.0, 5.0, theme("lamp")),
                text(
                    agent(name),
                    9.0,
                    -48.0,
                    name,
                    12.0,
                    theme("ink"),
                ),
            ]
        })
        .collect()
}

/// What the panel shows of `view`.
fn seen(view: &WorldView) -> Seen {
    let hour = view.hour;
    let (h, m) = (hour as u32, (hour.fract() * 60.0).round() as u32);
    let at = |at: &Option<world::Place>| match at {
        Some(at) => format!("at {:.0}, {:.0}", at.x, at.z),
        None => "out of sight".to_string(),
    };
    Seen {
        hour: format!("The sky shows {h}:{m:02}."),
        hamlets: view
            .hamlets
            .iter()
            .map(|h| format!("{} ({}), {}", h.name, h.id, at(&h.at)))
            .collect(),
        agents: view
            .agents
            .iter()
            .map(|agent| {
                let doing = if agent.busy { "busy" } else { "idle" };
                let line = format!(
                    "{} of {}, {doing}, {}",
                    agent.name,
                    agent.hamlet,
                    at(&agent.at)
                );
                (agent.name.clone(), line)
            })
            .collect(),
        selected: match &view.selected {
            Some(thing) => {
                format!("Selected: {} {}", thing.kind, thing.name)
            }
            None => "Nothing is selected.".into(),
        },
    }
}

hamlet_guest::export!(Surveyor);
