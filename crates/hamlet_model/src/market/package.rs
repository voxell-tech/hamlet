//! What is downloaded when a listing is bought: one JSON document
//! holding every file of an agent, a hamlet or a plugin's tools, in
//! the form the bazaar's server stores and checks.

use anyhow::{Result, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::lot::Kind;

/// The format written, and the only one read.
pub const FORMAT: u32 = 1;

/// The most bytes a package may be, as sent.
pub const MOST_BYTES: usize = 10 * 1024 * 1024;

/// The longest a file's path may be.
const LONGEST_PATH: usize = 200;

/// A package of the bazaar.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Package {
    pub format: u32,
    #[serde(flatten)]
    pub contents: Contents,
}

/// What a package holds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Contents {
    Agent { agent: Agent },
    Hamlet { hamlet: Hamlet },
    Plugin { plugin: Plugin },
}

impl Package {
    pub fn new(contents: Contents) -> Package {
        Package {
            format: FORMAT,
            contents,
        }
    }

    pub fn kind(&self) -> Kind {
        match self.contents {
            Contents::Agent { .. } => Kind::Agent,
            Contents::Hamlet { .. } => Kind::Hamlet,
            Contents::Plugin { .. } => Kind::Plugin,
        }
    }

    /// Reads a package, refusing one that is not whole and safe to
    /// write out.
    pub fn read(bytes: &[u8]) -> Result<Package> {
        let package = serde_json::from_slice::<Package>(bytes)?;
        package.check()?;
        Ok(package)
    }

    /// The package as it is sent.
    pub fn bytes(&self) -> Result<Vec<u8>> {
        let bytes = serde_json::to_vec(self)?;
        if bytes.len() > MOST_BYTES {
            bail!(
                "it comes to {:.1} MB, over the bazaar's {} MB",
                bytes.len() as f64 / 1024.0 / 1024.0,
                MOST_BYTES / 1024 / 1024
            );
        }
        Ok(bytes)
    }

    /// Checks every name and path, as the server does.
    pub fn check(&self) -> Result<()> {
        if self.format != FORMAT {
            bail!("a package of format {} is not known", self.format);
        }
        match &self.contents {
            Contents::Agent { agent } => agent.check(),
            Contents::Hamlet { hamlet } => {
                check_name(&hamlet.name)?;
                hamlet.mayor.check()?;
                let mut names = vec![hamlet.mayor.name.as_str()];
                for agent in &hamlet.agents {
                    agent.check()?;
                    if names.contains(&agent.name.as_str()) {
                        bail!(
                            "two agents are named `{}`",
                            agent.name
                        );
                    }
                    names.push(&agent.name);
                }
                Ok(())
            }
            Contents::Plugin { plugin } => {
                if plugin.tools.is_empty() {
                    bail!("a plugin needs a tool");
                }
                for tool in &plugin.tools {
                    check_name(&tool.name)?;
                    check_files(&tool.files)?;
                    if !tool.files.iter().any(|f| f.path == MANIFEST)
                    {
                        bail!(
                            "the tool `{}` has no manifest",
                            tool.name
                        );
                    }
                }
                Ok(())
            }
        }
    }
}

/// A tool's manifest, in its folder.
pub const MANIFEST: &str = "manifest.json";

/// An agent with everything it knows: files of its house.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Agent {
    pub name: String,
    pub purpose: String,
    /// The model its maker ran it on; only a suggestion.
    #[serde(default)]
    pub model: Option<String>,
    pub files: Vec<File>,
}

impl Agent {
    fn check(&self) -> Result<()> {
        check_name(&self.name)?;
        check_files(&self.files)?;
        for file in &self.files {
            if !agent_file(&file.path) {
                bail!("`{}` is not part of an agent", file.path);
            }
        }
        Ok(())
    }

    /// The file at `path`, as text.
    pub fn text(&self, path: &str) -> Option<String> {
        self.files
            .iter()
            .find(|file| file.path == path)
            .map(|file| {
                String::from_utf8_lossy(&file.data).into_owned()
            })
    }
}

/// A house's `CHARACTER.md`, as a path in a package.
pub const CHARACTER: &str = "CHARACTER.md";
/// A house's notes, as a path in a package.
pub const NOTES: &str = "workspace/NOTES.md";
/// A house's skills, as a path in a package.
pub const SKILLS: &str = "workspace/skills/";

/// True for the paths an agent's package may hold.
fn agent_file(path: &str) -> bool {
    path == CHARACTER
        || path == NOTES
        || path.strip_prefix(SKILLS).is_some_and(|rest| {
            rest.split_once('/')
                .is_some_and(|(skill, _)| check_name(skill).is_ok())
        })
}

/// A hamlet with its mayor and agents.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Hamlet {
    /// Its name on disk and in commands.
    pub name: String,
    /// The name it is shown by.
    pub title: String,
    /// What kind of hamlet it is, in a few words.
    pub kind: String,
    pub purpose: String,
    /// The seed its town's looks come from, in decimal.
    pub seed: String,
    pub mayor: Agent,
    /// Its agents besides the mayor.
    pub agents: Vec<Agent>,
}

impl Hamlet {
    /// Everyone who comes with it, the mayor first.
    pub fn everyone(&self) -> impl Iterator<Item = &Agent> {
        std::iter::once(&self.mayor).chain(&self.agents)
    }

    pub fn seed(&self) -> u64 {
        self.seed.parse().unwrap_or_default()
    }
}

/// Tools that add to Hamlet.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Plugin {
    pub tools: Vec<Tool>,
}

/// A tool: its manifest and program, the files of its folder under
/// `tools/`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tool {
    pub name: String,
    pub files: Vec<File>,
}

/// A file of a package.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct File {
    /// Relative, with `/` between its parts.
    pub path: String,
    #[serde(
        serialize_with = "to_base64",
        deserialize_with = "from_base64"
    )]
    pub data: Vec<u8>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub executable: bool,
}

fn to_base64<S: Serializer>(
    data: &[u8],
    to: S,
) -> Result<S::Ok, S::Error> {
    to.serialize_str(&STANDARD.encode(data))
}

fn from_base64<'de, D: Deserializer<'de>>(
    from: D,
) -> Result<Vec<u8>, D::Error> {
    let text = String::deserialize(from)?;
    STANDARD.decode(text).map_err(serde::de::Error::custom)
}

/// Checks a name of an agent, a hamlet, a tool or a skill as the
/// server does.
pub fn check_name(name: &str) -> Result<()> {
    let mut bytes = name.bytes();
    let first = bytes.next();
    let fits = name.len() <= 32
        && first.is_some_and(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit()
        })
        && bytes.all(|b| {
            b.is_ascii_lowercase()
                || b.is_ascii_digit()
                || b == b'-'
                || b == b'_'
        });
    if !fits {
        bail!(
            "`{name}` is not a name the bazaar takes: lowercase \
             letters, digits, `-` and `_`, at most 32"
        );
    }
    Ok(())
}

/// Checks that each path stays inside the folder it is written to,
/// and that none comes twice.
fn check_files(files: &[File]) -> Result<()> {
    let mut seen = Vec::<&str>::new();
    for file in files {
        if !safe(&file.path) {
            bail!("`{}` is not a path a package may hold", file.path);
        }
        if seen.contains(&file.path.as_str()) {
            bail!("`{}` comes twice", file.path);
        }
        seen.push(&file.path);
    }
    Ok(())
}

/// True when `path` is relative and stays inside its folder.
pub fn safe(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= LONGEST_PATH
        && !path.contains('\\')
        && !path.contains(':')
        && path
            .split('/')
            .all(|part| !matches!(part, "" | "." | ".."))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str) -> File {
        File {
            path: path.into(),
            data: b"hi".to_vec(),
            executable: false,
        }
    }

    fn agent(name: &str, files: Vec<File>) -> Agent {
        Agent {
            name: name.into(),
            purpose: "Helps.".into(),
            model: None,
            files,
        }
    }

    #[test]
    fn a_package_goes_out_and_back_as_the_server_reads_it() {
        let package = Package::new(Contents::Agent {
            agent: agent(
                "scribe",
                vec![File {
                    executable: true,
                    ..file("workspace/skills/ink/run.sh")
                }],
            ),
        });
        let bytes = package.bytes().unwrap();
        let json =
            serde_json::from_slice::<serde_json::Value>(&bytes)
                .unwrap();
        assert_eq!(json["kind"], "agent");
        assert_eq!(json["format"], 1);
        assert_eq!(json["agent"]["files"][0]["data"], "aGk=");
        assert_eq!(Package::read(&bytes).unwrap(), package);
    }

    #[test]
    fn a_package_that_would_write_outside_its_place_is_refused() {
        for bad in [
            "../x",
            "/etc/passwd",
            "workspace/skills/a/../../../x",
            "workspace//NOTES.md",
            "C:/x",
            "a\\b",
        ] {
            assert!(!safe(bad), "{bad}");
        }
        let refused = |files| {
            Package::new(Contents::Agent {
                agent: agent("scribe", files),
            })
            .check()
            .is_err()
        };
        assert!(refused(vec![file("sessions/a.jsonl")]));
        assert!(refused(vec![file("workspace/skills/Bad Name/x")]));
        assert!(refused(vec![file(NOTES), file(NOTES)]));
        assert!(!refused(vec![file(NOTES), file(CHARACTER)]));

        let tool = |name: &str, files| Tool {
            name: name.into(),
            files,
        };
        let plugin = |tools| {
            Package::new(Contents::Plugin {
                plugin: Plugin { tools },
            })
            .check()
        };
        assert!(
            plugin(vec![tool("cal", vec![file("run")])]).is_err()
        );
        assert!(
            plugin(vec![tool("cal", vec![file(MANIFEST)])]).is_ok()
        );
    }
}
