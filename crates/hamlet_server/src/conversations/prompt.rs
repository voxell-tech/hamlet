//! The prompt, built before every model call (C11).

mod place;

use std::path::Path;

use anyhow::Result;
use diesel::prelude::*;
use hamlet_git::{MAIN, Repository};
use hamlet_protocol::client::Confinement;
use hamlet_protocol::runtime::Section;
use hamlet_store::Db;
use hamlet_store::schema::document;

use crate::conversations::Conversation;
use crate::core::Core;
use crate::knowledge::folder::Folder;
use crate::knowledge::library::field;
use crate::knowledge::{self, CHARACTER};
use crate::realm::change::Done;
use crate::realm::layout::Layout;
use crate::realm::settings::Settings;
use crate::society::access::Access;
use crate::society::agents::{self, AgentKind};

/// The charter every agent is given unless a chief wrote its own
/// (C12).
const CHARTER: &str = include_str!("../../assets/charter.md");

/// The machine's shell tools, as the prompt names them.
const SYSTEM: &str = if cfg!(target_os = "macos") {
    "System: macOS, whose shell tools are the BSD ones. Flags that \
     only GNU has, such as `grep -P`, `cat -A` and `sed -r`, fail."
} else if cfg!(windows) {
    "System: Windows. `powershell` runs PowerShell. `bash`, where it \
     is offered, is Git Bash, which writes C:\\Users as /c/Users."
} else {
    "System: Linux."
};

/// Characters of the memory, all its root files together, and of a
/// project's own instructions, a prompt shows.
const MEMORY_SHOWN: usize = 8000;
/// Characters of a character a prompt shows.
const CHARACTER_SHOWN: usize = 4000;
/// The files that hold a project's own instructions for agents, the
/// first that reads winning.
const INSTRUCTIONS: [&str; 2] = ["AGENTS.md", "CLAUDE.md"];

/// What a prompt reads besides the database.
pub(crate) struct Context<'a> {
    pub layout: &'a Layout,
    pub settings: &'a Settings,
    pub confinement: &'a Confinement,
    /// The tools added by hand that failed to load.
    pub faults: &'a [String],
}

/// The sections of `conversation`'s next call.
pub(crate) fn sections(
    db: &mut Db,
    cx: &Context<'_>,
    conversation: &Conversation,
) -> Result<Vec<Section>> {
    if let Some(instructions) = &conversation.instructions {
        return Ok(vec![section("errand", instructions.clone())]);
    }
    let agent = agents::get(db, conversation.agent)?;
    let worktree = match agent.kind {
        AgentKind::Worker => cx.layout.worker(agent.id),
        _ => knowledge::worktree(db, cx.layout, agent.id)?.path,
    };
    let at = crate::calls::place::of(db, cx.layout, agent.id)?;
    let full = at.access.is_full();
    let folders = at.place.folder.as_ref().map(|first| {
        let all = std::iter::once(first)
            .chain(&at.place.also)
            .map(|folder| folder.display().to_string())
            .collect::<Vec<_>>();
        (all, at.access)
    });
    let mut sections = vec![
        section("charter", charter(db)?),
        section(
            "machine",
            machine(cx, conversation, &worktree, full)?,
        ),
    ];
    if let Some((paths, access)) = &folders {
        sections.push(section(
            "project",
            project(paths, *access, &worktree),
        ));
    }
    if agent.kind != AgentKind::Worker {
        if let Some(text) = character(db, cx.layout, agent.id)? {
            sections.push(section("character", text));
        }
        sections.push(section("memory", memory(&worktree)));
        sections.push(section(
            "skills",
            skills(&worktree, &cx.layout.library()),
        ));
    }
    sections.push(section("place", place::place(db, conversation)?));
    if let Some(text) = place::realm(db, agent.id)? {
        sections.push(section("realm", text));
    }
    if let Some(text) = place::profiles(db, agent.id)? {
        sections.push(section("profiles", text));
    }
    sections.retain(|section| !section.text.trim().is_empty());
    Ok(sections)
}

fn section(name: &str, text: String) -> Section {
    Section {
        name: name.into(),
        text,
    }
}

/// The realm's charter: the chief's own, else the default.
pub(crate) fn charter(db: &mut Db) -> Result<String> {
    Ok(document::table
        .find("charter")
        .select(document::value)
        .first::<String>(db)
        .optional()?
        .unwrap_or_else(|| CHARTER.to_owned()))
}

impl Core<'_, '_> {
    /// Keeps the realm's charter, or none when `text` is blank.
    pub(crate) fn edit_charter(
        &mut self,
        text: Option<String>,
    ) -> Done<String> {
        let chief = self.realm.chief();
        let db = self.realm.store().connection();
        match text.filter(|t| !t.trim().is_empty()) {
            Some(text) => {
                diesel::replace_into(document::table)
                    .values((
                        document::kind.eq("charter"),
                        document::value.eq(&text),
                        document::at.eq(crate::app::clock::now()),
                        document::chief.eq(chief),
                    ))
                    .execute(db)?;
            }
            None => {
                diesel::delete(document::table.find("charter"))
                    .execute(db)?;
            }
        }
        self.publish();
        Ok(Ok("The charter is saved.".into()))
    }
}

fn machine(
    cx: &Context<'_>,
    conversation: &Conversation,
    worktree: &Path,
    full: bool,
) -> Result<String> {
    // A remote workspace keeps its own policy, even under full
    // access.
    let sandbox = match cx.confinement {
        Confinement::Remote => "the remote workspace's",
        _ if full => {
            "none. A chief gave you full access: your commands are \
             not confined and reach whatever the chief's account can"
        }
        Confinement::Insecure => {
            "none. Commands are not confined: they reach whatever \
             the chief's account can"
        }
        Confinement::Sandbox { name } => name,
    };
    let net = if full {
        "open".to_owned()
    } else if cx.settings.allow_net {
        "on".to_owned()
    } else {
        "off unless a chief allows it; a command that needs it waits \
         until the chief answers, so set `network` to true on it and \
         carry on"
            .to_owned()
    };
    let model = conversation
        .model()
        .map(|m| format!(" Model: {}::{}.", m.provider, m.id))
        .unwrap_or_default();
    let failed = if cx.faults.is_empty() {
        String::new()
    } else {
        format!(
            "\nTools that failed to load:\n{}",
            cx.faults.join("\n")
        )
    };
    Ok(format!(
        "# Environment\nSandbox: {sandbox}. Network for tools: \
         {net}.{model}{failed}\n{SYSTEM}\nYour worktree: {}. It is \
         yours: your memory/ and skills/ live there, and what you \
         write there is kept for your profile.",
        worktree.display()
    ))
}

fn project(
    folders: &[String],
    access: Access,
    worktree: &Path,
) -> String {
    let folder = &folders[0];
    let mut lines = vec![
        match folders {
            [_] => format!(
                "Your hamlet works on the folder {folder}. The work \
                 a chief requests is done there:"
            ),
            _ => format!(
                "Your hamlet works on these folders: {}. The first, \
                 {folder}, is the project. The work a chief \
                 requests is done in them:",
                folders.join(", ")
            ),
        },
        format!(
            "- A command runs with {folder} as its working \
             directory. It may write there{} and in your worktree, \
             and nowhere else. It can read the rest of the machine, \
             so the project's own tools run as they do for the \
             chief.",
            if folders.len() > 1 {
                ", in the other folders"
            } else {
                ""
            }
        ),
        "- `read`, `write` and `edit` take a path relative to the \
         project, or a full path, which is how you reach the other \
         folders. `read`, `search` and `find` look at files without \
         running a command; use them rather than cat, grep and find."
            .into(),
        format!(
            "- Your worktree is {}. Keep your memory and skills \
             there, never in the project, and give `write` and \
             `edit` their full path.",
            worktree.display()
        ),
        "- A folder outside these is not yours to touch. If the \
         work needs one, call `propose_folder` with its path and \
         your reason: a chief answers, and it is added only if they \
         agree."
            .into(),
    ];
    if Path::new(folder).join(".git").exists() {
        lines.push(
            "- The project is a git repository. When two agents \
             must work apart, one may make a git worktree of the \
             project inside the project folder, such as under \
             `.hamlet/worktrees/<name>`."
                .into(),
        );
    }
    match access {
        Access::Supervised => lines.push(
            "- A chief approves each edit and each command in the \
             project before it runs, so each one waits. `read`, \
             `search` and `find` never wait: learn what you need \
             with them, then make few, well-aimed edits and \
             commands."
                .into(),
        ),
        Access::AcceptEdits => lines.push(
            "- Your edits in the project run at once. A chief \
             approves each command before it runs, so each one \
             waits: prefer `read`, `search`, `find` and `edit`, and \
             make few, well-aimed commands."
                .into(),
        ),
        Access::Auto | Access::Full => {}
    }
    if let Ok(project) = Folder::open(Path::new(folder.as_str())) {
        for name in INSTRUCTIONS {
            let Ok(bytes) = project.read(name) else {
                continue;
            };
            let text = String::from_utf8_lossy(&bytes);
            lines.push(format!(
                "\n# The project's {name}\n{}",
                cut(
                    &text,
                    MEMORY_SHOWN,
                    &format!("[{name} is cut here.]")
                )
            ));
            break;
        }
    }
    format!("# The project\n{}", lines.join("\n"))
}

/// The character at the tip of the agent's branch, else its profile's
/// `main`.
fn character(
    db: &mut Db,
    layout: &Layout,
    agent: hamlet_store::ids::AgentId,
) -> Result<Option<String>> {
    let worktree = knowledge::worktree(db, layout, agent)?;
    let Ok(repository) =
        Repository::open(&layout.repository(worktree.profile))
    else {
        return Ok(None);
    };
    let tip = match repository.tip(&knowledge::branch(agent))? {
        Some(tip) => Some(tip),
        None => repository.tip(MAIN)?,
    };
    let Some(tip) = tip else {
        return Ok(None);
    };
    let Some(bytes) = repository.read(tip, CHARACTER)? else {
        return Ok(None);
    };
    let text = String::from_utf8_lossy(&bytes);
    let text = text.trim();
    Ok((!text.is_empty()).then(|| {
        format!(
            "# Your character\n{}",
            cut(
                text,
                CHARACTER_SHOWN,
                "[The character is cut here.]"
            )
        )
    }))
}

/// The memory at the worktree's `memory/`: its root files, then each
/// folder's description.
fn memory(worktree: &Path) -> String {
    let Ok(folder) = Folder::open(worktree) else {
        return String::new();
    };
    let Ok(listing) = folder.list_under("memory") else {
        return String::new();
    };
    let mut files = Vec::new();
    let mut folders = Vec::new();
    for entry in listing {
        let Some(rest) = entry.path.strip_prefix("memory/") else {
            continue;
        };
        match rest.split_once('/') {
            None => {
                if let Ok(bytes) = folder.read(&entry.path) {
                    files.push((
                        rest.to_owned(),
                        String::from_utf8_lossy(&bytes).into_owned(),
                    ));
                }
            }
            Some((name, "MEMORY.md")) => {
                let description = folder
                    .read(&entry.path)
                    .map(|bytes| {
                        field(
                            &String::from_utf8_lossy(&bytes),
                            "description:",
                        )
                    })
                    .unwrap_or_default();
                folders.push((name.to_owned(), description));
            }
            Some(_) => {}
        }
    }
    let mut text = String::from(
        "# Your memory\nWhat you keep in memory/ is shown here \
         before each call: its files, then each folder's \
         description, whose `MEMORY.md` you read when you need it. \
         Keep it short and true.\n",
    );
    if files.is_empty() && folders.is_empty() {
        text.push_str("(empty)");
        return text;
    }
    let mut shown = String::new();
    for (name, body) in files {
        shown.push_str(&format!(
            "\n## memory/{name}\n{}\n",
            body.trim()
        ));
    }
    text.push_str(&cut(
        &shown,
        MEMORY_SHOWN,
        "[The memory is longer than is shown and is cut here. \
         Shorten it so all of it is shown.]",
    ));
    if !folders.is_empty() {
        text.push_str("\nFolders:\n");
        for (name, description) in folders {
            text.push_str(&format!(
                "- memory/{name}/: {description}\n"
            ));
        }
    }
    text.trim_end().to_owned()
}

/// A skill as the front matter of its `SKILL.md` tells of it.
struct Skill {
    folder: String,
    name: String,
    description: String,
    /// The command that runs it, from the root its `skills/` is in.
    usage: Option<String>,
}

/// The skills on the shelf at `path`: one folder each whose
/// `SKILL.md` is a regular file.
fn shelf(path: &Path, under: &str) -> Vec<Skill> {
    let Ok(folder) = Folder::open(path) else {
        return Vec::new();
    };
    let prefix = if under.is_empty() {
        String::new()
    } else {
        format!("{under}/")
    };
    let listing = if under.is_empty() {
        folder.list(&|_| false)
    } else {
        folder.list_under(under)
    };
    let Ok(listing) = listing else {
        return Vec::new();
    };
    let mut skills = Vec::new();
    for entry in listing {
        let Some(rest) = entry.path.strip_prefix(&prefix) else {
            continue;
        };
        let Some((name, "SKILL.md")) = rest.split_once('/') else {
            continue;
        };
        let text = folder
            .read(&entry.path)
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .unwrap_or_default();
        let named = field(&text, "name:");
        let usage = field(&text, "usage:");
        skills.push(Skill {
            folder: name.to_owned(),
            name: if named.is_empty() {
                name.to_owned()
            } else {
                named
            },
            description: field(&text, "description:"),
            usage: (!usage.is_empty()).then_some(usage),
        });
    }
    skills
}

fn skills(worktree: &Path, library: &Path) -> String {
    let own = shelf(worktree, "skills");
    let mut text = String::from("# Your skills\n");
    if own.is_empty() {
        text.push_str("(none yet)");
    } else {
        let quoted =
            worktree.display().to_string().replace('\'', "'\\''");
        for skill in &own {
            let run = skill
                .usage
                .as_ref()
                .map(|usage| {
                    format!(" Run: `cd '{quoted}' && {usage}`")
                })
                .unwrap_or_default();
            text.push_str(&format!(
                "- {}: {}{run} (skills/{}/SKILL.md)\n",
                skill.name, skill.description, skill.folder
            ));
        }
    }
    let shared = shelf(library, "");
    if !shared.is_empty() {
        text.push_str(&format!(
            "\n# The library\nSkills every agent shares, kept in \
             {}. Read one and run it from there; do not copy it \
             into your worktree. You cannot write there. To share a \
             skill of your own, use `submit_skill`.\n",
            library.display()
        ));
        let root = library.display();
        for skill in shared {
            let there = format!("{root}/{}/", skill.folder);
            let run = skill
                .usage
                .map(|usage| {
                    let own = format!("skills/{}/", skill.folder);
                    format!(" Run: `{}`", usage.replace(&own, &there))
                })
                .unwrap_or_default();
            text.push_str(&format!(
                "- {}: {}{run} ({there}SKILL.md)\n",
                skill.name, skill.description
            ));
        }
    }
    text.trim_end().to_owned()
}

/// `text` cut to `most` characters, with `note` where it was cut.
fn cut(text: &str, most: usize, note: &str) -> String {
    if text.chars().count() <= most {
        return text.to_owned();
    }
    let kept = text.chars().take(most).collect::<String>();
    format!("{kept}\n{note}")
}
