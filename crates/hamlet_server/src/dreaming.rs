//! When agents dream: one dream at a time, while dreams are on and in
//! the chief's hours, or when a chief asks with `/dream`. A hire
//! dreams once it learned something its scholar lacks, an ended hire
//! once more, and a scholar at most once a day to tidy its `main`.

use anyhow::Result;
use chrono::Timelike;
use hamlet_git::{MAIN, Repository};
use hamlet_store::ids::{Actor, AgentId, DreamId, ScholarId};

use crate::core::Core;
use crate::knowledge::{self, dreaming};
use crate::society::dreams::{self, DreamKind, Outcome};
use crate::society::refusal::Refusal;
use crate::society::{addresses, agents, city, hand, hires};
use crate::talk::conversations::{self, Begin, Kind};
use crate::talk::inputs::{self, InputKind, Keep};
use crate::talk::marks::Mark;
use crate::talk::tasks::{self, Origin};
use crate::talk::{models, runtimes};
use crate::work::catalogue::SOCIETY;

/// The least time between two dreams of one hire.
const HIRE_REST: i64 = 6 * 3_600_000;
/// The least time between two dreams of one scholar.
const SCHOLAR_REST: i64 = 24 * 3_600_000;
/// How long after its end a hire may still dream once more.
const ENDING_WINDOW: i64 = 24 * 3_600_000;

/// What dreams.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Dreamer {
    Hire { hire: AgentId, ending: bool },
    Scholar(ScholarId),
}

impl Core {
    /// Starts a dream when one is due.
    pub(crate) async fn dream_tick(&mut self) -> Result<()> {
        let dreams = &self.env.settings.dreams;
        if !dreams.on {
            return Ok(());
        }
        let hour = crate::clock::local(crate::clock::now())?.hour();
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
            self.dream(dreamer).await?;
        }
        Ok(())
    }

    /// The dreamer most due, if one is.
    fn due(&mut self) -> Result<Option<Dreamer>> {
        let layout = self.realm.layout().clone();
        let now = crate::clock::now();
        let db = self.realm.store().connection();
        for agent in agents::all_hires(db)? {
            let found = agents::get(db, agent)?;
            if found.erased || inputs::is_agent_busy(db, agent)? {
                continue;
            }
            let ending = match found.ended_at {
                None => false,
                Some(at) if now - at < ENDING_WINDOW => true,
                Some(_) => continue,
            };
            let last = dreams::last_of_hire(db, agent)?;
            if ending && last.as_ref().is_some_and(|d| d.ending) {
                continue;
            }
            if !ending
                && last
                    .as_ref()
                    .is_some_and(|d| now - d.began < HIRE_REST)
            {
                continue;
            }
            let hire = hires::get(db, agent)?;
            let Ok(repo) =
                Repository::open(&layout.repository(hire.scholar))
            else {
                continue;
            };
            let (Some(tip), Some(main)) = (
                repo.tip(&knowledge::branch(agent))?,
                repo.tip(MAIN)?,
            ) else {
                continue;
            };
            let dreamt = last.and_then(|d| d.branch_commit)
                == Some(tip.to_string());
            if !dreamt && repo.merge_base(tip, main)? != Some(tip) {
                return Ok(Some(Dreamer::Hire {
                    hire: agent,
                    ending,
                }));
            }
        }
        for scholar in city::all(db)? {
            if !scholar.is_living() || !scholar.planted {
                continue;
            }
            let rested = dreams::last_of_scholar(db, scholar.id)?
                .is_none_or(|d| now - d.began >= SCHOLAR_REST);
            let has_hires =
                !hires::living_of(db, scholar.id)?.is_empty();
            if rested && has_hires {
                return Ok(Some(Dreamer::Scholar(scholar.id)));
            }
        }
        Ok(None)
    }

    /// `/dream <scholar#id|address>`: dreams now, outside the hours.
    pub(crate) async fn dream_command(
        &mut self,
        here: Option<&hamlet_store::ids::HamletId>,
        words: &[&str],
    ) -> Result<Result<String, Refusal>> {
        let [target] = words else {
            return Ok(Err(Refusal::Usage {
                words: "Use /dream <scholar#id|address>".into(),
            }));
        };
        let db = self.realm.store().connection();
        if dreams::running(db)?.is_some() {
            return Ok(Err(Refusal::Usage {
                words: "A dream is running; one dreams at a time."
                    .into(),
            }));
        }
        let dreamer = if target.starts_with("scholar#") {
            let found = match addresses::parse_scholar(target) {
                Ok(id) => city::living(db, id)?,
                Err(refusal) => return Ok(Err(refusal)),
            };
            match found {
                Some(found) => Dreamer::Scholar(found.id),
                None => {
                    return Ok(Err(Refusal::NoScholar {
                        address: (*target).into(),
                    }));
                }
            }
        } else {
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
            if let Err(refusal) = self.leads(agent)? {
                return Ok(Err(refusal));
            }
            let db = self.realm.store().connection();
            let ending = !agents::get(db, agent)?.is_living();
            Dreamer::Hire {
                hire: agent,
                ending,
            }
        };
        Ok(Ok(match self.dream(dreamer).await? {
            Some(_) => "The dream began.".into(),
            None => "There is nothing to dream about.".into(),
        }))
    }

    /// Starts `dreamer`'s dream. `None` when there is nothing to
    /// dream about.
    pub(crate) async fn dream(
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
        let id = self
            .change(Actor::Realm, |cx, _| {
                dreams::begin(cx, kind, scholar, hire, ending)
            })
            .await?;
        let writing = self.writing.clone();
        let held = writing
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let db = self.realm.store().connection();
        let ready =
            dreaming::prepare(db, &layout, id, crate::clock::now())?;
        drop(held);
        let db = self.realm.store().connection();
        if !ready {
            dreams::end(
                db,
                id,
                Outcome::Nothing,
                None,
                None,
                crate::clock::now(),
            )?;
            return Ok(None);
        }
        let checkout = dreaming::checkout(&layout, id);
        let name = city::get(db, scholar)?.label();
        let instructions = match hire {
            Some(hire) => {
                let lodge =
                    knowledge::lodging(db, &layout, hire)?.path;
                let who = addresses::name_of(db, hire)?;
                format!(
                    "You are dreaming, between your work as \
                     `{who}`, a hire of the scholar `{name}`. Your \
                     lodge at {} holds what you learned as this \
                     hire: your memory/ and skills/. `{name}`'s \
                     shared knowledge, its main, is checked out at \
                     {}; every hire of `{name}` starts from it and \
                     takes it in.\n\nBring home what any hire of \
                     `{name}` would want to know: move or merge it \
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
                 `{name}`, checked out at {}: its memory/ and \
                 skills/. Merge what is said twice, correct or \
                 remove what is stale, keep the root memory files \
                 short and every memory file's `description:` true, \
                 and make each memory folder's MEMORY.md describe \
                 it. Do not change CHARACTER.md. Answer in one line \
                 what you changed, or `Nothing to tidy.`",
                checkout.display()
            ),
        };
        let names = self.catalogue_names();
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
                &SOCIETY,
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
        })
        .await?;
        Ok(Some(id))
    }
}
