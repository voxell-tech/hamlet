//! The prompt an agent is told before each model call, in C11's
//! order.

mod brief;
pub(crate) mod shelf;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use anyhow::Result;
use hamlet_runtime_protocol::Section;
use serde_json::Value;

use crate::papers::{Reader, cut};
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

/// A chief's instruction to an agent.
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
    /// The model the conversation runs, as the chief's limits name
    /// it, if any.
    pub model: Option<&'a str>,
    /// The platform sandbox's name, e.g. "bubblewrap".
    pub sandbox: &'a str,
    /// The instructions of the hamlet's agents, in the record's
    /// order of agents and each agent's oldest first.
    pub instructions: &'a [Instruction],
    /// The spend task of the turn (the runtime-scoped submission
    /// id), for the books.
    pub task: Option<&'a str>,
    /// The conversation the prompt is for, if any.
    pub conversation: Option<crate::harness::BindingId>,
}

/// Stocks the library at `root` with the starters it was not
/// offered yet, telling stderr when it cannot.
pub(crate) fn stock(root: &Path) {
    if let Err(error) = crate::starters::install(root) {
        eprintln!("hamlet: could not open the library: {error}");
    }
}

/// Every section of the prompt but the budget, in order, leaving out
/// the empty ones.
#[cfg(test)]
pub(crate) async fn sections(
    service: &mut Service,
    subject: &Subject<'_>,
) -> Result<Vec<Section>> {
    prepare(service, subject)?.render().await
}

/// Prompt metadata captured without retaining the service during file
/// reads.
pub(crate) struct Prepared {
    pub sections: Vec<Section>,
    /// The house as the agent's tools see it.
    house: PathBuf,
    /// The house on this machine, where it is here.
    local: PathBuf,
    work: crate::documents::Work,
    /// Whether the hamlet's folder is a remote one its agents share.
    shared: bool,
    /// The house and folder the notes, skills and project are read
    /// from, unless the sections are fixed.
    reader: Option<Reader>,
}

impl Prepared {
    pub fn fixed(sections: Vec<Section>) -> Self {
        Self {
            sections,
            house: Default::default(),
            local: Default::default(),
            work: Default::default(),
            shared: false,
            reader: None,
        }
    }

    pub fn signature(&self) -> Value {
        serde_json::json!([self.sections, self.work])
    }

    pub async fn render(mut self) -> Result<Vec<Section>> {
        if let Some(reader) = &self.reader {
            let project =
                project(&self.work, &self.house, self.shared, reader)
                    .await;
            for section in &mut self.sections {
                match section.name.as_str() {
                    "project" => {
                        section.text =
                            project.clone().unwrap_or_default()
                    }
                    "skills" => {
                        section.text = shelf::skills_text(
                            &self.house,
                            &reader.skills(&self.local).await,
                        );
                    }
                    "notes" => {
                        section.text = notes_text(
                            &reader.notes().await.unwrap_or_default(),
                        );
                    }
                    _ => {}
                }
            }
        }
        self.sections.retain(|section| !section.text.is_empty());
        Ok(self.sections)
    }
}

pub(crate) fn prepare(
    service: &mut Service,
    subject: &Subject<'_>,
) -> Result<Prepared> {
    let number = u64::try_from(subject.number)?;
    let root = service.home.layout().root().to_owned();
    let local = root
        .join("houses")
        .join(subject.hamlet)
        .join(number.to_string());
    let work = service.work(subject.hamlet)?;
    let reader = service.houses.reader(
        subject.hamlet,
        number,
        work.folder.is_some(),
    )?;
    let house = reader.house(&local);
    let record = service.home.store().record(subject.hamlet)?;
    let agent = record
        .as_ref()
        .and_then(|r| r.agents.iter().find(|a| a.number == number));
    let terms = terms(service, subject.hamlet)?;
    let company = company(service, subject, number)?;
    let parts = [
        ("charter", Some(service.charter()?.0.trim_end().to_owned())),
        ("machine", Some(machine(service, subject, &house)?)),
        ("project", Some(String::new())),
        ("character", agent.and_then(|a| character(&a.character))),
        ("notes", Some(String::new())),
        ("skills", Some(String::new())),
        (
            "library",
            shelf::library_at(
                &root.join("library"),
                &reader.library(&root.join("library")),
            ),
        ),
        (
            "hamlet",
            record.as_ref().and_then(|r| {
                brief::brief(
                    r,
                    number,
                    subject.instructions,
                    &terms,
                    &company,
                )
            }),
        ),
        ("realm", realm_brief(service, subject.hamlet, number)?),
    ];
    let sections = parts
        .into_iter()
        .filter_map(|(name, text)| {
            text.map(|text| Section {
                name: name.into(),
                text,
            })
        })
        .collect();
    Ok(Prepared {
        sections,
        house,
        local,
        work,
        shared: service.houses.machine().is_some(),
        reader: Some(reader),
    })
}

fn realm_brief(
    service: &mut Service,
    hamlet: &str,
    number: u64,
) -> Result<Option<String>> {
    let db = service.home.store().connection();
    let realm = crate::realm::read(db)?;
    if realm.live().count() < 2
        || crate::record::read(db, hamlet)?
            .is_none_or(|r| r.mayor != number)
    {
        return Ok(None);
    }
    let mut lines = vec![
        "# Other hamlets".into(),
        "You have a road to every other mayor: reach one with \
         `ask_agent` as `mayor@<hamlet>`. Whoever asks pays for the \
         answer. Between two hamlets a thing is done only when both \
         mayors agree: `agree_road` for a road, `agree_move` for an \
         agent to move with its house. `ask_removal` asks the chief \
         to remove a hamlet."
            .into(),
    ];
    for place in realm.live().filter(|h| h.id != hamlet) {
        if let Some(r) = crate::record::read(db, &place.id)? {
            lines.push(format!(
                "- {}: mayor `{}`; agents: {}",
                r.id,
                r.name_of(
                    r.mayor,
                    crate::record::NameFallback::Recorded
                ),
                r.agents
                    .iter()
                    .filter(|a| a.number != r.mayor)
                    .map(|a| a.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    Ok(Some(lines.join("\n")))
}

/// Whom the conversation of `subject` is with beyond its hamlet: the
/// agent it is between, and the reader's roads to other hamlets.
fn company(
    service: &mut Service,
    subject: &Subject,
    number: u64,
) -> Result<brief::Company> {
    let db = service.home.store().connection();
    let peer = match subject.conversation {
        Some(binding) => {
            crate::society::letters::sender_of(db, binding)?
        }
        None => None,
    };
    let peer = match peer {
        Some(peer) => crate::record::read(db, &peer.hamlet)?
            .map(|r| r.address_of(peer.number).to_owned())
            .filter(|name| !name.is_empty())
            .map(|name| {
                if peer.hamlet == subject.hamlet {
                    name
                } else {
                    format!("{name}@{}", peer.hamlet)
                }
            }),
        None => None,
    };
    let reader = crate::record::AgentRef {
        hamlet: subject.hamlet.into(),
        number,
    };
    let mut beyond = Vec::new();
    for road in crate::realm::roads(db)? {
        let other = if road.a == reader {
            road.b
        } else if road.b == reader {
            road.a
        } else {
            continue;
        };
        if let Some(there) = crate::record::read(db, &other.hamlet)? {
            beyond.push(if other.number == there.mayor {
                format!("mayor@{}", other.hamlet)
            } else {
                there.address_of(other.number).into()
            });
        }
    }
    Ok(brief::Company { peer, beyond })
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
        let lines = std::iter::once(&report.summary)
            .chain(&report.lines)
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join("\n");
        let road = limits.road.cost;
        let models = limits.models.join(", ");
        return Ok(format!(
            "# Budget\nOnly the chief changes this shared \
             budget.\n{lines}\nA road costs {}. Models you may \
             choose: {}.",
            money(crate::spend::micros(road.max(0.0))),
            if models.is_empty() {
                OWN_MODEL
            } else {
                &models
            }
        ));
    }
    let books = books(
        service.home.store().connection(),
        subject.hamlet,
        &limits.budget.period,
        subject.task,
        crate::clock::now(),
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
    Ok(brief::Terms {
        pass_yes: limits.votes.pass_yes,
        pass_of: limits.votes.pass_of,
        told_days: limits.mayor.told_days,
        now: crate::clock::now(),
    })
}

fn machine(
    service: &mut Service,
    subject: &Subject,
    house: &Path,
) -> Result<String> {
    if let Some(machine) = service.houses.machine() {
        return Ok(machine.into());
    }
    let net = if service.settings()?.allow_net {
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

async fn project(
    work: &crate::documents::Work,
    house: &Path,
    shared: bool,
    reader: &Reader,
) -> Option<String> {
    let folder = reader.folder(work.folder.as_deref()?);
    let mut lines = vec![
        format!(
            "Your hamlet works on the folder {folder}. The work a \
             chief asks for is done there. This changes what is \
             said above about where commands run:"
        ),
        if shared {
            "- `bash` runs in /project, shared with this hamlet's \
             agents. Only /project, your own /house, the read-only \
             shared /library and installed runtime are visible. This \
             is a remote project, not a synchronized folder on the \
             chief's machine."
                .into()
        } else {
            format!(
                "- `bash` runs with {folder} as its working \
                 directory. A command may write there and in your \
                 own workspace, and nowhere else. It can read the \
                 rest of the machine, so the project's own tools \
                 (cargo, npm, make and the like) run as they do for \
                 the chief."
            )
        },
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
    if reader.project_has(folder, ".git").await {
        lines.push(
            "- The project is a git repository. When two agents \
             must work apart, the mayor may have one of them use a \
             git worktree inside the project folder, for example \
             under `.hamlet/worktrees/<name>`: only the project \
             folder and an agent's own workspace can be written."
                .into(),
        );
    }
    if work.mode == crate::documents::Mode::Manual {
        lines.push(
            "- The chief approves each edit and each command before \
             it runs, so each one waits. `read`, `search` and \
             `find` never wait: learn what you need with them, then \
             make few, well-aimed edits and commands."
                .into(),
        );
    }
    for name in INSTRUCTIONS {
        let Some(bytes) = reader.project(folder, name).await else {
            continue;
        };
        let text = String::from_utf8_lossy(&bytes);
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
    Some(
        format!("# The project\n{}", lines.join("\n"))
            .trim_end()
            .into(),
    )
}

fn character(text: &str) -> Option<String> {
    let text = text.trim();
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

fn notes_text(text: &str) -> String {
    let note = format!(
        "\n[NOTES.md is longer than {NOTES_SHOWN} characters and is \
         cut here. Shorten it so that all of it is shown.]"
    );
    format!("# NOTES.md\n{}", cut(text, NOTES_SHOWN, &note))
        .trim_end()
        .into()
}
