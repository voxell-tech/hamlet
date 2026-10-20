//! What waits for the chief, told as it changes, the answers to it,
//! and the answers picked for an agent's questions, kept here so
//! every panel that shows them shows the same.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use bevy::app::{App, PostUpdate};
use bevy::ecs::entity::Entity;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{
    Local, Query, Res, ResMut, SystemParam, SystemState,
};
use bevy::ecs::world::World;
use hamlet_core::oversight::{
    Answer, Answered, Asks, Kind, Need as Kept, Waiting, danger,
};
use hamlet_core::session::{Entry, SessionKey, Transcript};
use hamlet_engine_api::Failure;
use hamlet_engine_api::agents::{
    AgentsEvent, Choice, Danger, Need, NeedKind, Offer, Pick,
    Question, Questions, Sheet,
};
use hamlet_rules::chief::asking::{self, Pick as Picked};
use hamlet_rules::chief::rulings::Choice as Chosen;

use crate::watch::{Gather, Watchers};
use crate::{Chief, views};

/// The time between two tellings of the time each has waited.
const TICK: Duration = Duration::from_secs(10);

/// The characters of what it is about that its line shows.
const LINE: usize = 80;

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Sheets>().add_systems(
        PostUpdate,
        (forget_sheets, tell_needs).chain().in_set(Gather),
    );
}

/// The answers picked so far for each card of questions.
#[derive(Resource, Default)]
pub(crate) struct Sheets(HashMap<Entity, Sheet>);

impl Sheets {
    /// The sheet of `card`, which asks `count` questions.
    fn sheet(&mut self, card: Entity, count: usize) -> &mut Sheet {
        self.0.entry(card).or_insert_with(|| Sheet {
            at: 0,
            picks: vec![None; count],
        })
    }
}

/// The needs' methods on [`World`].
pub(crate) trait NeedsWorldExt {
    fn needs(&mut self) -> Vec<Need>;

    /// The thing that waits whose id is `need`, with its questions.
    fn waiting(&self, need: u64) -> Result<(Entity, usize), Failure>;

    fn answer(
        &mut self,
        need: u64,
        choice: Choice,
        text: String,
    ) -> Result<(), Failure>;

    fn pick(
        &mut self,
        need: u64,
        question: u32,
        pick: Option<Pick>,
    ) -> Result<(), Failure>;

    fn turn(&mut self, need: u64, to: u32) -> Result<(), Failure>;

    fn submit(&mut self, need: u64) -> Result<(), Failure>;
}

impl NeedsWorldExt for World {
    fn needs(&mut self) -> Vec<Need> {
        let mut facts = SystemState::<NeedsFacts>::new(self);
        facts
            .get(self)
            .map(|facts| facts.needs())
            .unwrap_or_default()
    }

    fn waiting(&self, need: u64) -> Result<(Entity, usize), Failure> {
        let entity = Entity::try_from_bits(need)
            .filter(|entity| self.get::<Waiting>(*entity).is_some())
            .ok_or(Failure::NotFound)?;
        let count = self
            .get::<Asks>(entity)
            .map_or(0, |asks| asks.0.questions.len());
        Ok((entity, count))
    }

    fn answer(
        &mut self,
        need: u64,
        choice: Choice,
        text: String,
    ) -> Result<(), Failure> {
        let (entity, _) = self.waiting(need)?;
        let chief = self.resource::<Chief>().name.clone();
        self.trigger(Answer {
            entity,
            choice: chosen(choice),
            text: text.trim().to_string(),
            chief,
        });
        Ok(())
    }

    fn pick(
        &mut self,
        need: u64,
        question: u32,
        pick: Option<Pick>,
    ) -> Result<(), Failure> {
        let (card, count) = self.waiting(need)?;
        let at = question as usize;
        if at >= count {
            return Err(Failure::NotFound);
        }
        // Empty words leave an option picked as it was.
        let mut sheets = self.resource_mut::<Sheets>();
        let sheet = sheets.sheet(card, count);
        let pick = match pick {
            Some(Pick::Words(words)) if words.trim().is_empty() => {
                match &sheet.picks[at] {
                    Some(Pick::Offer(offer)) => {
                        Some(Pick::Offer(*offer))
                    }
                    _ => None,
                }
            }
            Some(Pick::Words(words)) => {
                Some(Pick::Words(words.trim().to_string()))
            }
            pick => pick,
        };
        let onward =
            matches!(pick, Some(Pick::Offer(_))) && at + 1 < count;
        sheet.picks[at] = pick;
        if onward {
            sheet.at = question + 1;
        }
        Ok(())
    }

    fn turn(&mut self, need: u64, to: u32) -> Result<(), Failure> {
        let (card, count) = self.waiting(need)?;
        if count == 0 {
            return Err(Failure::NotFound);
        }
        let last = u32::try_from(count - 1).unwrap_or(u32::MAX);
        self.resource_mut::<Sheets>().sheet(card, count).at =
            to.min(last);
        Ok(())
    }

    fn submit(&mut self, need: u64) -> Result<(), Failure> {
        let (card, count) = self.waiting(need)?;
        let picks = self
            .resource_mut::<Sheets>()
            .sheet(card, count)
            .picks
            .clone();
        if let Some(open) = picks.iter().position(Option::is_none) {
            let open = u32::try_from(open).unwrap_or(u32::MAX);
            return self.turn(need, open);
        }
        let chief = self.resource::<Chief>().name.clone();
        self.trigger(Answered {
            entity: card,
            picks: picks
                .into_iter()
                .flatten()
                .map(|pick| match pick {
                    Pick::Offer(at) => Picked::Offer(at as usize),
                    Pick::Words(words) => Picked::Words(words),
                })
                .collect(),
            chief,
        });
        Ok(())
    }
}

/// What the queue is made of.
#[derive(SystemParam)]
pub(crate) struct NeedsFacts<'w, 's> {
    waiting: Query<
        'w,
        's,
        (Entity, &'static Waiting, Option<&'static Asks>),
    >,
    sessions:
        Query<'w, 's, (&'static SessionKey, &'static Transcript)>,
    sheets: Res<'w, Sheets>,
}

impl NeedsFacts<'_, '_> {
    /// Everything that waits, oldest first.
    fn needs(&self) -> Vec<Need> {
        let mut rows = self.waiting.iter().collect::<Vec<_>>();
        rows.sort_by_key(|(_, waits, _)| waits.order);
        rows.into_iter()
            .map(|(entity, waits, asks)| {
                let kept = Kept::of(entity, waits, asks);
                let session = waits.session.and_then(|session| {
                    self.sessions.get(session).ok()
                });
                Need {
                    id: entity.to_bits(),
                    kind: kind(kept.kind),
                    line: kept.line(LINE),
                    by: kept.by,
                    change: held(
                        waits,
                        session.map(|(_, said)| said),
                    ),
                    about: kept.about,
                    waited: kept.waited,
                    asked: kept.asked,
                    answers: kept
                        .answers
                        .into_iter()
                        .map(choice)
                        .collect(),
                    questions: kept
                        .questions
                        .map(|asked| questions(&asked)),
                    session: session
                        .map(|(key, _)| views::session_name(key)),
                    sheet: self
                        .sheets
                        .0
                        .get(&entity)
                        .cloned()
                        .unwrap_or_else(|| Sheet {
                            at: 0,
                            picks: vec![
                                None;
                                asks.map_or(0, |asks| asks
                                    .0
                                    .questions
                                    .len())
                            ],
                        }),
                }
            })
            .collect()
    }
}

/// The change of the edit that waits as `waits`: the tool call open
/// in the conversation whose turn it holds.
fn held(
    waits: &Waiting,
    said: Option<&Transcript>,
) -> Option<hamlet_engine_api::agents::Change> {
    if waits.kind != Kind::Approval(danger::Danger::Edit) {
        return None;
    }
    let open = said?.items().iter().rev().find(|item| {
        matches!(item.entry, Entry::Tool { exit: None, .. })
    })?;
    let Entry::Tool {
        change: Some(change),
        ..
    } = &open.entry
    else {
        return None;
    };
    let path = waits.about.lines().next()?;
    path.starts_with(&change.path)
        .then(|| views::change(change))
}

fn kind(kind: Kind) -> NeedKind {
    match kind {
        Kind::Approval(risk) => NeedKind::Approval(match risk {
            danger::Danger::Network => Danger::Network,
            danger::Danger::Unconfined => Danger::Unconfined,
            danger::Danger::Destructive => Danger::Destructive,
            danger::Danger::Reserved => Danger::Reserved,
            danger::Danger::Edit => Danger::Edit,
            danger::Danger::Command => Danger::Command,
        }),
        Kind::Objection(_) => NeedKind::Objection,
        Kind::Question => NeedKind::Question,
        Kind::Fault => NeedKind::Fault,
        Kind::Lapsed => NeedKind::Lapsed,
    }
}

fn choice(choice: Chosen) -> Choice {
    match choice {
        Chosen::Allow => Choice::Allow,
        Chosen::Always => Choice::Always,
        Chosen::Deny => Choice::Deny,
        Chosen::GoAhead => Choice::GoAhead,
        Chosen::Withdraw => Choice::Withdraw,
        Chosen::Reply => Choice::Reply,
        Chosen::Dismiss => Choice::Dismiss,
    }
}

fn chosen(choice: Choice) -> Chosen {
    match choice {
        Choice::Allow => Chosen::Allow,
        Choice::Always => Chosen::Always,
        Choice::Deny => Chosen::Deny,
        Choice::GoAhead => Chosen::GoAhead,
        Choice::Withdraw => Chosen::Withdraw,
        Choice::Reply => Chosen::Reply,
        Choice::Dismiss => Chosen::Dismiss,
    }
}

fn questions(asked: &asking::Questions) -> Questions {
    Questions {
        message: asked.message.clone(),
        questions: asked
            .questions
            .iter()
            .map(|one| Question {
                question: one.question.clone(),
                description: one.description.clone(),
                options: one
                    .options
                    .iter()
                    .map(|offer| Offer {
                        title: offer.title.clone(),
                        description: offer.description.clone(),
                    })
                    .collect(),
                text_input: one.text_input,
            })
            .collect(),
    }
}

/// Drops what was picked for a card that no longer waits.
fn forget_sheets(
    waiting: Query<(), With<Waiting>>,
    mut sheets: ResMut<Sheets>,
) {
    if sheets.0.keys().any(|card| !waiting.contains(*card)) {
        sheets.0.retain(|card, _| waiting.contains(*card));
    }
}

/// What the watchers were last told, and when the time each has
/// waited is next told.
#[derive(Default)]
struct Shown {
    needs: Option<Vec<Need>>,
    due: Option<Instant>,
}

fn tell_needs(
    facts: NeedsFacts,
    mut watchers: ResMut<Watchers>,
    mut shown: Local<Shown>,
) {
    if !watchers.watched() {
        *shown = Shown::default();
        return;
    }
    let now = Instant::now();
    let ticked = shown.due.is_some_and(|due| now >= due);
    let needs = facts.needs();
    let moved = shown.needs.as_ref().is_none_or(|was| {
        // The time waited is told only every tick.
        was.len() != needs.len()
            || was.iter().zip(&needs).any(|(was, now)| {
                Need {
                    waited: now.waited.clone(),
                    ..was.clone()
                } != *now
            })
    });
    if !moved && !ticked {
        return;
    }
    shown.due = Some(now + TICK);
    if shown.needs.as_ref() != Some(&needs) {
        watchers.tell(AgentsEvent::Needs(needs.clone()));
        shown.needs = Some(needs);
    }
}
