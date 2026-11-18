//! The order agents dream in, one at a time.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use bevy_app::Startup;
use bevy_ecs::message::{Message, Messages};
use bevy_ecs::resource::Resource;
use bevy_ecs::system::ResMut;
use bevy_log::error;
use hamlet_protocol::client::ItemEvent;
use hamlet_store::Db;
use hamlet_store::ids::{Actor, AgentId, DreamId, ProfileId};

use crate::app::input::AddHandler;
use crate::app::timers::AddTimer;
use crate::conversations::inputs::{self, InputKind, Keep};
use crate::conversations::marks::Mark;
use crate::conversations::messages::{self, MessageKind};
use crate::conversations::tasks::{self, Origin};
use crate::conversations::{self, Begin, Kind, models, reflections};
use crate::core::Core;
use crate::realm::Realm;
use crate::realm::change::{Done, checked, done, reject};
use crate::realm::jobs::Job;
use crate::realm::layout::Layout;
use crate::realm::settings::Dreams;
use crate::society::dreams::{
    self, Asked, Dream, DreamKind, Outcome,
};
use crate::society::rejection::Rejection;
use crate::society::{addresses, agents, city, hamlet_agents, hand};
use crate::{knowledge, runtimes};

/// The least time between two dreams of one agent.
const AGENT_REST: i64 = 6 * 3_600_000;
/// The least time between two dreams of one profile.
const PROFILE_REST: i64 = 24 * 3_600_000;
/// The time after its end an agent may still dream once more.
const ENDING_WINDOW: i64 = 24 * 3_600_000;
const DREAM_CHECK: std::time::Duration =
    std::time::Duration::from_secs(60);

pub(crate) fn plugin(app: &mut bevy_app::App) {
    app.init_resource::<Learned>()
        .add_systems(Startup, note_learned)
        .add_handler::<DreamEnded, _>(start_next_dream)
        .add_every(DREAM_CHECK, dream_tick);
}

fn dream_tick(mut core: Core) -> Result<()> {
    core.dream_tick()
}

/// Dreams ended, each with the first line of its answer.
#[derive(Message)]
pub(crate) struct DreamEnded(pub Vec<(DreamId, String)>);

fn start_next_dream(
    mut ended: ResMut<Messages<DreamEnded>>,
    mut core: Core,
) -> bevy_ecs::error::Result {
    let ended = ended.drain().flat_map(|e| e.0).collect::<Vec<_>>();
    for (dream, summary) in ended {
        core.dreamt(dream, &summary, Notify::Ender)?;
    }
    core.dream_tick()?;
    Ok(())
}

/// Who learns how the last dream of an ended agent ended.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Notify {
    /// The agent that ended it, by a notification.
    Ender,
    /// The call that ended it, in its answer.
    Answered,
}

/// The living agents that learned what no dream took home yet.
#[derive(Resource, Default)]
pub(crate) struct Learned(pub BTreeSet<AgentId>);

impl Learned {
    pub fn take_in(&mut self, learning: &BTreeMap<AgentId, bool>) {
        for (&agent, &has) in learning {
            if has {
                self.0.insert(agent);
            } else {
                self.0.remove(&agent);
            }
        }
    }
}

/// Whether each agent the jobs `done` may have changed has learned
/// anything.
pub(crate) fn learning(
    db: &mut Db,
    layout: &Layout,
    done: &[Job],
) -> BTreeMap<AgentId, bool> {
    let mut learning = BTreeMap::new();
    for job in done {
        let found = learners(db, job).and_then(|agents| {
            agents
                .into_iter()
                .map(|agent| {
                    Ok((agent, has_learned(db, layout, agent)?))
                })
                .collect::<Result<Vec<_>>>()
        });
        match found {
            Ok(found) => learning.extend(found),
            Err(error) => {
                error!("finding what agents learned: {error:#}");
            }
        }
    }
    learning
}

/// The agents whose branch, or whose profile's `main`, `job` changes.
fn learners(db: &mut Db, job: &Job) -> Result<Vec<AgentId>> {
    let of_profile = |db: &mut Db, profile, also: Option<AgentId>| {
        let mut agents = hamlet_agents::living_of(db, profile)?
            .into_iter()
            .map(|found| found.agent)
            .collect::<Vec<_>>();
        agents.extend(also);
        anyhow::Ok(agents)
    };
    Ok(match job {
        Job::Worktree { agent }
        | Job::Commit { agent }
        | Job::TakeIn { agent }
        | Job::EraseAgent { agent } => vec![*agent],
        Job::BringHome { agent } => {
            let profile = hamlet_agents::get(db, *agent)?.profile;
            of_profile(db, profile, Some(*agent))?
        }
        Job::DreamEnd { dream, .. } => {
            let dream = dreams::get(db, *dream)?;
            of_profile(db, dream.profile, dream.agent)?
        }
        Job::Plant { profile, .. }
        | Job::EraseProfile { profile } => {
            of_profile(db, *profile, None)?
        }
        Job::Merge { merge } | Job::Split { merge } => {
            let merge = crate::society::merges::get(db, *merge)?;
            vec![merge.kept, merge.absorbed]
        }
        Job::Shelve { offer } => {
            vec![crate::society::library::get(db, *offer)?.agent]
        }
        Job::EraseHamlet { .. } => Vec::new(),
    })
}

fn has_learned(
    db: &mut Db,
    layout: &Layout,
    agent: AgentId,
) -> Result<bool> {
    Ok(hamlet_agents::find(db, agent)?.is_some()
        && agents::get(db, agent)?.is_living()
        && knowledge::dreams::has_learned(db, layout, agent)?)
}

fn note_learned(
    mut realm: ResMut<Realm>,
    mut learned: ResMut<Learned>,
) {
    let layout = realm.layout().clone();
    let db = realm.store().connection();
    let mut find = || {
        let mut found = BTreeSet::new();
        for agent in agents::all_hamlet_agents(db)? {
            if has_learned(db, &layout, agent)? {
                found.insert(agent);
            }
        }
        anyhow::Ok(found)
    };
    match find() {
        Ok(found) => learned.0 = found,
        Err(error) => {
            error!("finding what agents learned: {error:#}");
        }
    }
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

impl Dreamer {
    fn asked(self) -> Asked {
        match self {
            Self::Agent { agent, .. } => Asked::Agent(agent),
            Self::Profile(profile) => Asked::Profile(profile),
        }
    }

    fn is_of(self, dream: &Dream) -> bool {
        match self {
            Self::Agent { agent, .. } => dream.agent == Some(agent),
            Self::Profile(profile) => {
                dream.kind == DreamKind::Profile
                    && dream.profile == profile
            }
        }
    }
}

/// The dream to start next, when none is running.
pub(crate) fn next(
    db: &mut Db,
    layout: &Layout,
    settings: &Dreams,
    now: i64,
) -> Result<Option<Dreamer>> {
    if dreams::running(db)?.is_some() {
        return Ok(None);
    }
    for asked in dreams::asked(db)? {
        let dreamer = match asked {
            Asked::Agent(agent) => {
                let found = agents::get(db, agent)?;
                (!found.erased).then_some(Dreamer::Agent {
                    agent,
                    ending: !found.is_living(),
                })
            }
            Asked::Profile(profile) => city::living(db, profile)?
                .map(|_| Dreamer::Profile(profile)),
        };
        match dreamer {
            // An ended agent's dream waits for its reflection.
            Some(Dreamer::Agent {
                agent,
                ending: true,
            }) if reflections::is_reflecting(db, agent)? => {}
            Some(dreamer) => return Ok(Some(dreamer)),
            None => dreams::forget(db, asked)?,
        }
    }
    for (agent, ended) in
        hamlet_agents::ended_to_dream(db, now - ENDING_WINDOW)?
    {
        let dreamt = dreams::last_of_agent(db, agent)?
            .is_some_and(|d| d.ending && d.began >= ended);
        if !dreamt && !reflections::is_reflecting(db, agent)? {
            return Ok(Some(Dreamer::Agent {
                agent,
                ending: true,
            }));
        }
    }
    if !settings.on || !in_hours(settings.hours, now)? {
        return Ok(None);
    }
    due(db, layout, now)
}

fn in_hours(hours: (u8, u8), now: i64) -> Result<bool> {
    let hour = crate::app::clock::local(now)?.hour().unsigned_abs();
    let (from, to) = hours;
    Ok(if from <= to {
        (from..to).contains(&hour)
    } else {
        hour >= from || hour < to
    })
}

fn due(
    db: &mut Db,
    layout: &Layout,
    now: i64,
) -> Result<Option<Dreamer>> {
    for agent in agents::all_hamlet_agents(db)? {
        if !agents::get(db, agent)?.is_living()
            || inputs::is_agent_busy(db, agent)?
        {
            continue;
        }
        let rested = dreams::last_of_agent(db, agent)?
            .is_none_or(|d| now - d.began >= AGENT_REST);
        if rested
            && knowledge::dreams::has_learned(db, layout, agent)?
        {
            return Ok(Some(Dreamer::Agent {
                agent,
                ending: false,
            }));
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

/// How `dream` ended, in one line, given its answer's first line.
fn brought(dream: &Dream, summary: &str) -> String {
    match dream.outcome {
        Some(Outcome::Done) if !summary.is_empty() => {
            summary.to_owned()
        }
        Some(Outcome::Done) => "Brought what it learned home.".into(),
        Some(Outcome::Failed) => {
            "It failed; nothing was brought home.".into()
        }
        Some(Outcome::Stopped) => {
            "It was stopped; nothing was brought home.".into()
        }
        Some(Outcome::Nothing) | None => {
            "Nothing was brought home.".into()
        }
    }
}

impl Core<'_, '_> {
    /// Tells the agent's conversation, and its ender as `notify`
    /// says, how dream `id` ended.
    pub(crate) fn dreamt(
        &mut self,
        id: DreamId,
        summary: &str,
        notify: Notify,
    ) -> Result<()> {
        let db = self.realm.store().connection();
        let dream = dreams::get(db, id)?;
        let Some(agent) = dream.agent else {
            return Ok(());
        };
        let line = brought(&dream, summary);
        let ender = match agents::get(db, agent)?.ended_by {
            Some(Actor::Agent(ender))
                if dream.ending
                    && notify == Notify::Ender
                    && ender != agent
                    && agents::get(db, ender)?.is_living() =>
            {
                Some(ender)
            }
            _ => None,
        };
        self.change(Actor::Realm, |cx, env| {
            conversations::chats::event(
                cx,
                agent,
                ItemEvent::Dreamed {
                    summary: line.clone(),
                },
            )?;
            let (Some(ender), Some(from)) =
                (ender, conversations::chats::of(cx.db, agent)?)
            else {
                return Ok(());
            };
            let name = addresses::name_of(cx.db, agent)?;
            let address = addresses::address_of(cx.db, ender)?;
            let text =
                format!("The last dream of {name} ended. {line}");
            let task =
                tasks::mint(cx, Origin::Notice, agent, None, None)?;
            messages::send(
                cx,
                env,
                messages::Send {
                    kind: MessageKind::Notification,
                    runtime: "hamlet",
                    key: &format!("dream:{id}"),
                    sender: agent,
                    receiver: ender,
                    from,
                    address: &address,
                    task,
                    text: &text,
                    images: &[],
                },
            )?;
            Ok(())
        })
    }

    pub(crate) fn dream_tick(&mut self) -> Result<()> {
        self.start_dream(None)
    }

    /// [`Self::dream_tick`] for the call that just ended `agent` with
    /// a dream, which answers how it ended.
    pub(crate) fn dream_tick_ending(
        &mut self,
        agent: AgentId,
    ) -> Result<()> {
        self.start_dream(Some(agent))
    }

    /// How the last dream of `agent`, just ended, fares, for its
    /// ender.
    pub(crate) fn last_dream_words(
        &mut self,
        agent: AgentId,
        notify: Notify,
    ) -> Result<String> {
        let db = self.realm.store().connection();
        let told = match notify {
            Notify::Ender => {
                " You will be notified of what it brings home; until \
                 then its profile has nothing new from it."
            }
            Notify::Answered => "",
        };
        if reflections::is_reflecting(db, agent)? {
            return Ok(format!(
                "It reflects on its conversation, then dreams once \
                 more to bring what it learned home.{told}"
            ));
        }
        let ended = agents::get(db, agent)?.ended_at.unwrap_or(0);
        let last = dreams::last_of_agent(db, agent)?
            .filter(|d| d.ending && d.began >= ended);
        Ok(match last {
            Some(dream) if dream.ended.is_some() => {
                format!(
                    "Its last dream ended. {}",
                    brought(&dream, "")
                )
            }
            Some(_) => format!(
                "It dreams once more to bring what it learned \
                 home.{told}"
            ),
            None => format!(
                "It dreams once more, in its turn, to bring what it \
                 learned home.{told}"
            ),
        })
    }

    /// Starts the next dream, when none is running.
    fn start_dream(
        &mut self,
        answered: Option<AgentId>,
    ) -> Result<()> {
        let layout = self.realm.layout().clone();
        let settings = self.env.settings.dreams.clone();
        let mut tried = Vec::new();
        loop {
            let db = self.realm.store().connection();
            let now = crate::app::clock::now();
            let Some(dreamer) = next(db, &layout, &settings, now)?
            else {
                return Ok(());
            };
            // One with nothing to dream about ends at once, and the
            // next takes its turn.
            if tried.contains(&dreamer) {
                return Ok(());
            }
            tried.push(dreamer);
            let notify = match dreamer {
                Dreamer::Agent { agent, .. }
                    if Some(agent) == answered =>
                {
                    Notify::Answered
                }
                _ => Notify::Ender,
            };
            if self.dream(dreamer, notify)?.is_some() {
                return Ok(());
            }
        }
    }

    /// `/dream <profile#id|address>`: dreams now, or in its turn.
    pub(crate) fn dream_command(
        &mut self,
        here: Option<&hamlet_store::ids::HamletId>,
        words: &[&str],
    ) -> Done<String> {
        let [target] = words else {
            reject!(Rejection::DreamUsage);
        };
        let dreamer = done!(self.dreamer(here, target));
        if let Dreamer::Agent { agent, .. } = dreamer {
            done!(self.directs(agent));
        }
        Ok(Ok(self.ask_dreams(dreamer)?))
    }

    /// The living profile, or the agent living or ended, `target`
    /// names in `here`.
    pub(crate) fn dreamer(
        &mut self,
        here: Option<&hamlet_store::ids::HamletId>,
        target: &str,
    ) -> Done<Dreamer> {
        let db = self.realm.store().connection();
        if target.starts_with("profile#") {
            let id = checked!(addresses::parse_profile(target));
            return Ok(city::living(db, id)?
                .map(|found| Dreamer::Profile(found.id))
                .ok_or_else(|| Rejection::NoProfile {
                    address: target.into(),
                }));
        }
        let address = checked!(addresses::parse(target));
        let agent = done!(addresses::resolve(
            db,
            &address,
            here,
            addresses::Reach::Kept,
        ));
        let ending = !agents::get(db, agent)?.is_living();
        Ok(Ok(Dreamer::Agent { agent, ending }))
    }

    /// Asks for the dream of `dreamer` whatever the settings say,
    /// after those of a profile's agents that learned what it lacks.
    /// Returns how each fares.
    pub(crate) fn ask_dreams(
        &mut self,
        dreamer: Dreamer,
    ) -> Result<String> {
        let layout = self.realm.layout().clone();
        let db = self.realm.store().connection();
        let mut dreamers = Vec::new();
        if let Dreamer::Profile(profile) = dreamer {
            for found in hamlet_agents::living_of(db, profile)? {
                if knowledge::dreams::has_learned(
                    db,
                    &layout,
                    found.agent,
                )? {
                    dreamers.push(Dreamer::Agent {
                        agent: found.agent,
                        ending: false,
                    });
                }
            }
        }
        dreamers.push(dreamer);
        let now = crate::app::clock::now();
        for dreamer in &dreamers {
            dreams::ask(db, dreamer.asked(), now)?;
        }
        self.dream_tick()?;
        let db = self.realm.store().connection();
        let running = dreams::running(db)?;
        let waiting = dreams::asked(db)?;
        // One dream asked for is told without its dreamer's name.
        let one = dreamers.len() == 1;
        let mut words = Vec::new();
        for dreamer in dreamers {
            let label = match dreamer {
                Dreamer::Agent { agent, .. } => {
                    addresses::name_of(db, agent)?
                }
                Dreamer::Profile(profile) => {
                    city::get(db, profile)?.label()
                }
            };
            let of = if one {
                String::new()
            } else {
                format!(" of {label}")
            };
            words.push(
                if running.as_ref().is_some_and(|r| dreamer.is_of(r))
                {
                    format!("The dream{of} began.")
                } else if waiting.contains(&dreamer.asked()) {
                    format!("The dream{of} waits its turn.")
                } else if one {
                    "There is nothing to dream about.".to_owned()
                } else {
                    format!("{label} has nothing to dream about.")
                },
            );
        }
        Ok(words.join(" "))
    }

    /// Starts `dreamer`'s dream; `None` when it has nothing to dream
    /// about.
    fn dream(
        &mut self,
        dreamer: Dreamer,
        notify: Notify,
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
            self.dreamt(id, "", notify)?;
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

#[cfg(test)]
mod tests {
    use hamlet_store::ids::Actor;

    use super::*;
    use crate::society::agents::EndHow;
    use crate::society::hamlet_agents::LastDream;
    use crate::testing::Realm;

    #[test]
    fn only_an_agent_ended_with_a_dream_dreams_once_more() {
        let mut realm = Realm::new();
        let lead = realm.found("mill", "ada");
        let plain = realm.add_agent(&lead, "bo");
        let dreaming = realm.add_agent(&lead, "cy");
        for (ended, last) in [
            (&plain, LastDream::Without),
            (&dreaming, LastDream::With),
        ] {
            realm
                .chief(|cx| {
                    hamlet_agents::end(
                        cx,
                        ended.agent,
                        EndHow::Ended,
                        last,
                        "",
                    )
                })
                .unwrap();
        }
        let (layout, off) = (realm.layout(), Dreams::default());
        let now = realm.at;
        assert_eq!(
            next(realm.db(), &layout, &off, now).unwrap(),
            Some(Dreamer::Agent {
                agent: dreaming.agent,
                ending: true
            })
        );
        let dream = realm
            .by(Actor::Realm, |cx| {
                Ok(Ok(dreams::begin(
                    cx,
                    DreamKind::Agent,
                    dreaming.profile,
                    Some(dreaming.agent),
                    true,
                )?))
            })
            .unwrap();
        let now = realm.at;
        dreams::end(
            realm.db(),
            dream,
            Outcome::Nothing,
            None,
            None,
            now,
        )
        .unwrap();
        assert_eq!(
            next(realm.db(), &layout, &off, now).unwrap(),
            None
        );
    }
}
