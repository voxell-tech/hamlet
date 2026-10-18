//! The prompt an agent is told before each model call, in C11's
//! order, as the TypeScript server composes it.

mod brief;
mod shelf;
#[cfg(test)]
mod tests;

use std::path::Path;

use anyhow::Result;
use serde_json::Value;

use crate::harness::link::Section;
use crate::papers::cut;
use crate::record::trim;
use crate::service::Service;
use crate::spend::{books, money};

/// The machine's shell tools, as the prompt names them.
const SYSTEM: &str = if cfg!(target_os = "macos") {
    "System: macOS, whose shell tools are the BSD ones. Flags that \
     only GNU has, such as `grep -P`, `cat -A` and `sed -r`, fail."
} else {
    "System: Linux."
};

/// Characters of NOTES.md, and of a project's own instructions, a
/// prompt shows.
const NOTES_SHOWN: usize = 8000;

/// Characters of a character a prompt shows.
const CHARACTER_SHOWN: usize = 4000;

/// The files that hold a project's own instructions for agents, the
/// first that reads winning.
const INSTRUCTIONS: [&str; 2] = ["AGENTS.md", "CLAUDE.md"];

/// The words for the model of an agent that runs the hamlet's
/// default.
const OWN_MODEL: &str = "the hamlet's own model";

/// A chief's instruction to an agent, as `chats/instructions.ts`
/// keeps it.
pub(crate) struct Instruction {
    /// The number of the agent instructed.
    pub agent: i64,
    pub chief: String,
    pub text: String,
    pub at: i64,
}

/// The agent a prompt is for, and what of its conversation the
/// prompt tells.
pub(crate) struct Subject<'a> {
    pub hamlet: &'a str,
    pub number: i64,
    /// The model the conversation runs, as `modelName` names it in
    /// TS, if any.
    pub model: Option<&'a str>,
    /// The platform sandbox's name, e.g. "bubblewrap".
    pub sandbox: &'a str,
    /// The instructions of the hamlet's agents, in the record's
    /// order of agents and each agent's oldest first.
    pub instructions: &'a [Instruction],
    /// The spend task of the turn (the runtime-scoped submission
    /// id), for the books.
    pub task: Option<&'a str>,
}

/// Stocks the library at `root` with the starters it was not
/// offered yet, telling stderr when it cannot.
pub(crate) fn stock(root: &Path) {
    if let Err(error) = hamlet_home::starters::install(root) {
        eprintln!("hamlet: could not open the library: {error}");
    }
}

/// Every section of the prompt but the budget, in order, leaving out
/// the empty ones.
pub(crate) fn sections(
    service: &mut Service,
    subject: &Subject,
) -> Result<Vec<Section>> {
    let number = u64::try_from(subject.number)?;
    let root = service.home.layout().root().to_owned();
    let house = root
        .join("houses")
        .join(subject.hamlet)
        .join(number.to_string());
    let record = service.home.store().record(subject.hamlet)?;
    let agent = record
        .as_ref()
        .and_then(|r| r.agents.iter().find(|a| a.number == number));
    let terms = terms(service, subject.hamlet)?;
    let parts = [
        ("charter", Some(trim_end(&service.charter()?.0).to_owned())),
        ("machine", Some(machine(service, subject, &house)?)),
        ("project", project(service, subject.hamlet, &house)?),
        ("character", agent.and_then(|a| character(&a.character))),
        ("notes", Some(notes(service, subject.hamlet, number))),
        ("skills", Some(shelf::skills(&house))),
        ("library", shelf::library(&root.join("library"))),
        (
            "hamlet",
            record.as_ref().and_then(|r| {
                brief::brief(r, number, subject.instructions, &terms)
            }),
        ),
    ];
    Ok(parts
        .into_iter()
        .filter_map(|(name, text)| {
            text.filter(|t| !t.is_empty()).map(|text| Section {
                name: name.into(),
                text,
            })
        })
        .collect())
}

/// The budget section's words, computed now.
pub(crate) fn budget(
    service: &mut Service,
    subject: &Subject,
) -> Result<String> {
    let number = u64::try_from(subject.number)?;
    let mayor = service
        .home
        .store()
        .record(subject.hamlet)?
        .is_some_and(|r| r.mayor == number);
    let limits = service.limits_of(subject.hamlet)?;
    if mayor {
        let report =
            service.report(subject.hamlet, Some(OWN_MODEL))?;
        let lines = std::iter::once(&report["summary"])
            .chain(report["lines"].as_array().into_iter().flatten())
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join("\n");
        let road =
            limits["road"]["cost"].as_f64().unwrap_or_default();
        let models = limits["models"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        return Ok(format!(
            "# Budget\nOnly the chief changes this shared \
             budget.\n{lines}\nA road costs {}. Models you may \
             choose: {}.",
            money((road.max(0.0) * 1_000_000.0).round() as i64),
            if models.is_empty() {
                OWN_MODEL
            } else {
                &models
            }
        ));
    }
    let period = limits["budget"]["period"].as_str().unwrap_or("day");
    let books = books(
        service.home.store().connection(),
        subject.hamlet,
        period,
        subject.task,
        now(),
    )?;
    Ok(format!(
        "# Budget\nEveryone in the hamlet spends from its one \
         budget. Your spend: {}.",
        money(books.agents.get(&number).map_or(0, |a| a.micros))
    ))
}

/// The whole prompt as the model reads it.
/// The folders of the skills in `house`, as a client lists them.
pub(crate) fn skill_names(house: &Path) -> Vec<String> {
    shelf::names(house)
}

pub(crate) fn text(sections: &[Section]) -> String {
    sections
        .iter()
        .map(|s| s.text.as_str())
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn terms(
    service: &mut Service,
    hamlet: &str,
) -> Result<brief::Terms> {
    let limits = service.limits_of(hamlet)?;
    let defaults = service.limits.defaults();
    let whole = |pointer| {
        limits
            .pointer(pointer)
            .or_else(|| defaults.pointer(pointer))
            .and_then(Value::as_i64)
            .unwrap_or_default()
    };
    Ok(brief::Terms {
        pass_yes: whole("/votes/passYes"),
        pass_of: whole("/votes/passOf"),
        told_days: whole("/mayor/toldDays"),
        now: now(),
    })
}

fn machine(
    service: &mut Service,
    subject: &Subject,
    house: &Path,
) -> Result<String> {
    let settings = service.settings()?;
    let net = if settings["allowNet"] == true {
        "on"
    } else {
        "off unless a chief allows it; a command that needs it is \
         held until the chief answers, so set `network` to true on \
         it and carry on"
    };
    let model = subject
        .model
        .map(|m| format!(" Model: {m}."))
        .unwrap_or_default();
    let faults = crate::hand::load(service.home.layout().root()).1;
    let failed = if faults.is_empty() {
        String::new()
    } else {
        format!("\nTools that failed to load:\n{}", faults.join("\n"))
    };
    Ok(format!(
        "# Environment\nSandbox: {}. Network for tools: \
         {net}.{model}{failed}\n{SYSTEM}\nWorkspace: {}",
        subject.sandbox,
        house.display()
    ))
}

fn project(
    service: &mut Service,
    hamlet: &str,
    house: &Path,
) -> Result<Option<String>> {
    let work = service.work(hamlet)?;
    let Some(folder) = work["folder"].as_str() else {
        return Ok(None);
    };
    let mut lines = vec![
        format!(
            "Your hamlet works on the folder {folder}. The work a \
             chief asks for is done there. This changes what is \
             said above about where commands run:"
        ),
        format!(
            "- `bash` runs with {folder} as its working directory. \
             A command may write there and in your own workspace, \
             and nowhere else. It can read the rest of the machine, \
             so the project's own tools (cargo, npm, make and the \
             like) run as they do for the chief."
        ),
        "- `read`, `write` and `edit` take a path relative to the \
         project, or a full path."
            .into(),
        "- `read`, `search` and `find` look at files without \
         running a command. Use them rather than cat, grep and find."
            .into(),
        format!(
            "- Your own workspace is {}. NOTES.md and skills/ are \
             there and stay there: never put notes, skills or files \
             of your own in the project. Give `write` and `edit` \
             their full path, and run a skill from your workspace, \
             as shown below.",
            house.display()
        ),
    ];
    let folder = Path::new(folder);
    // The project is read on the host here, and through the agent's
    // sandbox by the TypeScript server.
    if folder.join(".git").exists() {
        lines.push(
            "- The project is a git repository. When two agents \
             must work apart, the mayor may have one of them use a \
             git worktree inside the project folder, for example \
             under `.hamlet/worktrees/<name>`: only the project \
             folder and an agent's own workspace can be written."
                .into(),
        );
    }
    if work["mode"] == "manual" {
        lines.push(
            "- The chief approves each edit and each command before \
             it runs, so each one waits. `read`, `search` and \
             `find` never wait: learn what you need with them, then \
             make few, well-aimed edits and commands."
                .into(),
        );
    }
    for name in INSTRUCTIONS {
        let Ok(text) = std::fs::read_to_string(folder.join(name))
        else {
            continue;
        };
        let note = format!(
            "\n[{name} is longer than {NOTES_SHOWN} characters and \
             is cut here.]"
        );
        lines.push(format!(
            "\n# The project's {name}\n{}",
            cut(&text, NOTES_SHOWN, &note)
        ));
        break;
    }
    Ok(Some(
        trim_end(&format!("# The project\n{}", lines.join("\n")))
            .into(),
    ))
}

fn character(text: &str) -> Option<String> {
    let text = trim(text);
    let note = format!(
        "\n[The character is longer than {CHARACTER_SHOWN} \
         characters and is cut here.]"
    );
    (!text.is_empty()).then(|| {
        format!(
            "# Your character\n{}",
            cut(text, CHARACTER_SHOWN, &note)
        )
    })
}

fn notes(service: &Service, hamlet: &str, number: u64) -> String {
    let text =
        service.houses.read(hamlet, number).unwrap_or_default();
    let note = format!(
        "\n[NOTES.md is longer than {NOTES_SHOWN} characters and is \
         cut here. Shorten it so that all of it is shown.]"
    );
    trim_end(&format!(
        "# NOTES.md\n{}",
        cut(&text, NOTES_SHOWN, &note)
    ))
    .into()
}

/// JavaScript's `trimEnd`.
fn trim_end(text: &str) -> &str {
    text.trim_end_matches(|c: char| {
        (c.is_whitespace() && c != '\u{85}') || c == '\u{feff}'
    })
}

fn now() -> i64 {
    chrono::Local::now().timestamp_millis()
}
