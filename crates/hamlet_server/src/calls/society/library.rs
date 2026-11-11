//! An agent and the library (H33): a skill offered to the librarian,
//! and a question of which skills help. Each runs as an errand the
//! call waits for.

use anyhow::Result;
use hamlet_store::ids::Actor;

use super::{Call, Step, said, text};
use crate::core::Core;
use crate::knowledge::folder::Folder;
use crate::knowledge::{self, library};
use crate::society::library::{self as shelf, Offering};
use crate::society::refusal::Refusal;
use crate::talk::errands::{self, Ended, Errand};

/// The errand the librarian's calls are.
pub(crate) const LIBRARIAN: &str = "librarian";

/// The librarian's part before every errand.
const ROLE: &str =
    "You are the realm's librarian. You keep the library of skills \
     every agent shares: you know what is on its shelves, you take \
     a skill offered only when it is useful to others, distinct \
     from the shelves, clear enough to run and free of private \
     names, keys and paths, and you never invent a skill.";

/// The owner of the errand that answers the call `key` of `runtime`.
pub(crate) fn asked(runtime: &str, key: &str) -> String {
    format!("ask:{runtime}/{key}")
}

/// The owner of the errand that judges offer `id`.
pub(crate) fn judging(id: i64) -> String {
    format!("offer:{id}")
}

impl Core {
    /// What the librarian is told before its task: its role and the
    /// shelves.
    fn librarian(&mut self) -> Result<String> {
        let layout = self.realm.layout().clone();
        let shelves = library::skills(&layout)?
            .into_iter()
            .map(|(name, description)| {
                format!("- {name}: {description}")
            })
            .collect::<Vec<_>>();
        let shelves = if shelves.is_empty() {
            "(empty)".to_owned()
        } else {
            shelves.join("\n")
        };
        Ok(format!("{ROLE}\n\n# The shelves\n{shelves}"))
    }

    pub(in crate::calls) async fn ask_librarian(
        &mut self,
        call: &Call<'_>,
    ) -> Result<Step> {
        let owner = asked(&call.call.runtime, &call.call.key);
        let db = self.realm.store().connection();
        match errands::ended(db, LIBRARIAN, &owner)? {
            Some(Ended::Answered(answer)) => {
                return Ok(Step::Done(said(
                    format!("The librarian says: {answer}"),
                    false,
                )));
            }
            Some(Ended::Failed(failure)) => {
                return Ok(Step::Done(said(
                    format!(
                        "The librarian could not answer: {failure}"
                    ),
                    true,
                )));
            }
            None => {}
        }
        let question = text(call, "question");
        if question.is_empty() {
            return Ok(Step::of(Err(Refusal::Usage {
                words: "Say what you need a skill for.".into(),
            })));
        }
        let by = self.name(call.conversation.agent)?;
        let instructions = format!(
            "{}\n\nName the skills on the shelves that do what is \
             asked, with their exact commands. If nothing fits, say \
             so.",
            self.librarian()?
        );
        let question = format!("{by} asks: {question}");
        let task = self.task_of(call)?;
        let agent = call.conversation.agent;
        self.change(Actor::Agent(agent), |cx, env| {
            errands::run(
                cx,
                env,
                Errand {
                    kind: LIBRARIAN,
                    owner: &owner,
                    agent,
                    instructions: &instructions,
                    question: &question,
                    task,
                },
            )
        })
        .await?;
        Ok(Step::Wait("The librarian is looking.".into()))
    }

    pub(in crate::calls) async fn submit_skill(
        &mut self,
        call: &Call<'_>,
    ) -> Result<Step> {
        let db = self.realm.store().connection();
        if let Some(found) =
            shelf::offer_of(db, &call.call.runtime, &call.call.key)?
        {
            return Ok(match found.words {
                Some(words) => Step::Done(said(words, false)),
                None => {
                    Step::Wait("The librarian is judging it.".into())
                }
            });
        }
        let name = text(call, "skill");
        let reason = text(call, "reason");
        let plain = !name.is_empty()
            && name.bytes().all(|b| {
                b.is_ascii_alphanumeric() || b == b'-' || b == b'_'
            });
        if !plain {
            return Ok(Step::of(Err(Refusal::Usage {
                words: "`skill` is its folder under skills/, in \
                        letters, digits, hyphens and underscores."
                    .into(),
            })));
        }
        let layout = self.realm.layout().clone();
        if library::has(&layout, &name)? {
            return Ok(Step::of(Err(Refusal::Usage {
                words: format!(
                    "The library has a skill named `{name}` \
                     already. Use it, or offer yours under another \
                     name if it does something else."
                ),
            })));
        }
        let db = self.realm.store().connection();
        if shelf::is_judging(db, &name)? {
            return Ok(Step::of(Err(Refusal::Usage {
                words: "Another offer of that skill is being judged."
                    .into(),
            })));
        }
        let agent = call.conversation.agent;
        let lodge = knowledge::lodging(db, &layout, agent)?.path;
        let files = match skill_files(&lodge, &name)? {
            Ok(files) => files,
            Err(refusal) => return Ok(Step::of(Err(refusal))),
        };
        let paper = files
            .iter()
            .find(|(path, _)| path == "SKILL.md")
            .map(|(_, bytes)| {
                String::from_utf8_lossy(bytes).into_owned()
            })
            .unwrap_or_default();
        if library::field(&paper, "name:").is_empty()
            || library::field(&paper, "description:").is_empty()
        {
            return Ok(Step::of(Err(Refusal::Usage {
                words: format!(
                    "skills/{name}/SKILL.md must open with a front \
                     matter `name:` and `description:`."
                ),
            })));
        }
        let task = self.task_of(call)?;
        let (runtime, key) =
            (call.call.runtime.clone(), call.call.key.clone());
        let id = self
            .change(Actor::Agent(agent), |cx, _| {
                shelf::offer(
                    cx,
                    Offering {
                        agent,
                        skill: &name,
                        reason: &reason,
                        task,
                        runtime: &runtime,
                        key: &key,
                    },
                )
            })
            .await?;
        let tree = library::keep_offer(
            &layout,
            id.0,
            &files,
            crate::clock::now(),
        )?;
        let db = self.realm.store().connection();
        shelf::kept(db, id, &tree.to_string())?;
        let by = self.name(agent)?;
        let listed = files
            .iter()
            .map(|(path, _)| path.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let shown = paper.chars().take(6000).collect::<String>();
        let instructions = format!(
            "{}\n\nAnswer with ACCEPT or REFUSE as your first word, \
             then say why in a sentence or two.",
            self.librarian()?
        );
        let question = format!(
            "{by} offers the skill `{name}`: {reason}\n\nIts files: \
             {listed}\n\nIts SKILL.md:\n{shown}"
        );
        let owner = judging(id.0);
        self.change(Actor::Agent(agent), |cx, env| {
            errands::run(
                cx,
                env,
                Errand {
                    kind: LIBRARIAN,
                    owner: &owner,
                    agent,
                    instructions: &instructions,
                    question: &question,
                    task,
                },
            )
        })
        .await?;
        Ok(Step::Wait("The librarian is judging it.".into()))
    }
}

/// The files of the skill `name` in the lodge at `lodge`, within the
/// library's cap.
fn skill_files(
    lodge: &std::path::Path,
    name: &str,
) -> Result<Result<Vec<(String, Vec<u8>)>, Refusal>> {
    let Ok(folder) = Folder::open(lodge) else {
        return Ok(Err(Refusal::NoSkill { name: name.into() }));
    };
    let prefix = format!("skills/{name}/");
    let listing = folder.list_under(&format!("skills/{name}"))?;
    if listing.is_empty() {
        return Ok(Err(Refusal::NoSkill { name: name.into() }));
    }
    let mut files = Vec::new();
    let mut total = 0;
    for entry in listing {
        let bytes = folder.read(&entry.path)?;
        total += bytes.len();
        if total > library::MOST_BYTES {
            return Ok(Err(Refusal::Usage {
                words: format!(
                    "`{name}` holds more than the library takes, 4 \
                     MB."
                ),
            }));
        }
        files.push((entry.path[prefix.len()..].to_owned(), bytes));
    }
    Ok(Ok(files))
}
