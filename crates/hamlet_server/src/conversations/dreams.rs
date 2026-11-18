//! The dreams of agents: one dream at a time, while dreams are on and
//! in the chief's hours, or when a chief calls for one with `/dream`.
//! An agent dreams once it learned something its profile lacks, an
//! ended agent once more, and a profile at most once a day to tidy
//! its `main`.

use anyhow::Result;
use hamlet_git::{MAIN, Repository};
use hamlet_store::ids::{Actor, AgentId, DreamId, ProfileId};

use crate::app::timers::AddTimer;
use crate::conversations::inputs::{self, InputKind, Keep};
use crate::conversations::marks::Mark;
use crate::conversations::tasks::{self, Origin};
use crate::conversations::{self, Begin, Kind, models};
use crate::core::Core;
use crate::realm::change::Done;
use crate::society::dreams::{self, DreamKind, Outcome};
use crate::society::rejection::Rejection;
use crate::society::{addresses, agents, city, hamlet_agents, hand};
use crate::{knowledge, runtimes};

/// The least time between two dreams of one agent.
const AGENT_REST: i64 = 6 * 3_600_000;
/// The least time between two dreams of one profile.
const PROFILE_REST: i64 = 24 * 3_600_000;
/// The time after its end an agent may still dream once more.
const ENDING_WINDOW: i64 = 24 * 3_600_000;
/// The time between the server's looks for a due dream.
const DREAM_CHECK: std::time::Duration =
    std::time::Duration::from_secs(60);

pub(crate) fn plugin(app: &mut bevy_app::App) {
    app.add_every(DREAM_CHECK, dream_tick);
}

fn dream_tick(mut core: Core) -> Result<()> {
    core.dream_tick()
}

/// An agent or a profile that dreams.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Dreamer {
    /// An agent of a hamlet.
    Agent {
        agent: AgentId,
        ending: bool,
    },
    Profile(ProfileId),
}

impl Core<'_, '_> {
    /// Starts a dream when one is due.
    pub(crate) fn dream_tick(&mut self) -> Result<()> {
        let dreams = &self.env.settings.dreams;
        if !dreams.on {
            return Ok(());
        }
        let hour = u32::try_from(
            crate::app::clock::local(crate::app::clock::now())?
                .hour(),
        )?;
        let (from, to) =
            (u32::from(dreams.hours.0), u32::from(dreams.hours.1));
        let in_hours = if from <= to {
            (from..to).contains(&hour)
        } else {
            hour >= from || hour < to
        };
        if !in_hours {
            return Ok(());
        }
        let db = self.realm.store().connection();
        if dreams::running(db)?.is_some() {
            return Ok(());
        }
        if let Some(dreamer) = self.due()? {
            self.dream(dreamer)?;
        }
        Ok(())
    }

    /// The dreamer most due, if one is.
    fn due(&mut self) -> Result<Option<Dreamer>> {
        let layout = self.realm.layout().clone();
        let now = crate::app::clock::now();
        let db = self.realm.store().connection();
        for agent in agents::all_hamlet_agents(db)? {
            let found = agents::get(db, agent)?;
            if found.erased || inputs::is_agent_busy(db, agent)? {
                continue;
            }
            let ending = match found.ended_at {
                None => false,
                Some(at) if now - at < ENDING_WINDOW => true,
                Some(_) => continue,
            };
            let last = dreams::last_of_agent(db, agent)?;
            if ending && last.as_ref().is_some_and(|d| d.ending) {
                continue;
            }
            if !ending
                && last
                    .as_ref()
                    .is_some_and(|d| now - d.began < AGENT_REST)
            {
                continue;
            }
            let hamlet_agent = hamlet_agents::get(db, agent)?;
            let Ok(repository) = Repository::open(
                &layout.repository(hamlet_agent.profile),
            ) else {
                continue;
            };
            let (Some(tip), Some(main)) = (
                repository.tip(&knowledge::branch(agent))?,
                repository.tip(MAIN)?,
            ) else {
                continue;
            };
            let dreamt = last.and_then(|d| d.branch_commit)
                == Some(tip.to_string());
            if !dreamt
                && repository.merge_base(tip, main)? != Some(tip)
            {
                return Ok(Some(Dreamer::Agent { agent, ending }));
            }
        }
        for profile in city::all(db)? {
            if !profile.is_living() || !profile.planted {
                continue;
            }
            let rested = dreams::last_of_profile(db, profile.id)?
                .is_none_or(|d| now - d.began >= PROFILE_REST);
            let has_agents =
                !hamlet_agents::living_of(db, profile.id)?.is_empty();
            if rested && has_agents {
                return Ok(Some(Dreamer::Profile(profile.id)));
            }
        }
        Ok(None)
    }

    /// `/dream <profile#id|address>`: dreams now, outside the hours.
    pub(crate) fn dream_command(
        &mut self,
        here: Option<&hamlet_store::ids::HamletId>,
        words: &[&str],
    ) -> Done<String> {
        let [target] = words else {
            return Ok(Err(Rejection::DreamUsage));
        };
        let db = self.realm.store().connection();
        if dreams::running(db)?.is_some() {
            return Ok(Err(Rejection::DreamRunning));
        }
        let dreamer = if target.starts_with("profile#") {
            let found = match addresses::parse_profile(target) {
                Ok(id) => city::living(db, id)?,
                Err(rejection) => return Ok(Err(rejection)),
            };
            match found {
                Some(found) => Dreamer::Profile(found.id),
                None => {
                    return Ok(Err(Rejection::NoProfile {
                        address: (*target).into(),
                    }));
                }
            }
        } else {
            let address = match addresses::parse(target) {
                Ok(address) => address,
                Err(rejection) => return Ok(Err(rejection)),
            };
            let agent = match addresses::resolve(
                db,
                &address,
                here,
                addresses::Reach::Kept,
            )? {
                Ok(agent) => agent,
                Err(rejection) => return Ok(Err(rejection)),
            };
            if let Err(rejection) = self.directs(agent)? {
                return Ok(Err(rejection));
            }
            let db = self.realm.store().connection();
            let ending = !agents::get(db, agent)?.is_living();
            Dreamer::Agent { agent, ending }
        };
        Ok(Ok(match self.dream(dreamer)? {
            Some(_) => "The dream began.".into(),
            None => "There is nothing to dream about.".into(),
        }))
    }

    /// Starts `dreamer`'s dream. `None` when there is nothing to
    /// dream about.
    pub(crate) fn dream(
        &mut self,
        dreamer: Dreamer,
    ) -> Result<Option<DreamId>> {
        let layout = self.realm.layout().clone();
        let db = self.realm.store().connection();
        let (kind, profile, hamlet_agent, ending, agent) =
            match dreamer {
                Dreamer::Agent { agent, ending } => {
                    let found = hamlet_agents::get(db, agent)?;
                    (
                        DreamKind::Agent,
                        found.profile,
                        Some(agent),
                        ending,
                        agent,
                    )
                }
                Dreamer::Profile(profile) => (
                    DreamKind::Profile,
                    profile,
                    None,
                    false,
                    hand::id(db)?,
                ),
            };
        let id = self.change(Actor::Realm, |cx, _| {
            dreams::begin(cx, kind, profile, hamlet_agent, ending)
        })?;
        let writing = self.writing.clone();
        let held = writing.hold();
        let db = self.realm.store().connection();
        let ready = knowledge::dreams::prepare(
            db,
            &layout,
            id,
            crate::app::clock::now(),
        )?;
        drop(held);
        let db = self.realm.store().connection();
        if !ready {
            dreams::end(
                db,
                id,
                Outcome::Nothing,
                None,
                None,
                crate::app::clock::now(),
            )?;
            return Ok(None);
        }
        let checkout = knowledge::dreams::checkout(&layout, id);
        let label = city::get(db, profile)?.label();
        let instructions = match hamlet_agent {
            Some(hamlet_agent) => {
                let worktree =
                    knowledge::worktree(db, &layout, hamlet_agent)?
                        .path;
                let who = addresses::name_of(db, hamlet_agent)?;
                format!(
                    "You are dreaming, between your work as \
                     `{who}`, an agent of the profile `{label}`. \
                     Your worktree at {} holds what you learned as \
                     this agent: your memory/ and skills/. \
                     `{label}`'s shared knowledge, its main, is \
                     checked out at {}; every agent of `{label}` \
                     starts from it and takes it in.\n\nBring home \
                     what any agent of `{label}` would want to \
                     know: move or merge it into the checkout's \
                     memory/ and skills/, written for a reader who \
                     was not there. Keep in your worktree what only \
                     this agent needs. Tidy as you go: one fact in \
                     one place, nothing stale, the root memory \
                     files short, and a true `description:` on \
                     every memory file. Do not change CHARACTER.md. \
                     When done, answer in one line what you brought \
                     home, or `Nothing to bring home.`",
                    worktree.display(),
                    checkout.display()
                )
            }
            None => format!(
                "You are tidying the knowledge of the profile \
                 `{label}`, checked out at {}: its memory/ and \
                 skills/. Merge what is said twice, correct or \
                 remove what is stale, keep the root memory files \
                 short and every memory file's `description:` true, \
                 and make each memory folder's MEMORY.md describe \
                 it. Do not change CHARACTER.md. Answer in one line \
                 what you changed, or `Nothing to tidy.`",
                checkout.display()
            ),
        };
        let names = self.tools.catalogue.names();
        self.change(Actor::Realm, |cx, env| {
            let harness = agents::get(cx.db, agent)?.harness;
            let runtime = runtimes::of_harness(cx.db, &harness)?;
            let choice = models::choose(
                cx.db,
                &env.settings,
                &env.overrides,
                agent,
            )?;
            let tools = crate::calls::place::tools(
                cx.db,
                agent,
                Kind::Dream,
                &names,
            )?;
            let mut opening = Begin::new(agent, Kind::Dream);
            opening.instructions = Some(instructions);
            let conversation = conversations::begin(
                cx,
                opening,
                &runtime,
                &choice.model,
                Some(&tools),
            )?;
            let task =
                tasks::mint(cx, Origin::Dream, agent, None, None)?;
            inputs::keep(
                cx,
                Keep {
                    conversation,
                    request: format!("dream:{id}"),
                    kind: InputKind::Dream,
                    task: Some(task),
                    content: "Dream now.".into(),
                    mark: Mark::Dream { at: cx.at },
                    steer: false,
                    boundary: false,
                },
            )?;
            dreams::runs_in(cx.db, id, conversation, task)
        })?;
        Ok(Some(id))
    }
}
