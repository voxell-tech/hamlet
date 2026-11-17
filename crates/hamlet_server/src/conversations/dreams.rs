//! When agents dream, one at a time. A dream asked for starts in its
//! turn whatever the settings say, and so does the last dream of a
//! hire ended with one. While dreams are on and in the chief's hours,
//! a hire dreams once it learned something its scholar lacks, and a
//! scholar at most once a day to tidy its `main`.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use bevy_app::Startup;
use bevy_ecs::message::{Message, Messages};
use bevy_ecs::resource::Resource;
use bevy_ecs::system::ResMut;
use bevy_log::error;
use hamlet_store::Db;
use hamlet_store::ids::{Actor, AgentId, DreamId, ScholarId};

use crate::app::input::AddHandler;
use crate::app::timers::AddTimer;
use crate::conversations::inputs::{self, InputKind, Keep};
use crate::conversations::marks::Mark;
use crate::conversations::tasks::{self, Origin};
use crate::conversations::{self, Begin, Kind, models};
use crate::core::Core;
use crate::realm::Realm;
use crate::realm::change::Done;
use crate::realm::jobs::Job;
use crate::realm::layout::Layout;
use crate::realm::settings::Dreams;
use crate::society::dreams::{
    self, Asked, Dream, DreamKind, Outcome,
};
use crate::society::refusal::Refusal;
use crate::society::{addresses, agents, city, hand, hires};
use crate::{knowledge, runtimes};

/// The least time between two dreams of one hire.
const HIRE_REST: i64 = 6 * 3_600_000;
/// The least time between two dreams of one scholar.
const SCHOLAR_REST: i64 = 24 * 3_600_000;
/// The time after its end a hire may still dream once more.
const ENDING_WINDOW: i64 = 24 * 3_600_000;
/// The time between the server's looks for a due dream.
const DREAM_CHECK: std::time::Duration =
    std::time::Duration::from_secs(60);

/// A check for a due dream every [`DREAM_CHECK`], and another once a
/// dream ended; what the hires learned, found at the start.
pub(crate) fn plugin(app: &mut bevy_app::App) {
    app.init_resource::<Learned>()
        .add_systems(Startup, note_learned)
        .add_handler::<DreamEnded, _>(start_next_dream)
        .add_every(DREAM_CHECK, dream_tick);
}

fn dream_tick(mut core: Core) -> Result<()> {
    core.dream_tick()
}

/// A dream ended, so the next one waiting may begin.
#[derive(Message)]
pub(crate) struct DreamEnded;

fn start_next_dream(
    mut ended: ResMut<Messages<DreamEnded>>,
    mut core: Core,
) -> bevy_ecs::error::Result {
    ended.clear();
    core.dream_tick()?;
    Ok(())
}

/// The living hires whose branch holds what they learned that no
/// dream took home yet, as the clients are shown.
#[derive(Resource, Default)]
pub(crate) struct Learned(pub BTreeSet<AgentId>);

impl Learned {
    /// Finds them again.
    pub fn note(&mut self, db: &mut Db, layout: &Layout) {
        match find_learned(db, layout) {
            Ok(learned) => self.0 = learned,
            Err(error) => {
                error!("finding what hires learned: {error:#}");
            }
        }
    }

    /// Takes in whether each hire of `learning` has learned something
    /// now.
    pub fn take_in(&mut self, learning: &BTreeMap<AgentId, bool>) {
        for (&hire, &has) in learning {
            if has {
                self.0.insert(hire);
            } else {
                self.0.remove(&hire);
            }
        }
    }
}

/// Whether each hire the jobs `done` may have changed has learned
/// something now.
pub(crate) fn learning(
    db: &mut Db,
    layout: &Layout,
    done: &[Job],
) -> BTreeMap<AgentId, bool> {
    let mut learning = BTreeMap::new();
    for job in done {
        let found = learners(db, job).and_then(|hires| {
            hires
                .into_iter()
                .map(|hire| {
                    Ok((hire, has_learned(db, layout, hire)?))
                })
                .collect::<Result<Vec<_>>>()
        });
        match found {
            Ok(found) => learning.extend(found),
            Err(error) => {
                error!("finding what hires learned: {error:#}");
            }
        }
    }
    learning
}

/// The hires whose branch, or whose scholar's `main`, `job` changes.
fn learners(db: &mut Db, job: &Job) -> Result<Vec<AgentId>> {
    let of_scholar = |db: &mut Db, scholar, also: Option<AgentId>| {
        let mut hires = hires::living_of(db, scholar)?
            .into_iter()
            .map(|hire| hire.agent)
            .collect::<Vec<_>>();
        hires.extend(also);
        anyhow::Ok(hires)
    };
    Ok(match job {
        Job::Lodge { hire }
        | Job::Commit { hire }
        | Job::TakeIn { hire }
        | Job::EraseHire { hire } => vec![*hire],
        Job::BringHome { hire } => {
            let scholar = hires::get(db, *hire)?.scholar;
            of_scholar(db, scholar, Some(*hire))?
        }
        Job::DreamEnd { dream, .. } => {
            let dream = dreams::get(db, *dream)?;
            of_scholar(db, dream.scholar, dream.hire)?
        }
        Job::Plant { scholar, .. }
        | Job::EraseScholar { scholar } => {
            of_scholar(db, *scholar, None)?
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

/// Whether `agent` is a living hire whose branch holds what it
/// learned that no dream took home yet.
fn has_learned(
    db: &mut Db,
    layout: &Layout,
    agent: AgentId,
) -> Result<bool> {
    Ok(hires::find(db, agent)?.is_some()
        && agents::get(db, agent)?.is_living()
        && knowledge::dreams::has_learned(db, layout, agent)?)
}

fn note_learned(
    mut realm: ResMut<Realm>,
    mut learned: ResMut<Learned>,
) {
    let layout = realm.layout().clone();
    learned.note(realm.store().connection(), &layout);
}

fn find_learned(
    db: &mut Db,
    layout: &Layout,
) -> Result<BTreeSet<AgentId>> {
    let mut learned = BTreeSet::new();
    for agent in agents::all_hires(db)? {
        if has_learned(db, layout, agent)? {
            learned.insert(agent);
        }
    }
    Ok(learned)
}

/// A hire or a scholar that dreams.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Dreamer {
    Hire { hire: AgentId, ending: bool },
    Scholar(ScholarId),
}

impl Dreamer {
    fn asked(self) -> Asked {
        match self {
            Self::Hire { hire, .. } => Asked::Hire(hire),
            Self::Scholar(scholar) => Asked::Scholar(scholar),
        }
    }

    /// Whether `dream` is this one's.
    fn is_of(self, dream: &Dream) -> bool {
        match self {
            Self::Hire { hire, .. } => dream.hire == Some(hire),
            Self::Scholar(scholar) => {
                dream.kind == DreamKind::Scholar
                    && dream.scholar == scholar
            }
        }
    }
}

/// The dream to start next, when none is running: one asked for, then
/// the last dream of a hire ended with one, then, while dreams are on
/// and in their hours, the dreamer most due. What was asked of a
/// dreamer that is gone is let go.
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
            Asked::Hire(hire) => {
                let found = agents::get(db, hire)?;
                (!found.erased).then_some(Dreamer::Hire {
                    hire,
                    ending: !found.is_living(),
                })
            }
            Asked::Scholar(scholar) => city::living(db, scholar)?
                .map(|_| Dreamer::Scholar(scholar)),
        };
        match dreamer {
            Some(dreamer) => return Ok(Some(dreamer)),
            None => dreams::forget(db, asked)?,
        }
    }
    for (hire, ended) in
        hires::ended_to_dream(db, now - ENDING_WINDOW)?
    {
        let dreamt = dreams::last_of_hire(db, hire)?
            .is_some_and(|d| d.ending && d.began >= ended);
        if !dreamt {
            return Ok(Some(Dreamer::Hire { hire, ending: true }));
        }
    }
    if !settings.on || !in_hours(settings.hours, now)? {
        return Ok(None);
    }
    due(db, layout, now)
}

/// Whether `now` falls in the local `hours`, from and to.
fn in_hours(hours: (u8, u8), now: i64) -> Result<bool> {
    let hour = crate::app::clock::local(now)?.hour().unsigned_abs();
    let (from, to) = hours;
    Ok(if from <= to {
        (from..to).contains(&hour)
    } else {
        hour >= from || hour < to
    })
}

/// The living hire or the scholar most due a dream, if one is.
fn due(
    db: &mut Db,
    layout: &Layout,
    now: i64,
) -> Result<Option<Dreamer>> {
    for agent in agents::all_hires(db)? {
        if !agents::get(db, agent)?.is_living()
            || inputs::is_agent_busy(db, agent)?
        {
            continue;
        }
        let rested = dreams::last_of_hire(db, agent)?
            .is_none_or(|d| now - d.began >= HIRE_REST);
        if rested
            && knowledge::dreams::has_learned(db, layout, agent)?
        {
            return Ok(Some(Dreamer::Hire {
                hire: agent,
                ending: false,
            }));
        }
    }
    for scholar in city::all(db)? {
        if !scholar.is_living() || !scholar.planted {
            continue;
        }
        let rested = dreams::last_of_scholar(db, scholar.id)?
            .is_none_or(|d| now - d.began >= SCHOLAR_REST);
        let has_hires = !hires::living_of(db, scholar.id)?.is_empty();
        if rested && has_hires {
            return Ok(Some(Dreamer::Scholar(scholar.id)));
        }
    }
    Ok(None)
}

impl Core<'_, '_> {
    /// Starts the next dream, when none is running.
    pub(crate) fn dream_tick(&mut self) -> Result<()> {
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
            if self.dream(dreamer)?.is_some() {
                return Ok(());
            }
        }
    }

    /// `/dream <scholar#id|address>`: dreams now, or in its turn.
    pub(crate) fn dream_command(
        &mut self,
        here: Option<&hamlet_store::ids::HamletId>,
        words: &[&str],
    ) -> Done<String> {
        let [target] = words else {
            return Ok(Err(Refusal::DreamUsage));
        };
        let dreamer = match self.dreamer(here, target)? {
            Ok(dreamer) => dreamer,
            Err(refusal) => return Ok(Err(refusal)),
        };
        if let Dreamer::Hire { hire, .. } = dreamer
            && let Err(refusal) = self.leads(hire)?
        {
            return Ok(Err(refusal));
        }
        Ok(Ok(self.ask_dreams(dreamer)?))
    }

    /// What `target` names to dream, said in `here`: a living
    /// scholar, or an agent living or ended.
    pub(crate) fn dreamer(
        &mut self,
        here: Option<&hamlet_store::ids::HamletId>,
        target: &str,
    ) -> Done<Dreamer> {
        let db = self.realm.store().connection();
        if target.starts_with("scholar#") {
            let found = match addresses::parse_scholar(target) {
                Ok(id) => city::living(db, id)?,
                Err(refusal) => return Ok(Err(refusal)),
            };
            return Ok(match found {
                Some(found) => Ok(Dreamer::Scholar(found.id)),
                None => Err(Refusal::NoScholar {
                    address: target.into(),
                }),
            });
        }
        let address = match addresses::parse(target) {
            Ok(address) => address,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let agent = match addresses::resolve(
            db,
            &address,
            here,
            addresses::Reach::Kept,
        )? {
            Ok(agent) => agent,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let ending = !agents::get(db, agent)?.is_living();
        Ok(Ok(Dreamer::Hire {
            hire: agent,
            ending,
        }))
    }

    /// Asks for the dream of `dreamer`, whatever the settings say,
    /// and says how each asked fares. A scholar's own comes after
    /// those of its living hires that learned what it lacks. Each
    /// starts now when no dream runs, else in its turn.
    pub(crate) fn ask_dreams(
        &mut self,
        dreamer: Dreamer,
    ) -> Result<String> {
        let layout = self.realm.layout().clone();
        let db = self.realm.store().connection();
        let mut dreamers = Vec::new();
        if let Dreamer::Scholar(scholar) = dreamer {
            for hire in hires::living_of(db, scholar)? {
                if knowledge::dreams::has_learned(
                    db, &layout, hire.agent,
                )? {
                    dreamers.push(Dreamer::Hire {
                        hire: hire.agent,
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
                Dreamer::Hire { hire, .. } => {
                    addresses::name_of(db, hire)?
                }
                Dreamer::Scholar(scholar) => {
                    city::get(db, scholar)?.label()
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

    /// Starts `dreamer`'s dream. `None` when there is nothing to
    /// dream about.
    pub(crate) fn dream(
        &mut self,
        dreamer: Dreamer,
    ) -> Result<Option<DreamId>> {
        let layout = self.realm.layout().clone();
        let db = self.realm.store().connection();
        let (kind, scholar, hire, ending, agent) = match dreamer {
            Dreamer::Hire { hire, ending } => {
                let found = hires::get(db, hire)?;
                (
                    DreamKind::Hire,
                    found.scholar,
                    Some(hire),
                    ending,
                    hire,
                )
            }
            Dreamer::Scholar(scholar) => (
                DreamKind::Scholar,
                scholar,
                None,
                false,
                hand::id(db)?,
            ),
        };
        let id = self.change(Actor::Realm, |cx, _| {
            dreams::begin(cx, kind, scholar, hire, ending)
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
        let label = city::get(db, scholar)?.label();
        let instructions = match hire {
            Some(hire) => {
                let lodge =
                    knowledge::lodging(db, &layout, hire)?.path;
                let who = addresses::name_of(db, hire)?;
                format!(
                    "You are dreaming, between your work as \
                     `{who}`, a hire of the scholar `{label}`. Your \
                     lodge at {} holds what you learned as this \
                     hire: your memory/ and skills/. `{label}`'s \
                     shared knowledge, its main, is checked out at \
                     {}; every hire of `{label}` starts from it and \
                     takes it in.\n\nBring home what any hire of \
                     `{label}` would want to know: move or merge it \
                     into the checkout's memory/ and skills/, \
                     written for a reader who was not there. Keep \
                     in your lodge what only this hire needs. Tidy \
                     as you go: one fact in one place, nothing \
                     stale, the root memory files short, and a true \
                     `description:` on every memory file. Do not \
                     change CHARACTER.md. When done, answer in one \
                     line what you brought home, or `Nothing to \
                     bring home.`",
                    lodge.display(),
                    checkout.display()
                )
            }
            None => format!(
                "You are tidying the knowledge of the scholar \
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
    use crate::society::hires::LastDream;
    use crate::testing::Realm;

    #[test]
    fn an_asked_dream_runs_with_dreams_off_and_outside_their_hours() {
        let mut realm = Realm::new();
        let mayor = realm.found("mill", "ada");
        let (layout, now) = (realm.layout(), realm.at);
        let hour = crate::app::clock::local(now)
            .unwrap()
            .hour()
            .unsigned_abs();
        let elsewhere = Dreams {
            on: true,
            hours: ((hour + 1) % 24, (hour + 2) % 24),
            ..Dreams::default()
        };
        let settings = [Dreams::default(), elsewhere];
        for dreams in &settings {
            assert_eq!(
                next(realm.db(), &layout, dreams, now).unwrap(),
                None
            );
        }
        dreams::ask(realm.db(), Asked::Hire(mayor.agent), now)
            .unwrap();
        for dreams in &settings {
            assert_eq!(
                next(realm.db(), &layout, dreams, now).unwrap(),
                Some(Dreamer::Hire {
                    hire: mayor.agent,
                    ending: false
                })
            );
        }
    }

    #[test]
    fn only_a_hire_ended_with_a_dream_dreams_once_more() {
        let mut realm = Realm::new();
        let mayor = realm.found("mill", "ada");
        let plain = realm.hire(&mayor, "bo");
        let dreaming = realm.hire(&mayor, "cy");
        for (ended, last) in [
            (&plain, LastDream::Without),
            (&dreaming, LastDream::With),
        ] {
            realm
                .chief(|cx| {
                    hires::end(
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
            Some(Dreamer::Hire {
                hire: dreaming.agent,
                ending: true
            })
        );
        let dream = realm
            .by(Actor::Realm, |cx| {
                Ok(Ok(dreams::begin(
                    cx,
                    DreamKind::Hire,
                    dreaming.scholar,
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
