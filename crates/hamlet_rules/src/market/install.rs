//! A package bought, written into the home: an agent moved into a
//! hamlet, tools added, or an owned one brought up to a new version.

use std::path::Path;

use anyhow::{Result, bail};
use hamlet_home::Home;
use hamlet_model::market::package::{
    Agent, CHARACTER, File, Hamlet as Packed, NOTES, Plugin, SKILLS,
    check_name, safe,
};

use crate::hamlet::{self, Hamlet};
use crate::realm::Realm;

/// What is added to an agent's name, in turn, when its own is taken.
const OTHER_NAMES: [&str; 9] =
    ["ii", "iii", "iv", "v", "vi", "vii", "viii", "ix", "x"];

/// Moves `agent` into `hamlet` with all it knows, under its own name
/// or, when that is taken in the realm, the first free of `name-ii`,
/// `name-iii`... Returns the name it lives under.
pub fn hire(
    hamlet: &Hamlet,
    agent: &Agent,
    reason: &str,
) -> Result<String> {
    check_name(&agent.name)?;
    let realm = Realm::new(hamlet.home().to_path_buf());
    let taken = |name: &str| {
        realm.hamlet_of(name).is_some()
            || hamlet.members().iter().any(|r| r.name == name)
    };
    let name = std::iter::once(agent.name.clone())
        .chain(
            OTHER_NAMES.iter().map(|n| format!("{}-{n}", agent.name)),
        )
        .find(|name| {
            hamlet::check_name(name).is_ok() && !taken(name)
        });
    let Some(name) = name else {
        bail!(
            "an agent named `{}` already lives in the realm",
            agent.name
        );
    };
    let purpose = match agent.purpose.trim() {
        "" => "came from the Agent Bazaar",
        purpose => purpose,
    };
    hamlet.create(&name, purpose, reason)?;
    move_in(&hamlet.house(&name)?, agent)?;
    Ok(name)
}

/// Writes all `agent` knows into `house`, which is new.
pub(crate) fn move_in(house: &Home, agent: &Agent) -> Result<()> {
    write_agent(house, agent, true)
}

/// Brings the agent in `house` up to `agent`: its character and the
/// skills `agent` comes with are replaced. Its notes are its own
/// memory and are kept, unless it has none yet.
pub fn refresh(house: &Home, agent: &Agent) -> Result<()> {
    write_agent(house, agent, false)
}

fn write_agent(house: &Home, agent: &Agent, new: bool) -> Result<()> {
    let mut skills = Vec::<&str>::new();
    for file in &agent.files {
        if let Some(rest) = file.path.strip_prefix(SKILLS)
            && let Some((skill, _)) = rest.split_once('/')
            && !skills.contains(&skill)
        {
            check_name(skill)?;
            skills.push(skill);
        }
    }
    for skill in &skills {
        let folder = house.skills().join(skill);
        if folder.exists() {
            std::fs::remove_dir_all(folder)?;
        }
    }
    let notes_kept = !new
        && std::fs::read_to_string(house.notes())
            .is_ok_and(|notes| notes.trim() != "# Notes");
    for file in &agent.files {
        let to = match file.path.as_str() {
            CHARACTER => house.character(),
            NOTES if notes_kept => continue,
            NOTES => house.notes(),
            path => match path.strip_prefix(SKILLS) {
                Some(rest) => house.skills().join(rest),
                None => bail!("`{path}` is not part of an agent"),
            },
        };
        write(&to, file)?;
    }
    Ok(())
}

/// Brings `hamlet`, founded from a package, up to `bought`: each of
/// its agents that lives there is refreshed, and one that does not is
/// moved in.
pub fn bring_up(hamlet: &Hamlet, bought: &Packed) -> Result<()> {
    let members = hamlet.members();
    for agent in bought.everyone() {
        if members.iter().any(|member| member.name == agent.name) {
            refresh(&hamlet.house(&agent.name)?, agent)?;
            continue;
        }
        let purpose = match agent.purpose.trim() {
            "" => "came with the hamlet",
            purpose => purpose,
        };
        hamlet.create(
            &agent.name,
            purpose,
            "came with a new version of the hamlet",
        )?;
        move_in(&hamlet.house(&agent.name)?, agent)?;
    }
    Ok(())
}

/// Adds the tools of `plugin` to the house `house`. A tool of the
/// same name is refused, unless it is one of `replacing`, which this
/// plugin put there. Returns the tools' names.
pub fn install_tools(
    house: &Home,
    plugin: &Plugin,
    replacing: &[String],
) -> Result<Vec<String>> {
    let tools = house.tools();
    for tool in &plugin.tools {
        check_name(&tool.name)?;
        if tools.join(&tool.name).exists()
            && !replacing.contains(&tool.name)
        {
            bail!(
                "the home has a tool named `{}` already",
                tool.name
            );
        }
    }
    let mut names = Vec::new();
    for tool in &plugin.tools {
        let dir = tools.join(&tool.name);
        // Written beside it first, so a tool half written is never
        // read.
        let fresh = tools.join(format!(".{}.new", tool.name));
        if fresh.exists() {
            std::fs::remove_dir_all(&fresh)?;
        }
        for file in &tool.files {
            write(&fresh.join(&file.path), file)?;
        }
        if dir.exists() {
            std::fs::remove_dir_all(&dir)?;
        }
        std::fs::rename(&fresh, &dir)?;
        names.push(tool.name.clone());
    }
    Ok(names)
}

/// Writes `file` at `to`, which is its path under where it goes.
fn write(to: &Path, file: &File) -> Result<()> {
    if !safe(&file.path) {
        bail!("`{}` is not a path a package may hold", file.path);
    }
    if let Some(folder) = to.parent() {
        std::fs::create_dir_all(folder)?;
    }
    std::fs::write(to, &file.data)?;
    #[cfg(unix)]
    if file.executable {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            to,
            std::fs::Permissions::from_mode(0o755),
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use hamlet_model::market::package::{MANIFEST, Tool};
    use hamlet_testing::Scratch;

    use super::*;
    use crate::hamlet::HOME;
    use crate::market::{pack_agent, pack_tools};

    fn file(path: &str, text: &str) -> File {
        File {
            path: path.into(),
            data: text.as_bytes().to_vec(),
            executable: false,
        }
    }

    fn scribe() -> Agent {
        Agent {
            name: "scribe".into(),
            purpose: "Writes letters.".into(),
            model: None,
            files: vec![
                file(CHARACTER, "Careful and kind."),
                file(NOTES, "# Notes\nthe seller's notes"),
                file(
                    "workspace/skills/ink/SKILL.md",
                    "---\nname: ink\n---\n",
                ),
                File {
                    executable: true,
                    ..file(
                        "workspace/skills/ink/scripts/run.sh",
                        "echo",
                    )
                },
            ],
        }
    }

    #[test]
    fn an_agent_bought_moves_in_whole_and_is_packed_back_the_same() {
        let dir = Scratch::new("market-hire");
        let realm = Realm::open(&dir).unwrap();
        let home = realm.hamlet(HOME).unwrap();
        let agent = scribe();

        let name = hire(&home, &agent, "bought").unwrap();
        assert_eq!(name, "scribe");
        // Hired twice, the second takes another name.
        assert_eq!(
            hire(&home, &agent, "bought").unwrap(),
            "scribe-ii"
        );

        let packed = pack_agent(&home, "scribe", true).unwrap();
        assert_eq!(packed.purpose, agent.purpose);
        let paths = |agent: &Agent| {
            let mut paths = agent
                .files
                .iter()
                .map(|file| (file.path.clone(), file.executable))
                .collect::<Vec<_>>();
            paths.sort();
            paths
        };
        assert_eq!(paths(&packed), paths(&agent));
        assert_eq!(
            packed.text(CHARACTER).unwrap().trim(),
            "Careful and kind."
        );
        let unnoted = pack_agent(&home, "scribe", false).unwrap();
        assert!(unnoted.text(NOTES).is_none());
    }

    #[test]
    fn an_update_replaces_character_and_skills_and_keeps_the_notes() {
        let dir = Scratch::new("market-refresh");
        let realm = Realm::open(&dir).unwrap();
        let home = realm.hamlet(HOME).unwrap();
        hire(&home, &scribe(), "bought").unwrap();
        let house = home.house("scribe").unwrap();
        std::fs::write(house.notes(), "# Notes\nmine now").unwrap();
        std::fs::write(
            house.skills().join("ink").join("old.md"),
            "x",
        )
        .unwrap();

        let newer = Agent {
            files: vec![
                file(CHARACTER, "Bolder."),
                file(NOTES, "# Notes\nnewer seller notes"),
                file("workspace/skills/ink/SKILL.md", "v2"),
            ],
            ..scribe()
        };
        refresh(&house, &newer).unwrap();
        let read =
            |path: &Path| std::fs::read_to_string(path).unwrap();
        assert_eq!(read(&house.character()), "Bolder.");
        assert_eq!(read(&house.notes()), "# Notes\nmine now");
        assert_eq!(read(&house.skills().join("ink/SKILL.md")), "v2");
        assert!(!house.skills().join("ink/old.md").exists());
    }

    #[test]
    fn tools_are_added_and_replaced_only_by_their_own_plugin() {
        let dir = Scratch::new("market-tools");
        let realm = Realm::open(&dir).unwrap();
        let home = realm.hamlet(HOME).unwrap();
        let house = home.hall().unwrap();
        let plugin = Plugin {
            tools: vec![Tool {
                name: "cal".into(),
                files: vec![
                    file(
                        MANIFEST,
                        r#"{"name":"cal","exec":["./run"]}"#,
                    ),
                    File {
                        executable: true,
                        ..file("run", "#!/bin/sh")
                    },
                ],
            }],
        };
        assert_eq!(
            install_tools(&house, &plugin, &[]).unwrap(),
            ["cal"]
        );
        assert!(install_tools(&house, &plugin, &[]).is_err());
        install_tools(&house, &plugin, &["cal".into()]).unwrap();
        let found = crate::market::tools(&realm);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].agent, home.mayor().name);
        let packed = pack_tools(&[found[0].dir.clone()]).unwrap();
        assert_eq!(packed, plugin);
    }
}
