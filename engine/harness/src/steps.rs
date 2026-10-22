//! A script of steps, as a plugin's `steps.toml` writes it, run
//! against the harness. Each step does one thing; one that expects
//! something waits frames for it, and the script stops at the first
//! that fails.
//!
//! ```toml
//! # Built plugins loaded besides the one checked, from this folder.
//! with = ["../notes"]
//!
//! [[step]]
//! allow = "sample.board"
//!
//! [[step]]
//! expect = { panel = "sample.board/board", text = "First note" }
//!
//! [[step]]
//! click = { panel = "sample.notes/notes", widget = "Save" }
//! ```

use std::path::{Path, PathBuf};

use hamlet_engine_api::{Failure, FetchReply};
use hamlet_engine_grants::Answer;
use serde::Deserialize;

use crate::Harness;

/// A script: the plugins it needs besides the one checked, and its
/// steps.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Steps {
    /// Folders of built plugins, from the script's folder.
    #[serde(default)]
    pub with: Vec<PathBuf>,
    #[serde(default, rename = "step")]
    pub steps: Vec<Step>,
}

/// One thing a script does.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Step {
    /// Lets this plugin in once it asks, each ask allowed.
    Allow(String),
    /// Lets the plugin in once it asks, each ask answered `answer`.
    Admit { plugin: String, answer: Answer },
    /// Answers the next use the plugin asks about.
    Answer { plugin: String, answer: Answer },
    /// Runs this many frames.
    Frames(usize),
    /// Waits for the panel's outline to hold the text.
    Expect { panel: String, text: String },
    /// Waits for the panel's outline to stop holding the text.
    Lacks { panel: String, text: String },
    /// Presses the button whose text is `widget`.
    Click { panel: String, widget: String },
    /// Leaves the text in the field whose placeholder is `widget`.
    Write {
        panel: String,
        widget: String,
        text: String,
    },
    /// Switches the switch nearest the text `widget`.
    Toggle {
        panel: String,
        widget: String,
        on: bool,
    },
    /// Runs a command by its full id.
    Run(String),
    /// Waits for an asset of the type whose bytes hold the text.
    ExpectAsset {
        #[serde(rename = "type")]
        kind: String,
        text: String,
    },
    /// Waits for a line of the log that holds the text.
    ExpectLog(String),
    /// Replies to the fetch of the address once it is made.
    Reply {
        url: String,
        #[serde(default = "ok")]
        status: u16,
        #[serde(default)]
        body: String,
    },
    /// Fails the fetch of the address once it is made.
    Fail { url: String, why: String },
    /// Waits for the plugin's scene to hold as many of each as given.
    ExpectScene {
        plugin: String,
        entities: Option<usize>,
        meshes: Option<usize>,
        materials: Option<usize>,
        cameras: Option<usize>,
        decals: Option<usize>,
    },
    /// Clicks the entity of the plugin's scene named `key`, the copy
    /// `index` of instances.
    Pick {
        plugin: String,
        key: u32,
        #[serde(default)]
        index: u32,
    },
}

fn ok() -> u16 {
    200
}

impl Steps {
    /// The script in the file `path`, with its plugins' folders made
    /// whole.
    pub fn read(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|why| format!("{}: {why}", path.display()))?;
        let mut steps =
            toml::from_str::<Self>(&text).map_err(|why| {
                format!("{}: {}", path.display(), why.message())
            })?;
        let folder = path.parent().unwrap_or(Path::new("."));
        for with in &mut steps.with {
            *with = folder.join(&*with);
        }
        Ok(steps)
    }

    /// Runs each step on `harness`, telling `told` of each as it
    /// passes; why the first that fails did.
    pub fn run(
        &self,
        harness: &mut Harness,
        mut told: impl FnMut(&Step),
    ) -> Result<(), String> {
        for step in &self.steps {
            step.run(harness)?;
            told(step);
        }
        Ok(())
    }
}

impl Step {
    /// Does this step on `harness`.
    pub fn run(&self, harness: &mut Harness) -> Result<(), String> {
        match self {
            Self::Allow(plugin) => {
                harness.allow(plugin, Answer::Allow)
            }
            Self::Admit { plugin, answer } => {
                harness.allow(plugin, *answer)
            }
            Self::Answer { plugin, answer } => {
                harness.answer_use(plugin, *answer)
            }
            Self::Frames(count) => {
                harness.frames(*count);
                Ok(())
            }
            Self::Expect { panel, text }
            | Self::Lacks { panel, text } => {
                let holds = matches!(self, Self::Expect { .. });
                let mut last = None;
                let met = harness.until(|harness| {
                    last = harness.outline(panel);
                    last.as_ref().is_some_and(|shown| {
                        shown.contains(text.as_str()) == holds
                    })
                });
                if met {
                    return Ok(());
                }
                let wanted = if holds {
                    "never showed"
                } else {
                    "kept showing"
                };
                Err(match last {
                    Some(shown) => {
                        format!(
                            "{panel} {wanted} {text:?}; it \
                             shows\n{shown}"
                        )
                    }
                    None => format!("{panel} was never drawn"),
                })
            }
            Self::Click { panel, widget } => {
                harness.click(panel, widget)
            }
            Self::Write {
                panel,
                widget,
                text,
            } => harness.write(panel, widget, text),
            Self::Toggle { panel, widget, on } => {
                harness.toggle(panel, widget, *on)
            }
            Self::Run(command) => harness.run(command),
            Self::ExpectAsset { kind, text } => {
                let met = harness.until(|harness| {
                    harness.assets(kind).iter().any(|info| {
                        harness.read_asset(info.id).is_ok_and(
                            |asset| {
                                String::from_utf8_lossy(&asset.bytes)
                                    .contains(text.as_str())
                            },
                        )
                    })
                });
                met.then_some(()).ok_or_else(|| {
                    format!("no asset of {kind} ever held {text:?}")
                })
            }
            Self::ExpectLog(text) => {
                let met = harness.until(|harness| {
                    harness
                        .log()
                        .iter()
                        .any(|line| line.contains(text.as_str()))
                });
                met.then_some(()).ok_or_else(|| {
                    format!("the log never said {text:?}")
                })
            }
            Self::Reply { url, status, body } => harness.reply(
                url,
                Ok(FetchReply {
                    status: *status,
                    content_type: "text/plain".into(),
                    body: body.as_bytes().to_vec(),
                }),
            ),
            Self::Fail { url, why } => {
                harness.reply(url, Err(Failure::Failed(why.clone())))
            }
            Self::ExpectScene {
                plugin,
                entities,
                meshes,
                materials,
                cameras,
                decals,
            } => {
                let wanted = [
                    *entities, *meshes, *materials, *cameras, *decals,
                ];
                let mut last = None;
                let met = harness.until(|harness| {
                    last = harness.scene(plugin);
                    last.is_some_and(|held| {
                        let had = [
                            held.entities,
                            held.meshes,
                            held.materials,
                            held.cameras,
                            held.decals,
                        ];
                        wanted.iter().zip(had).all(|(want, has)| {
                            want.is_none_or(|want| want == has)
                        })
                    })
                });
                met.then_some(()).ok_or_else(|| match last {
                    Some(held) => format!(
                        "the scene of {plugin} never held what was \
                         wanted; it holds {held:?}"
                    ),
                    None => format!("{plugin} never made a scene"),
                })
            }
            Self::Pick { plugin, key, index } => {
                harness.pick(plugin, *key, *index)
            }
        }
    }
}
