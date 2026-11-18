//! The Hand's acts on an agent.

use anyhow::Result;
use hamlet_protocol::client::{self as protocol, Keeper};
use hamlet_store::ids::{Actor, AgentId};

use super::args::{
    Called, Dream, EditMemory, EraseAgent, Pin, ReadPapers,
    ReadPrompt, Reflect, RestoreAgent, Resume, SetEffort, SetFast,
    Stop,
};
use super::{Call, cut, unsupported};
use crate::conversations::dreams::Dreamer;
use crate::conversations::{self, chats, reflections};
use crate::core::Core;
use crate::knowledge::memory_path;
use crate::knowledge::papers::papers;
use crate::knowledge::versions::{pin_target, pinned};
use crate::realm::change::{Done, checked, done, reject};
use crate::society::addresses::{self, Reach};
use crate::society::agents::{self, Effort};
use crate::society::rejection::Rejection;
use crate::society::{authority, hamlet_agents, merges};

/// The address that names every agent but the caller.
const ALL: &str = "all";

impl Core<'_, '_> {
    /// Carries out the Hand's `called` on an agent it is a parent
    /// of.
    pub(super) fn child_tool(
        &mut self,
        call: &Call<'_>,
        called: Called,
    ) -> Done<String> {
        match called {
            Called::Pin(Pin {
                agent: address,
                version,
                reason,
            }) => {
                let agent = done!(self.child_within(
                    call,
                    &address,
                    None,
                    Reach::Living
                ));
                let name = self.name(agent)?;
                let layout = self.realm.layout().clone();
                let db = self.realm.store().connection();
                let target =
                    done!(pin_target(db, &layout, agent, &version));
                self.carry(call, |cx, _| {
                    Ok(hamlet_agents::set_pin(
                        cx,
                        agent,
                        target.as_deref(),
                        &reason,
                    )?
                    .map(|()| pinned(&name, target.as_deref())))
                })
            }
            Called::RestoreAgent(RestoreAgent {
                agent: address,
                reason,
            }) => {
                let agent = done!(self.child_within(
                    call,
                    &address,
                    None,
                    Reach::Kept
                ));
                let name = self.name(agent)?;
                // A merged agent comes back by splitting its merge.
                let db = self.realm.store().connection();
                let merge = merges::of_absorbed(db, agent)?;
                self.carry(call, |cx, _| {
                    Ok(match &merge {
                        Some(merge) => merges::split(
                            cx, merge.id, &reason,
                        )?
                        .map(|()| {
                            format!(
                                "{name} is back; what it brought to \
                                 the agent it was merged into is \
                                 taken out again."
                            )
                        }),
                        None => hamlet_agents::come_back(
                            cx, agent, &reason,
                        )?
                        .map(|()| format!("{name} is back.")),
                    })
                })
            }
            Called::EraseAgent(EraseAgent { agent: address }) => {
                let agent = done!(self.child_within(
                    call,
                    &address,
                    None,
                    Reach::Kept
                ));
                let name = self.name(agent)?;
                self.carry(call, |cx, _| {
                    Ok(hamlet_agents::erase(cx, agent)?.map(|()| {
                        format!("{name} was erased for good.")
                    }))
                })
            }
            _ => reject!(unsupported(call)),
        }
    }

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
            _ => reject!(unsupported(call)),
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
            reject!(Rejection::OwnSteering);
        }
        if !allowed {
            reject!(Rejection::NotYours { name });
        }
        match called {
            Called::SetEffort(SetEffort { effort, .. }) => {
                let effort = match effort.as_str() {
                    "" | "none" => None,
                    level => match level.parse::<Effort>() {
                        Ok(effort) => Some(effort),
                        Err(_) => reject!(Rejection::NoEffort {
                            level: level.to_owned(),
                        }),
                    },
                };
                self.carry(call, |cx, env| {
                    Ok(chats::remodel(cx, env, agent, |cx| {
                        agents::set_effort(cx, agent, effort)
                    })?
                    .map(|()| match effort {
                        Some(effort) => {
                            format!("{name} thinks at {effort}.")
                        }
                        None => format!(
                            "{name} thinks as hard as the realm's \
                             setting says."
                        ),
                    }))
                })
            }
            Called::SetFast(SetFast { fast, .. }) => {
                self.carry(call, |cx, _| {
                    Ok(agents::set_fast(cx, agent, fast)?.map(|()| {
                        let word = if fast { "on" } else { "off" };
                        format!(
                            "{name} calls for the fast tier: {word}."
                        )
                    }))
                })
            }
            Called::Reflect(_) => self.carry(call, |cx, env| {
                let conversation = chats::current(cx, env, agent)?;
                Ok(Ok(
                    match reflections::reflect(cx, conversation)? {
                        Some(_) => {
                            format!("{name} reflects now.")
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
                    reject!(Rejection::StopSelf);
                }
                let working = self.working()?;
                self.carry(call, |cx, _| {
                    let stopped = chats::stop(cx, agent, &working)?;
                    let turns =
                        if stopped == 1 { "turn" } else { "turns" };
                    Ok(Ok(format!(
                        "Stopped {stopped} running {turns} of \
                         {name}."
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
                let path = checked!(memory_path(&path));
                self.begin(call)?;
                self.write_worktree_memory(
                    by, agent, &path, &content,
                )?;
                Ok(Ok(format!("Wrote {path} of {name}'s worktree.")))
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
                let layout = self.realm.layout().clone();
                let db = self.realm.store().connection();
                Ok(papers(db, &layout, of)?
                    .map(|papers| cut(paper_text(&papers))))
            }
        }
    }

    /// Every living agent but `call`'s.
    fn others(&mut self, call: &Call<'_>) -> Result<Vec<AgentId>> {
        let caller = call.conversation.agent;
        let db = self.realm.store().connection();
        Ok(agents::living(db)?
            .into_iter()
            .filter(|agent| *agent != caller)
            .collect())
    }

    fn stop_all(&mut self, call: &Call<'_>) -> Done<String> {
        let others = self.others(call)?;
        let working = self.working()?;
        self.carry(call, |cx, _| {
            let mut stopped = 0;
            for agent in others {
                stopped += chats::stop(cx, agent, &working)?;
            }
            let turns = if stopped == 1 { "turn" } else { "turns" };
            Ok(Ok(format!("Stopped {stopped} running {turns}.")))
        })
    }

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

    pub(super) fn dream_tool(
        &mut self,
        call: &Call<'_>,
        args: Dream,
    ) -> Done<String> {
        let dreamer = done!(self.dreamer(None, &args.who));
        match dreamer {
            Dreamer::Profile(profile) => {
                let db = self.realm.store().connection();
                let by = Actor::Agent(call.conversation.agent);
                if !authority::may_keep(db, by, profile)? {
                    reject!(Rejection::ProfileNotOwn {
                        name: addresses::profile_name_of(
                            db, profile
                        )?,
                    });
                }
            }
            Dreamer::Agent { agent, .. } => {
                done!(self.require_child(call, agent));
            }
        }
        self.begin(call)?;
        Ok(Ok(self.ask_dreams(dreamer)?))
    }
}

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
        lines.push("# Learned, not yet in its profile's main".into());
        for path in &papers.learned {
            lines.push(format!("- {path}"));
        }
    }
    lines.join("\n")
}
