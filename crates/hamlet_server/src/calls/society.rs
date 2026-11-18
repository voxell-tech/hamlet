//! The society's tools (H35).

mod agent_acts;
pub(super) mod args;
mod hamlet_acts;
pub(crate) mod library;
mod messages;
mod profiles;
mod realm;
pub(crate) mod welcome;

use anyhow::Result;
use hamlet_store::ids::{
    Actor, AgentId, ChiefId, HamletId, ProfileId, TaskId,
};

use self::args::{
    AddAgent, AgreeChannel, Called, CreateProfile, EndAgent,
    FoundHamlet, MakeProfile, MergeAgents, ReadRecord, RenameAgent,
    SetAllowance, SetLead, SetModel, SetTools,
};
use super::looks::Look;
use super::{Call, rejected, said};
use crate::conversations::dreams::Notify;
use crate::core::Core;
use crate::knowledge::{promotion, versions};
use crate::realm::change::{Change, Done, checked, done, reject};
use crate::realm::settings::EffectiveSettings;
use crate::society::addresses::{self, Reach};
use crate::society::agents::{self, AgentKind, EndHow};
use crate::society::hamlet_agents::{
    self, LastDream, ProfileChoice, Spec,
};
use crate::society::hamlets::{self, Founding};
use crate::society::rejection::Rejection;
use crate::society::spend::{self, micros, money};
use crate::society::{authority, channels, city, limits, record};
use crate::tools::builtin::Outcome;
use crate::tools::catalogue::Society;

/// The most of a text a read gives, in bytes.
const MOST_READ: usize = 50 * 1024;

/// `words`, cut at [`MOST_READ`].
fn cut(mut words: String) -> String {
    if words.len() > MOST_READ {
        let mut end = MOST_READ;
        while !words.is_char_boundary(end) {
            end -= 1;
        }
        words.truncate(end);
        words.push_str("\n[cut here]");
    }
    words
}

/// The result of a society call.
pub(super) enum Step {
    Done(Outcome),
    /// It waits for another agent, in these words.
    Wait(String),
    /// It goes on once it has looked at this, off the loop.
    Look(Box<Look>),
}

impl Step {
    fn of(result: Result<String, Rejection>) -> Self {
        match result {
            Ok(words) => Self::Done(said(words)),
            Err(rejection) => {
                Self::Done(rejected(rejection.to_string()))
            }
        }
    }
}

impl Core<'_, '_> {
    /// Carries out the society call `call` of `tool`, which the gate
    /// let run.
    pub(super) fn society(
        &mut self,
        call: &Call<'_>,
        tool: Society,
    ) -> Result<Step> {
        let called = match Called::read(tool, call.arguments) {
            Ok(called) => called,
            Err(rejection) => return Ok(Step::of(Err(rejection))),
        };
        match called {
            Called::Notify(args) => {
                self.send_notification(call, args)
            }
            Called::Request(args) => self.send_request(call, args),
            Called::ProposeChannel(args) => {
                self.propose_channel(call, args)
            }
            Called::StartWorker(args) => {
                self.start_worker(call, args)
            }
            Called::SubmitSkill(args) => {
                self.submit_skill(call, args)
            }
            Called::RequestLibrarian(args) => {
                self.request_librarian(call, args)
            }
            called => Ok(Step::of(self.act(call, called)?)),
        }
    }

    /// Carries out a society call answered at once, in words.
    fn act(
        &mut self,
        call: &Call<'_>,
        called: Called,
    ) -> Done<String> {
        let agent = call.conversation.agent;
        let here = self.hamlet_of(agent)?;
        let here = here.as_ref();
        match called {
            Called::ReadProfile(args) => {
                self.read_profile(call, args)
            }
            Called::EditProfile(args) => {
                self.edit_profile(call, args)
            }
            Called::Tag(args) => self.tag_profile(call, args),
            Called::Revert(args) => self.revert_profile(call, args),
            Called::CreateProfile(CreateProfile {
                name,
                character,
            }) => {
                let owner = self.owner(call, here)?;
                self.carry(call, |cx, env| {
                    let character = if character.is_empty() {
                        env.settings.character.clone()
                    } else {
                        character
                    };
                    let made = done!(city::make(
                        cx, &name, owner, &character, None,
                    ));
                    Ok(Ok(arrived(cx, env, made, &character)?))
                })
            }
            Called::MakeProfile(MakeProfile {
                agent: address,
                name,
                reason,
            }) => {
                let hamlet_agent = done!(self.child_within(
                    call,
                    &address,
                    here,
                    Reach::Kept
                ));
                let owner = self.owner(call, here)?;
                let layout = self.realm.layout().clone();
                let make = promotion::make_profile(
                    self.realm.store().connection(),
                    &layout,
                    &self.writing,
                    hamlet_agent,
                    &name,
                    owner,
                    reason,
                )?;
                self.carry(call, |cx, env| {
                    let made = done!(make(cx));
                    Ok(Ok(arrived(
                        cx,
                        env,
                        made.profile,
                        &made.character,
                    )?))
                })
            }
            Called::AddAgent(args) => {
                self.add_agent(call, here, args)
            }
            Called::EndAgent(EndAgent {
                agent: address,
                dream,
                reason,
            }) => {
                let hamlet_agent =
                    done!(self.child(call, &address, here));
                let last = if dream {
                    LastDream::With
                } else {
                    LastDream::Without
                };
                let name = self.name(hamlet_agent)?;
                let ended = self.carry(call, |cx, _| {
                    Ok(hamlet_agents::end(
                        cx,
                        hamlet_agent,
                        EndHow::Ended,
                        last,
                        &reason,
                    )?
                    .map(|()| format!("Ended {name}.")))
                })?;
                match ended {
                    Ok(words) if last == LastDream::With => {
                        self.dream_tick_ending(hamlet_agent)?;
                        let dream = self.last_dream_words(
                            hamlet_agent,
                            Notify::Ender,
                        )?;
                        Ok(Ok(format!("{words} {dream}")))
                    }
                    ended => Ok(ended),
                }
            }
            Called::SetModel(SetModel {
                agent: address,
                model,
                reason,
            }) => {
                let hamlet_agent =
                    done!(self.child(call, &address, here));
                let model = (!model.is_empty()).then_some(model);
                let name = self.name(hamlet_agent)?;
                let words = format!(
                    "{name} runs {} now.",
                    model.as_deref().unwrap_or("its hamlet's model")
                );
                self.carry(call, |cx, env| {
                    Ok(crate::conversations::chats::remodel(
                        cx,
                        env,
                        hamlet_agent,
                        |cx| {
                            hamlet_agents::set_model(
                                cx,
                                hamlet_agent,
                                model.as_deref(),
                                &reason,
                            )
                        },
                    )?
                    .map(|()| words))
                })
            }
            Called::SetTools(SetTools {
                agent: address,
                off,
                on,
                reason,
            }) => {
                let hamlet_agent =
                    done!(self.child(call, &address, here));
                let name = self.name(hamlet_agent)?;
                self.carry(call, |cx, _| {
                    Ok(hamlet_agents::set_tools(
                        cx,
                        hamlet_agent,
                        &off,
                        &on,
                        &reason,
                    )?
                    .map(|()| {
                        format!("Changed the tools of {name}.")
                    }))
                })
            }
            Called::SetAllowance(SetAllowance {
                agent: address,
                dollars,
                reason,
            }) => {
                let hamlet_agent =
                    done!(self.child(call, &address, here));
                let allowance = dollars.map(micros);
                let name = self.name(hamlet_agent)?;
                self.carry(call, |cx, _| {
                    Ok(hamlet_agents::set_allowance(
                        cx,
                        hamlet_agent,
                        allowance,
                        &reason,
                    )?
                    .map(|()| {
                        format!("Set the allowance of {name}.")
                    }))
                })
            }
            Called::RenameAgent(RenameAgent {
                agent: address,
                name: typed,
                reason,
            }) => {
                let hamlet_agent =
                    done!(self.child(call, &address, here));
                let given = (!typed.is_empty()).then_some(typed);
                let name = self.name(hamlet_agent)?;
                let words = match &given {
                    Some(given) => {
                        format!("{name} is called {given} now.")
                    }
                    None => format!(
                        "{name} goes by its profile's name again."
                    ),
                };
                self.carry(call, |cx, _| {
                    Ok(hamlet_agents::set_name(
                        cx,
                        hamlet_agent,
                        given.as_deref(),
                        &reason,
                    )?
                    .map(|()| words))
                })
            }
            Called::OpenChannel(args::OpenChannel {
                a,
                b,
                reason,
            }) => {
                let a = done!(self.child(call, &a, here));
                let b = done!(self.child(call, &b, here));
                let (one, other) = (self.name(a)?, self.name(b)?);
                let task = self.task_of(call)?;
                self.carry(call, |cx, _| {
                    Ok(channels::open(cx, a, b, &reason, task)?.map(
                        |_| {
                            format!(
                                "Opened a channel between {one} and \
                                 {other}."
                            )
                        },
                    ))
                })
            }
            Called::CloseChannel(args::CloseChannel {
                a,
                b,
                reason,
            }) => {
                let a = done!(self.child(call, &a, here));
                let b = done!(self.child(call, &b, here));
                let (one, other) = (self.name(a)?, self.name(b)?);
                self.carry(call, |cx, _| {
                    Ok(channels::close(cx, a, b, &reason)?.map(
                        |()| {
                            format!(
                                "Closed the channel between {one} \
                                 and {other}."
                            )
                        },
                    ))
                })
            }
            Called::MergeAgents(MergeAgents {
                absorbed,
                into,
                reason,
            }) => {
                let absorbed =
                    done!(self.child(call, &absorbed, here));
                let kept = done!(self.child(call, &into, here));
                let (one, other) =
                    (self.name(absorbed)?, self.name(kept)?);
                self.carry(call, |cx, env| {
                    let merged = crate::society::merges::merge(
                        cx, kept, absorbed, &reason,
                    )?;
                    if merged.is_ok() {
                        crate::conversations::chats::merged(
                            cx, env, kept, absorbed,
                        )?;
                    }
                    Ok(merged.map(|_| {
                        format!("Merged {one} into {other}.")
                    }))
                })
            }
            Called::AgreeChannel(args) => {
                self.agree_channel(call, here, args)
            }
            Called::FoundHamlet(FoundHamlet {
                name,
                profile,
                purpose,
            }) => {
                let chief = self.owner(call, None)?;
                let lead = if profile.is_empty() {
                    ProfileChoice::Blank {
                        name: String::new(),
                        character: self
                            .env
                            .settings
                            .character
                            .clone(),
                    }
                } else {
                    done!(self.chosen_profile(&profile))
                };
                self.carry(call, |cx, _| {
                    let founding = Founding {
                        name: name.clone(),
                        chief,
                        lead,
                        purpose,
                    };
                    let added = done!(hamlets::found(cx, founding));
                    Ok(Ok(format!(
                        "Founded {name} (`{}`).",
                        added.hamlet
                    )))
                })
            }
            Called::SetLead(SetLead {
                agent: address,
                reason,
            }) => {
                let hamlet_agent = done!(self.named(&address, here));
                let db = self.realm.store().connection();
                let Some(found) =
                    hamlet_agents::find(db, hamlet_agent)?
                else {
                    let name = addresses::name_of(db, hamlet_agent)?;
                    reject!(Rejection::NotHamletAgent { name });
                };
                let hamlet = found.hamlet;
                let name = self.name(hamlet_agent)?;
                self.carry(call, |cx, _| {
                    Ok(hamlet_agents::set_lead(
                        cx,
                        &hamlet,
                        hamlet_agent,
                        &reason,
                    )?
                    .map(|()| {
                        format!("{name} is the lead of {hamlet} now.")
                    }))
                })
            }
            called @ (Called::RemoveHamlet(_)
            | Called::RestoreHamlet(_)
            | Called::EraseHamlet(_)
            | Called::NameHamlet(_)
            | Called::MoveHamlet(_)
            | Called::SetTier(_)
            | Called::SetFolder(_)
            | Called::SetMode(_)
            | Called::ResetBudget(_)
            | Called::EditLimits(_)) => {
                self.hamlet_tool(call, called)
            }
            called @ (Called::Versions(_)
            | Called::Fork(_)
            | Called::Purge(_)
            | Called::Welcome(_)
            | Called::TurnAway(_)
            | Called::NameProfile(_)
            | Called::MoveHouse(_)
            | Called::RemoveProfile(_)
            | Called::RestoreProfile(_)
            | Called::EraseProfile(_)) => {
                self.profile_tool(call, called)
            }
            called @ (Called::Pin(_)
            | Called::RestoreAgent(_)
            | Called::EraseAgent(_)) => self.child_tool(call, called),
            called @ (Called::SetEffort(_)
            | Called::SetFast(_)
            | Called::Reflect(_)
            | Called::Stop(_)
            | Called::Resume(_)
            | Called::EditMemory(_)
            | Called::ReadPrompt(_)
            | Called::ReadPapers(_)) => self.agent_tool(call, called),
            Called::Dream(args) => self.dream_tool(call, args),
            called @ (Called::EditSettings(_)
            | Called::EditCharter(_)
            | Called::ReadCharter(_)
            | Called::ReadModels(_)
            | Called::Unshelve(_)) => self.realm_tool(call, called),
            Called::ReadBudget(_) => {
                let db = self.realm.store().connection();
                Ok(Ok(budget(db, agent)?))
            }
            Called::ReadRecord(ReadRecord { hamlet }) => {
                let id = HamletId(hamlet);
                let db = self.realm.store().connection();
                if hamlets::find(db, &id)?.is_none() {
                    reject!(Rejection::NoHamlet {
                        id: id.to_string()
                    });
                }
                Ok(Ok(record::lines(db, &id, 0)?))
            }
            // The calls answer `object` before the gate, and these
            // before here.
            Called::Object(_)
            | Called::Notify(_)
            | Called::Request(_)
            | Called::ProposeChannel(_)
            | Called::StartWorker(_)
            | Called::SubmitSkill(_)
            | Called::RequestLibrarian(_) => {
                Ok(Err(unsupported(call)))
            }
        }
    }

    /// The chief who owns what `call`'s agent makes in `here`.
    fn owner(
        &mut self,
        call: &Call<'_>,
        here: Option<&HamletId>,
    ) -> Result<ChiefId> {
        let task = self.task_of(call)?;
        let db = self.realm.store().connection();
        let chief = task_chief(db, task)?;
        let by = Actor::Agent(call.conversation.agent);
        authority::owner_for(db, by, here, chief)
    }

    /// Makes the change of `call` and keeps its answer in the same
    /// transaction, so a call replayed after a crash never changes
    /// anything twice.
    fn carry(
        &mut self,
        call: &Call<'_>,
        make: impl FnOnce(
            &mut Change<'_>,
            &EffectiveSettings,
        ) -> Done<String>,
    ) -> Done<String> {
        let identity = call.call.identity();
        let request = &call.request;
        let split = crate::app::fault::is_marker_split();
        let made = self.attempt(
            Actor::Agent(call.conversation.agent),
            |cx, env| {
                let made = make(cx, env)?;
                if let Ok(words) = &made
                    && !split
                {
                    hamlet_store::marker::settle(
                        cx.db,
                        identity,
                        request,
                        &said(words.clone()),
                        cx.at,
                    )?;
                }
                Ok(made)
            },
        )?;
        if made.is_err() {
            return Ok(made);
        }
        // The crash test's fault point comes once the change is kept:
        // with its answer at the input's end, or apart from it.
        if split {
            hamlet_store::commit(self.realm.store().connection())?;
            crate::app::fault::point("carried");
        } else {
            self.changed.crash_test_fault_point = Some("carried");
        }
        Ok(made)
    }

    /// Marks `call` begun before an effect outside the store, so a
    /// call replayed after a crash is answered as interrupted.
    fn begin(&mut self, call: &Call<'_>) -> Result<()> {
        let db = self.realm.store().connection();
        hamlet_store::marker::begin(
            db,
            call.call.identity(),
            &call.request,
            crate::app::clock::now(),
        )
    }

    /// The hamlet `agent` speaks in, when it is an agent of one.
    pub(super) fn hamlet_of(
        &mut self,
        agent: AgentId,
    ) -> Result<Option<HamletId>> {
        let db = self.realm.store().connection();
        Ok(hamlet_agents::find(db, agent)?.map(|found| found.hamlet))
    }

    pub(super) fn name(&mut self, agent: AgentId) -> Result<String> {
        let db = self.realm.store().connection();
        addresses::name_of(db, agent)
    }

    /// The living agent at `address`, read from `here`.
    pub(super) fn named(
        &mut self,
        address: &str,
        here: Option<&HamletId>,
    ) -> Done<AgentId> {
        self.named_within(address, here, Reach::Living)
    }

    /// The agent within `reach` at `address`, read from `here`.
    fn named_within(
        &mut self,
        address: &str,
        here: Option<&HamletId>,
        reach: Reach,
    ) -> Done<AgentId> {
        let address = checked!(addresses::parse(address));
        let db = self.realm.store().connection();
        addresses::resolve(db, &address, here, reach)
    }

    /// The living agent of a hamlet at `address`, which `call`'s
    /// agent is a parent of.
    fn child(
        &mut self,
        call: &Call<'_>,
        address: &str,
        here: Option<&HamletId>,
    ) -> Done<AgentId> {
        self.child_within(call, address, here, Reach::Living)
    }

    /// The agent of a hamlet within `reach` at `address`, which
    /// `call`'s agent is a parent of.
    fn child_within(
        &mut self,
        call: &Call<'_>,
        address: &str,
        here: Option<&HamletId>,
        reach: Reach,
    ) -> Done<AgentId> {
        let agent = done!(self.named_within(address, here, reach));
        Ok(self.require_child(call, agent)?.map(|()| agent))
    }

    /// Rejects unless `agent` is an agent of a hamlet `call`'s agent
    /// is a parent of.
    fn require_child(
        &mut self,
        call: &Call<'_>,
        agent: AgentId,
    ) -> Done<()> {
        let caller = call.conversation.agent;
        let db = self.realm.store().connection();
        if agents::get(db, agent)?.kind != AgentKind::Hamlet {
            let name = addresses::name_of(db, agent)?;
            reject!(Rejection::NotHamletAgent { name });
        }
        if agent == caller
            || !authority::may_direct(
                db,
                Actor::Agent(caller),
                agent,
            )?
        {
            let name = addresses::name_of(db, agent)?;
            reject!(Rejection::NotParent { name });
        }
        Ok(Ok(()))
    }

    /// The profile `text` addresses, as an agent takes it: at `main`,
    /// or pinned to `@<version>`.
    fn chosen_profile(&mut self, text: &str) -> Done<ProfileChoice> {
        let layout = self.realm.layout().clone();
        let db = self.realm.store().connection();
        let (profile, version) =
            done!(addresses::profile_pinned(db, text));
        versions::existing(db, &layout, profile, version)
    }

    fn add_agent(
        &mut self,
        call: &Call<'_>,
        here: Option<&HamletId>,
        args: AddAgent,
    ) -> Done<String> {
        let AddAgent {
            profile: named,
            name: blank,
            purpose,
            under,
            hamlet: named_hamlet,
        } = args;
        let agent = call.conversation.agent;
        let hamlet = match (here, named_hamlet.is_empty()) {
            (Some(here), _) => here.clone(),
            (None, false) => HamletId(named_hamlet),
            (None, true) => reject!(Rejection::NoHamletToAddTo),
        };
        let parent = if under.is_empty() {
            let db = self.realm.store().connection();
            if agents::get(db, agent)?.kind == AgentKind::Hand {
                if hamlets::find(db, &hamlet)?.is_none() {
                    reject!(Rejection::NoHamlet {
                        id: hamlet.to_string(),
                    });
                }
                crate::society::tree::lead(db, &hamlet)?
            } else {
                agent
            }
        } else {
            let found = done!(self.named(&under, Some(&hamlet)));
            let db = self.realm.store().connection();
            if found != agent
                && !authority::may_direct(
                    db,
                    Actor::Agent(agent),
                    found,
                )?
            {
                let name = addresses::name_of(db, found)?;
                reject!(Rejection::NotParent { name });
            }
            found
        };
        // A profile of the city, or a blank agent when none fits.
        let choice = match (named.is_empty(), blank.is_empty()) {
            (false, true) => done!(self.chosen_profile(&named)),
            (true, _) => ProfileChoice::Blank {
                name: blank,
                character: self.env.settings.character.clone(),
            },
            (false, false) => reject!(Rejection::AddBoth),
        };
        let db = self.realm.store().connection();
        if let ProfileChoice::Existing { profile, .. } = &choice
            && !authority::may_add(
                db,
                Actor::Agent(agent),
                *profile,
                &hamlet,
            )?
        {
            let name = city::get(db, *profile)?.label();
            reject!(Rejection::ProfileNotOwn { name });
        }
        let owner = self.owner(call, Some(&hamlet))?;
        // An agent says what it adds an agent for; only a chief may
        // leave it to the new agent's first message.
        if purpose.is_empty() {
            reject!(Rejection::NoPurpose);
        }
        self.carry(call, |cx, _| {
            let added = done!(hamlet_agents::add(
                cx,
                Spec {
                    hamlet,
                    parent: Some(parent),
                    profile: choice,
                    purpose,
                    reason: String::new(),
                    owner,
                },
            ));
            let name = addresses::name_of(cx.db, added.agent)?;
            Ok(Ok(format!("Added {name}.")))
        })
    }

    fn agree_channel(
        &mut self,
        call: &Call<'_>,
        here: Option<&HamletId>,
        args: AgreeChannel,
    ) -> Done<String> {
        let AgreeChannel {
            mine,
            theirs,
            reason,
        } = args;
        let agent = call.conversation.agent;
        let db = self.realm.store().connection();
        if !hamlet_agents::is_lead(db, agent)? {
            reject!(Rejection::NotLead);
        }
        let mine = done!(self.named(&mine, here));
        let theirs = done!(self.named(&theirs, None));
        let task = self.task_of(call)?;
        let (one, other) = (self.name(mine)?, self.name(theirs)?);
        self.carry(call, |cx, _| {
            Ok(channels::agree(
                cx, agent, mine, theirs, &reason, task,
            )?
            .map(|agreed| match agreed {
                channels::Agreed::Opened(_) => format!(
                    "The channel between {one} and {other} is open."
                ),
                channels::Agreed::Waiting => format!(
                    "You agree a channel between {one} and {other}; \
                     it opens once the other lead agrees."
                ),
            }))
        })
    }
}

/// Where the profile `id`, just made, arrived, for its maker.
fn arrived(
    cx: &mut Change<'_>,
    env: &EffectiveSettings,
    id: ProfileId,
    character: &str,
) -> Result<String> {
    let made = city::get(cx.db, id)?;
    let name = made.label();
    if made.arrival == city::Arrival::Housed {
        return Ok(format!(
            "The profile {name} lives in the city; add an agent of \
             it next."
        ));
    }
    if city::awaits_hand(cx.db, id)? {
        welcome::judge(cx, env, id, character)?;
    }
    Ok(format!(
        "The profile {name} is made and waits at the harbor until \
         it is let into the city; you can add an agent of it now."
    ))
}

/// The budget of `agent`, for `read_budget`.
fn budget(
    db: &mut hamlet_store::Db,
    agent: AgentId,
) -> Result<String> {
    let at = crate::app::clock::now();
    let payer = crate::conversations::tasks::payer_of(db, agent)?;
    let report = spend::report(db, payer.as_ref(), at)?;
    let mut lines = vec![report.summary];
    let spent = spend::of_agent(db, agent, at)?;
    lines.push(format!(
        "Your spend this period: {}.",
        money(spent.micros)
    ));
    if let Some(allowance) = hamlet_agents::find(db, agent)?
        .and_then(|found| found.allowance)
    {
        lines.push(format!(
            "Your allowance, with what everything under you spends: \
             {}.",
            money(allowance)
        ));
    }
    Ok(lines.join("\n"))
}

/// The rejection of a tool the dispatcher did not expect.
pub(super) fn unsupported(call: &Call<'_>) -> Rejection {
    Rejection::Unsupported {
        what: format!("the tool {}", call.tool),
    }
}

pub(super) fn task_chief(
    db: &mut hamlet_store::Db,
    task: Option<TaskId>,
) -> Result<Option<ChiefId>> {
    Ok(match task {
        Some(task) => {
            crate::conversations::tasks::get(db, task)?.chief
        }
        None => None,
    })
}

/// The limits of `hamlet`, or the defaults outside every hamlet.
pub(super) fn limits_of(
    db: &mut hamlet_store::Db,
    hamlet: Option<&HamletId>,
) -> Result<limits::Limits> {
    Ok(match hamlet {
        Some(hamlet) => limits::of(db, hamlet)?,
        None => limits::Limits::default(),
    })
}
