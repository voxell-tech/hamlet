//! The open conversation as the chat shows it: the sessions beside
//! it, their messages told as they come or change, a rewind, a
//! decree, and the papers and the prompt of its agent.

use std::collections::HashMap;

use bevy::app::{App, PostUpdate};
use bevy::ecs::change_detection::{DetectChanges, Ref};
use bevy::ecs::entity::Entity;
use bevy::ecs::observer::On;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Local, Query, Res, ResMut};
use bevy::ecs::world::World;
use hamlet_core::agents::{self, LivesIn, Mayor, Settlement, papers};
use hamlet_core::core::{self, HomeDir, Skills};
use hamlet_core::session::{
    self, Entry, Item, Reply, Rewind, Rewound, Seat, SessionKey,
    TalksTo, Transcript, UserMessage,
};
use hamlet_engine_api::Failure;
use hamlet_engine_api::agents::{
    AgentsEvent, Gift, Papers, Part, Related, SessionName, Skill,
};
use hamlet_home::Home;
use hamlet_rules::hamlet::{HOME, MAYOR};

use crate::watch::{Gather, Watchers};
use crate::{CHANNEL, Chief, views};

/// The most of the notes or the character that is told, in
/// characters.
const PAPERS_TOLD: usize = 6000;

pub(crate) fn plugin(app: &mut App) {
    app.add_observer(rewound).add_systems(
        PostUpdate,
        (tell_said, tell_skills).in_set(Gather),
    );
}

/// The open conversation's methods on [`World`].
pub(crate) trait ConversationWorldExt {
    /// The sessions shown in the chat of the open conversation's
    /// agent, the open one first.
    fn related(&mut self) -> Vec<Related>;

    /// The session named `name`.
    fn session_named(&mut self, name: &SessionName)
    -> Option<Entity>;

    /// Takes the session `name` back to before its message `entry`.
    fn rewind(
        &mut self,
        name: &SessionName,
        entry: u64,
    ) -> Result<(), Failure>;

    /// Gives a chief's command to `hamlet`, telling the open
    /// conversation the answers.
    fn decree(&mut self, hamlet: &str, text: String);

    /// The house of the open conversation's agent.
    fn open_house(&self) -> Home;

    fn papers(&self) -> Papers;

    /// The skills the open conversation's agent could be given.
    fn gifts(&self) -> Vec<Gift>;

    /// The prompt the open conversation's agent is given now.
    fn prompt(&self) -> Result<String, Failure>;

    /// Opens the character of the open conversation's agent in the
    /// system's editor, made empty if there is none.
    fn edit_character(&self) -> Result<(), Failure>;

    /// The library's skills.
    fn skills(&self) -> Vec<Skill>;
}

impl ConversationWorldExt for World {
    fn related(&mut self) -> Vec<Related> {
        let open = self.resource::<Chief>().session;
        let mut related = views::session(self, open)
            .map(|session| Related {
                session,
                part: Part::With,
                from: None,
                asked: false,
            })
            .into_iter()
            .collect::<Vec<_>>();
        let Some(agent) = self.get::<TalksTo>(open).map(|to| to.0)
        else {
            return related;
        };
        let address = match self.get::<Mayor>(agent) {
            Some(_) => MAYOR.to_string(),
            None => agents::name_of(self, agent),
        };
        let lives = |world: &World, agent: Entity| {
            world.get::<LivesIn>(agent).map(|lives| lives.0)
        };
        let home = lives(self, agent);
        let mut others = self
            .query::<(Entity, &SessionKey, Option<&TalksTo>)>()
            .iter(self)
            .filter(|(session, key, _)| {
                *session != open && (key.is_mail() || key.is_asked())
            })
            .filter_map(|(session, key, to)| {
                let to = to?.0;
                let part = if to == agent {
                    Part::With
                } else if key.chat == address
                    && lives(self, to) == home
                {
                    Part::Wrote
                } else {
                    return None;
                };
                Some((
                    session,
                    part,
                    key.chat.clone(),
                    key.is_asked(),
                ))
            })
            .collect::<Vec<_>>();
        others.sort_by_key(|(session, ..)| *session);
        related.extend(others.into_iter().filter_map(
            |(session, part, from, asked)| {
                Some(Related {
                    session: views::session(self, session)?,
                    part,
                    from: Some(from),
                    asked,
                })
            },
        ));
        related
    }

    fn session_named(
        &mut self,
        name: &SessionName,
    ) -> Option<Entity> {
        let key = views::session_key(name);
        self.query::<(Entity, &SessionKey)>()
            .iter(self)
            .find(|(_, at)| **at == key)
            .map(|(session, _)| session)
    }

    fn rewind(
        &mut self,
        name: &SessionName,
        entry: u64,
    ) -> Result<(), Failure> {
        let session =
            self.session_named(name).ok_or(Failure::NotFound)?;
        self.trigger(Rewind { session, entry });
        Ok(())
    }

    fn decree(&mut self, hamlet: &str, text: String) {
        let chief = self.resource::<Chief>();
        let (open, user) = (chief.session, chief.name.clone());
        let Some(here_key) = self.get::<SessionKey>(open).cloned()
        else {
            return;
        };
        let here = self
            .get::<TalksTo>(open)
            .and_then(|with| agents::hamlet_of(self, with.0))
            .unwrap_or(HOME);
        let key = if here == hamlet {
            here_key.clone()
        } else {
            SessionKey::with_mayor(CHANNEL, &user, hamlet)
        };
        let elsewhere = key != here_key;
        let entries = |world: &World, session: Option<Entity>| {
            session
                .and_then(|session| world.get::<Transcript>(session))
                .map_or(0, |said| said.items().len())
        };
        let there = self.session_named(&views::session_name(&key));
        let before =
            (entries(self, there), entries(self, Some(open)));
        self.trigger(UserMessage {
            key: key.clone(),
            user,
            text,
        });
        self.flush();
        if !elsewhere {
            return;
        }
        let Some(there) =
            self.session_named(&views::session_name(&key))
        else {
            return;
        };
        // The core tells every conversation of some answers itself.
        let told = answers(self, open, before.1);
        let answers = answers(self, there, before.0);
        if let Some(mut said) = self.get_mut::<Transcript>(open) {
            for answer in answers {
                if !told.contains(&answer) {
                    said.apply(&Reply::Notice(answer));
                }
            }
        }
    }

    fn open_house(&self) -> Home {
        let home = &self.resource::<HomeDir>().0;
        let open = self.resource::<Chief>().session;
        session::house_at(home, self.get::<Seat>(open))
    }

    fn papers(&self) -> Papers {
        let kept = papers::Papers::read(&self.open_house());
        let told =
            |text: String| text.chars().take(PAPERS_TOLD).collect();
        Papers {
            notes: told(kept.notes),
            character: told(kept.character),
            skills: kept
                .skills
                .into_iter()
                .map(|(name, description)| Skill {
                    name,
                    description,
                })
                .collect(),
            gifts: self.gifts(),
        }
    }

    fn gifts(&self) -> Vec<Gift> {
        let open = self.resource::<Chief>().session;
        let offers = self.get::<TalksTo>(open).and_then(|to| {
            let hamlet = self.get::<LivesIn>(to.0)?.0;
            let place = self.get::<Settlement>(hamlet)?.record();
            Some(place.offers(&agents::name_of(self, to.0)))
        });
        offers
            .unwrap_or_default()
            .into_iter()
            .map(|offer| Gift {
                skill: offer.skill,
                from: offer.from,
            })
            .collect()
    }

    fn prompt(&self) -> Result<String, Failure> {
        let open = self.resource::<Chief>().session;
        let Some(seat) = self.get::<Seat>(open) else {
            return Ok(String::new());
        };
        session::prompt_of(self, seat)
            .map_err(|why| Failure::Failed(format!("{why:#}")))
    }

    fn edit_character(&self) -> Result<(), Failure> {
        let failed =
            |why: std::io::Error| Failure::Failed(why.to_string());
        let file = papers::character_file(&self.open_house())
            .map_err(failed)?;
        core::browse(&file.to_string_lossy()).map_err(failed)
    }

    fn skills(&self) -> Vec<Skill> {
        listed(self.resource::<Skills>())
    }
}

fn listed(skills: &Skills) -> Vec<Skill> {
    skills
        .0
        .iter()
        .map(|skill| Skill {
            name: skill.name.clone(),
            description: skill.description.clone(),
        })
        .collect()
}

/// The notices and errors of `session` after its first `skip`
/// entries.
fn answers(
    world: &World,
    session: Entity,
    skip: usize,
) -> Vec<String> {
    world
        .get::<Transcript>(session)
        .into_iter()
        .flat_map(|said| said.items().iter().skip(skip))
        .filter_map(|item| match &item.entry {
            Entry::Notice(text) | Entry::Error(text) => {
                Some(text.clone())
            }
            _ => None,
        })
        .collect()
}

/// Tells the watchers of each message that came or changed, in each
/// session, since they were last told: the messages each session
/// held then.
fn tell_said(
    sessions: Query<(Entity, &SessionKey, Ref<Transcript>)>,
    mut watchers: ResMut<Watchers>,
    mut shown: Local<HashMap<Entity, Vec<Item>>>,
) {
    if !watchers.watched() {
        shown.clear();
        return;
    }
    shown.retain(|session, _| sessions.contains(*session));
    for (session, key, said) in &sessions {
        let now = said.items();
        let Some(was) = shown.get_mut(&session) else {
            shown.insert(session, now.to_vec());
            continue;
        };
        if !said.is_changed() {
            continue;
        }
        let name = views::session_name(key);
        let kept = now.len() >= was.len()
            && was.iter().zip(now).all(|(was, now)| was.id == now.id);
        if !kept {
            *was = now.to_vec();
            watchers.tell(AgentsEvent::Said {
                session: name,
                messages: Vec::new(),
                again: true,
            });
            continue;
        }
        // A tool call is closed where it began, so the first one
        // still open may change too.
        let from = was
            .iter()
            .position(|item| {
                matches!(item.entry, Entry::Tool { exit: None, .. })
            })
            .unwrap_or(was.len().saturating_sub(1));
        let messages = now
            .iter()
            .enumerate()
            .skip(from)
            .filter(|(at, item)| was.get(*at) != Some(*item))
            .map(|(_, item)| views::message(&said, item))
            .collect::<Vec<_>>();
        if messages.is_empty() {
            continue;
        }
        was.truncate(from);
        was.extend_from_slice(&now[from..]);
        watchers.tell(AgentsEvent::Said {
            session: name,
            messages,
            again: false,
        });
    }
}

fn tell_skills(skills: Res<Skills>, mut watchers: ResMut<Watchers>) {
    if skills.is_changed() && watchers.watched() {
        watchers.tell(AgentsEvent::Skills(listed(&skills)));
    }
}

fn rewound(
    done: On<Rewound>,
    keys: Query<&SessionKey>,
    mut watchers: ResMut<Watchers>,
) {
    if let Ok(key) = keys.get(done.session) {
        watchers.tell(AgentsEvent::Rewound {
            session: views::session_name(key),
            text: done.text.clone(),
        });
    }
}
