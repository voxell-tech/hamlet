//! Shelves of skills: an agent's own and the library every agent
//! shares (C8, H33).

use std::io::{self, Read};
use std::path::Path;

use hamlet_os::fs::{Directory, Kind, LastLink, Name, Open};

/// A skill as the front matter of its `SKILL.md` tells of it.
pub(crate) struct Skill {
    pub name: String,
    pub description: String,
    usage: Option<String>,
    pub folder: String,
}

pub(crate) fn house_skills(house: &Directory) -> Vec<Skill> {
    Name::new("skills")
        .ok()
        .and_then(|name| house.child(&name).ok())
        .map(|shelf| listed(&shelf))
        .unwrap_or_default()
}

pub(super) fn skills_text(house: &Path, found: &[Skill]) -> String {
    let quoted = house.display().to_string().replace('\'', "'\\''");
    let lines = found
        .iter()
        .map(|s| {
            let run = s
                .usage
                .as_ref()
                .map(|u| format!(" Run: `cd '{quoted}' && {u}`"))
                .unwrap_or_default();
            format!(
                "- {}: {}{run} (skills/{}/SKILL.md)",
                s.name, s.description, s.folder
            )
        })
        .collect::<Vec<_>>();
    format!(
        "# Skills\n{}",
        if lines.is_empty() {
            "(none yet)".into()
        } else {
            lines.join("\n")
        }
    )
}

/// The folders of the house's skills.
pub(super) fn names(house: &Directory) -> Vec<String> {
    house_skills(house)
        .into_iter()
        .map(|skill| skill.folder)
        .collect()
}

/// The library's part where it is on this machine.
pub(crate) fn library(root: &Path) -> Option<String> {
    library_at(root, root)
}

/// The library's part: where it is, shown as `shown`, and each skill
/// with the command that runs it from there. None while it has no
/// skills.
pub(super) fn library_at(
    root: &Path,
    shown: &Path,
) -> Option<String> {
    let skills = if is_real_dir(root) {
        shelf(root)
    } else {
        Vec::new()
    };
    if skills.is_empty() {
        return None;
    }
    let root = shown.display();
    let mut lines = vec![format!(
        "# Library\nSkills every agent shares, kept in {root}. Use \
         one as it stands: read it and run it from there, and do \
         not copy it into your workspace. Where its text says \
         `skills/<name>/`, its files are in `{root}/<name>/`. You \
         cannot write there. To share a skill of your own, use \
         `submit_skill`."
    )];
    for skill in skills {
        let there = format!("{root}/{}/", skill.folder);
        let run = skill
            .usage
            .map(|u| {
                let own = format!("skills/{}/", skill.folder);
                format!(" Run: `{}`", u.replace(&own, &there))
            })
            .unwrap_or_default();
        lines.push(format!(
            "- {}: {}{run} ({there}SKILL.md)",
            skill.name, skill.description
        ));
    }
    Some(lines.join("\n"))
}

fn is_real_dir(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.is_dir())
}

pub(crate) fn shelf(dir: &Path) -> Vec<Skill> {
    Directory::open(dir, LastLink::Follow)
        .map(|shelf| listed(&shelf))
        .unwrap_or_default()
}

/// The skills of `shelf`, one folder each whose `SKILL.md` is a
/// regular file, in the folders' order.
fn listed(shelf: &Directory) -> Vec<Skill> {
    let mut found = shelf
        .names()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|name| {
            let text = name.to_str()?.to_owned();
            let folder = shelf.child(&name).ok()?;
            let paper =
                folder.stat(&Name::new(PAPER).ok()?).ok()??;
            (paper.kind == Kind::File).then(|| {
                skill(text, &read_paper(&folder).unwrap_or_default())
            })
        })
        .collect::<Vec<_>>();
    found.sort_by(|a, b| a.folder.cmp(&b.folder));
    found
}

/// The skill a `SKILL.md` of `text` tells of: its front matter runs
/// to the first line opening with `---`, and names it, or the folder
/// does.
pub(crate) fn skill(folder: String, text: &str) -> Skill {
    let front = text
        .strip_prefix("---")
        .map(|t| t.split("\n---").next().unwrap_or_default());
    let field = |key: &str| {
        front?.split('\n').find_map(|line| {
            let (k, v) = line.split_once(':')?;
            (k.trim() == key)
                .then(|| v.trim().trim_matches('"').to_owned())
        })
    };
    match field("name") {
        Some(name) => Skill {
            name,
            description: field("description").unwrap_or_default(),
            usage: field("usage").filter(|u| !u.is_empty()),
            folder,
        },
        None => Skill {
            name: folder.clone(),
            description: String::new(),
            usage: None,
            folder,
        },
    }
}

const PAPER: &str = "SKILL.md";

fn read_paper(folder: &Directory) -> io::Result<String> {
    let file = folder.file(&Name::new(PAPER)?, Open::Read)?;
    let mut bytes = Vec::new();
    file.into_file().take(64 * 1024).read_to_end(&mut bytes)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use hamlet_testing::Scratch;

    use crate::prompt::{Subject, prepare};
    use crate::service::Service;
    use crate::{Home, Overrides};

    #[tokio::test]
    async fn lists_no_skills_through_a_linked_shelf() {
        let dir = Scratch::new("prompt-linked-shelf");
        let mut service = Service::new(
            Home::open(&dir).unwrap(),
            Overrides::none(true),
        )
        .unwrap();
        let house =
            service.home.layout().root().join("houses/home/0");
        std::fs::remove_dir(house.join("skills")).unwrap();
        let elsewhere = dir.join("elsewhere");
        std::fs::create_dir_all(elsewhere.join("forecast")).unwrap();
        std::fs::write(
            elsewhere.join("forecast/SKILL.md"),
            "---\nname: forecast\ndescription: Get the \
             forecast\n---\n",
        )
        .unwrap();
        hamlet_testing::links::symlink(
            &elsewhere,
            house.join("skills"),
        )
        .unwrap();

        let subject = Subject {
            hamlet: "home",
            number: 0,
            model: Some("faux::faux-1"),
            sandbox: "bubblewrap",
            instructions: &[],
            task: None,
            conversation: None,
        };
        let sections = prepare(&mut service, &subject)
            .unwrap()
            .render()
            .await
            .unwrap();
        let skills = sections.iter().find(|s| s.name == "skills");
        assert_eq!(
            skills.map(|s| s.text.as_str()),
            Some("# Skills\n(none yet)")
        );
        let state = crate::state::state(&mut service, 0, 0).unwrap();
        let listed = state
            .agents
            .iter()
            .map(|agent| agent.skills.clone())
            .collect::<Vec<_>>();
        assert_eq!(listed, [Vec::<String>::new()]);
    }
}
