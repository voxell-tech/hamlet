//! The Hand's acts on an agent: a hire pinned, restored or erased, a
//! model's effort and fast tier, a dream, a reflection, a stop and a
//! resume, a lodge's memory written, and its prompt and papers read.
//! A reserved act waits for a chief before it gets here.

use anyhow::Result;
use hamlet_protocol::client::{self as protocol, Keeper};
use hamlet_store::ids::{Actor, AgentId};

use super::args::{
    Called, Dream, EditMemory, EraseHire, Pin, ReadPapers,
    ReadPrompt, Reflect, RestoreHire, Resume, SetEffort, SetFast,
    Stop,
};
use super::{Call, cut};
use crate::conversations::dreams::Dreamer;
use crate::conversations::{self, chats, reflections};
use crate::core::Core;
use crate::knowledge::memory_path;
use crate::knowledge::versions::pinned;
use crate::realm::change::{Done, done, refuse};
use crate::society::addresses::{self, Reach};
use crate::society::agents::{self, Effort};
use crate::society::refusal::Refusal;
use crate::society::{authority, hires, merges};

/// The address that names every agent but the caller.
const ALL: &str = "all";

impl Core<'_, '_> {
    /// Carries out the Hand's call `called` on a hire it leads.
    pub(super) fn led_tool(
        &mut self,
        call: &Call<'_>,
        called: Called,
    ) -> Done<String> {
        match called {
            Called::Pin(Pin {
                hire,
                version,
                reason,
            }) => {
                let hire = done!(self.led_within(
                    call,
                    &hire,
                    None,
                    Reach::Living
                ));
                let name = self.name(hire)?;
                let target = done!(self.pin_target(hire, &version));
                self.carry(call, |cx, _| {
                    Ok(hires::set_pin(
                        cx,
                        hire,
                        target.as_deref(),
                        &reason,
                    )?
                    .map(|()| pinned(&name, target.as_deref())))
                })
            }
            Called::RestoreHire(RestoreHire { hire, reason }) => {
                let hire = done!(self.led_within(
                    call,
                    &hire,
                    None,
                    Reach::Kept
                ));
                let name = self.name(hire)?;
                // A merged hire comes back by splitting its merge.
                let db = self.realm.store().connection();
                let merge = merges::of_absorbed(db, hire)?;
                self.carry(call, |cx, _| {
                    Ok(match &merge {
                        Some(merge) => merges::split(
                            cx, merge.id, &reason,
                        )?
                        .map(|()| {
                            format!(
                                "{name} is back; what it brought to \
                                 the hire it was merged into is \
                                 taken out again."
                            )
                        }),
                        None => hires::come_back(cx, hire, &reason)?
                            .map(|()| format!("{name} is back.")),
                    })
                })
            }
            Called::EraseHire(EraseHire { hire }) => {
                let hire = done!(self.led_within(
                    call,
                    &hire,
                    None,
                    Reach::Kept
                ));
                let name = self.name(hire)?;
                self.carry(call, |cx, _| {
                    Ok(hires::erase(cx, hire)?.map(|()| {
                        format!("{name} was erased for good.")
                    }))
                })
            }
            _ => refuse!(Refusal::Unsupported {
                what: format!("the tool {}", call.tool),
            }),
        }
    }

    /// Carries out the Hand's call `called` on an agent.
    pub(super) fn agent_tool(
        &mut self,
        call: &Call<'_>,
        called: Called,
    ) -> Done<String> {
        let caller = call.conversation.agent;
        let by = Actor::Agent(caller);
        let address = match &called {
            Called::Stop(Stop { agent }) if agent == ALL => {
                return self.stop_all(call);
            }
            Called::Resume(Resume { agent }) if agent == ALL => {
                return self.resume_all(call);
            }
            Called::SetEffort(SetEffort { agent, .. })
            | Called::SetFast(SetFast { agent, .. })
            | Called::Reflect(Reflect { agent })
            | Called::Stop(Stop { agent })
            | Called::Resume(Resume { agent })
            | Called::EditMemory(EditMemory { agent, .. })
            | Called::ReadPrompt(ReadPrompt { agent })
            | Called::ReadPapers(ReadPapers { agent }) => {
                agent.clone()
            }
            _ => refuse!(Refusal::Unsupported {
                what: format!("the tool {}", call.tool),
            }),
        };
        let agent = done!(self.named(&address, None));
        let name = self.name(agent)?;
        let db = self.realm.store().connection();
        let allowed = match called {
            Called::SetEffort(_) | Called::SetFast(_) => {
                authority::may_steer(db, by, agent)?
            }
            _ => authority::may_reach(db, by, agent)?,
        };
        if !allowed && agent == caller {
            refuse!(Refusal::OwnSteering);
        }
        if !allowed {
            refuse!(Refusal::NotYours { name });
        }
        match called {
            Called::SetEffort(SetEffort { effort, .. }) => {
                let effort = match effort.as_str() {
                    "" | "none" => None,
                    level => match level.parse::<Effort>() {
                        Ok(effort) => Some(effort),
                        Err(_) => refuse!(Refusal::NoEffort {
                            level: level.to_owned(),
                        }),
                    },
                };
                self.carry(call, |cx, _| {
                    Ok(agents::set_effort(cx, agent, effort)?.map(
                        |()| match effort {
                            Some(effort) => {
                                format!("{name} thinks at {effort}.")
                            }
                            None => format!(
                                "{name} thinks as hard as the \
                                 realm's setting says."
                            ),
                        },
                    ))
                })
            }
            Called::SetFast(SetFast { fast, .. }) => {
                self.carry(call, |cx, _| {
                    Ok(agents::set_fast(cx, agent, fast)?.map(|()| {
                        let word = if fast { "on" } else { "off" };
                        format!(
                            "{name} asks for the fast tier: {word}."
                        )
                    }))
                })
            }
            Called::Reflect(_) => self.carry(call, |cx, env| {
                let conversation = chats::current(cx, env, agent)?;
                Ok(Ok(
                    match reflections::reflect(cx, conversation)? {
                        Some(_) => {
                            format!("Asked {name} to reflect.")
                        }
                        None => format!(
                            "{name} is reflecting already, or has \
                             nothing new to reflect on."
                        ),
                    },
                ))
            }),
            Called::Stop(_) => {
                if agent == caller {
                    refuse!(Refusal::StopSelf);
                }
                let working = self.working()?;
                self.carry(call, |cx, _| {
                    let stopped = chats::stop(cx, agent, &working)?;
                    Ok(Ok(format!(
                        "Stopped {stopped} running turns of {name}."
                    )))
                })
            }
            Called::Resume(_) => self.carry(call, |cx, _| {
                Ok(Ok(if chats::resume(cx, agent)? {
                    format!("{name} takes what waits again.")
                } else {
                    format!("{name} was not stopped.")
                }))
            }),
            Called::EditMemory(EditMemory {
                path, content, ..
            }) => {
                let path = match memory_path(&path) {
                    Ok(path) => path,
                    Err(refusal) => refuse!(refusal),
                };
                self.begin(call)?;
                self.write_lodge_memory(by, agent, &path, &content)?;
                Ok(Ok(format!("Wrote {path} of {name}'s lodge.")))
            }
            Called::ReadPrompt(_) => {
                let conversation = self.change(by, |cx, env| {
                    chats::current(cx, env, agent)
                })?;
                let db = self.realm.store().connection();
                let kept = conversations::get(db, conversation)?;
                let sections = self
                    .prompt(&kept)?
                    .into_iter()
                    .map(|section| {
                        format!(
                            "[{}]\n{}",
                            section.name, section.text
                        )
                    })
                    .collect::<Vec<_>>();
                Ok(Ok(cut(sections.join("\n\n"))))
            }
            _ => {
                let of = Keeper::Agent(protocol::AgentId(
                    u64::try_from(agent.0)?,
                ));
                Ok(self
                    .papers(of)?
                    .map(|papers| cut(paper_text(&papers))))
            }
        }
    }

    /// Every living agent but the caller of `call`.
    fn others(&mut self, call: &Call<'_>) -> Result<Vec<AgentId>> {
        let caller = call.conversation.agent;
        let db = self.realm.store().connection();
        Ok(agents::living(db)?
            .into_iter()
            .filter(|agent| *agent != caller)
            .collect())
    }

    /// Stops every running turn of every agent but the caller of
    /// `call`.
    fn stop_all(&mut self, call: &Call<'_>) -> Done<String> {
        let others = self.others(call)?;
        let working = self.working()?;
        self.carry(call, |cx, _| {
            let mut stopped = 0;
            for agent in others {
                stopped += chats::stop(cx, agent, &working)?;
            }
            Ok(Ok(format!("Stopped {stopped} running turns.")))
        })
    }

    /// Resumes every stopped agent but the caller of `call`.
    fn resume_all(&mut self, call: &Call<'_>) -> Done<String> {
        let others = self.others(call)?;
        self.carry(call, |cx, _| {
            let mut resumed = 0;
            for agent in others {
                if chats::resume(cx, agent)? {
                    resumed += 1;
                }
            }
            Ok(Ok(format!("Resumed {resumed} agents.")))
        })
    }

    /// Carries out the Hand's `dream` of a scholar or of a hire it
    /// leads.
    pub(super) fn dream_tool(
        &mut self,
        call: &Call<'_>,
        args: Dream,
    ) -> Done<String> {
        let dreamer = done!(self.dreamer(None, &args.who));
        match dreamer {
            Dreamer::Scholar(scholar) => {
                let db = self.realm.store().connection();
                let by = Actor::Agent(call.conversation.agent);
                if !authority::may_keep(db, by, scholar)? {
                    refuse!(Refusal::ScholarNotOwn {
                        name: addresses::scholar_name_of(
                            db, scholar
                        )?,
                    });
                }
            }
            Dreamer::Hire { hire, .. } => {
                done!(self.leads_hire(call, hire));
            }
        }
        self.begin(call)?;
        Ok(Ok(self.ask_dreams(dreamer)?))
    }
}

/// `papers` as a model reads them.
fn paper_text(papers: &protocol::Papers) -> String {
    let mut lines =
        vec!["# Character".to_owned(), papers.character.clone()];
    if !papers.memory.is_empty() {
        lines.push("# Memory".into());
        for file in &papers.memory {
            lines.push(format!(
                "## memory/{}\n{}",
                file.path, file.text
            ));
        }
    }
    if !papers.skills.is_empty() {
        lines.push("# Skills".into());
        for skill in &papers.skills {
            lines.push(format!(
                "- {}: {}",
                skill.name, skill.description
            ));
        }
    }
    if !papers.learned.is_empty() {
        lines.push("# Learned, not yet in its scholar's main".into());
        for path in &papers.learned {
            lines.push(format!("- {path}"));
        }
    }
    lines.join("\n")
}
