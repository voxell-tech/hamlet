//! The realm (H6): its hamlets' places beyond their records, the
//! roads and offices between hamlets, agreed moves and removal asks.

pub(crate) mod dealings;
mod deeds;
use anyhow::Result;
pub(crate) use deeds::{Unfounded, deed, found_from, found_in};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::record::{self, AgentRef, Hamlet};
use crate::society;

/// The kind of the realm's society document.
const KIND: &str = "realm";

/// An agent, or a hamlet's office, as a lasting road end.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum End {
    Office { office: String },
    Agent(AgentRef),
}

/// Where a hamlet is shown, in the world's coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct At {
    pub x: f64,
    pub z: f64,
}

/// The vote a hamlet was founded by.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct From {
    pub hamlet: String,
    pub vote: u64,
}

/// Who closed a hamlet, and why.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Removed {
    pub at: i64,
    pub by: String,
    pub reason: String,
}

/// A hamlet's place in the realm.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Place {
    pub id: String,
    pub title: Option<String>,
    pub at: Option<At>,
    pub from: Option<From>,
    pub removed: Option<Removed>,
    pub erased: bool,
}

/// A road between hamlets, or between two hamlets' offices.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Road {
    pub a: End,
    pub b: End,
    pub agreed: Vec<String>,
    pub reason: String,
    pub messages: u64,
    pub last_from: Option<End>,
    pub last_used: i64,
}

/// One hamlet's agreement to a move.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Agreed {
    pub hamlet: String,
    pub keep: Vec<u64>,
    pub reason: String,
    pub at: i64,
}

/// A move of an agent with its house, as the mayors agree it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Move {
    pub agent: AgentRef,
    pub to: String,
    pub agreed: Vec<Agreed>,
}

/// A mayor's ask that a hamlet be removed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Removal {
    pub hamlet: String,
    pub by: String,
    pub reason: String,
    pub at: i64,
}

/// A house move recorded and not yet finished on disk.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Pending {
    pub from: AgentRef,
    pub to: AgentRef,
}

/// The realm's places, agreements and unfinished house move.
#[derive(
    Clone, Debug, Default, PartialEq, Serialize, Deserialize,
)]
pub(crate) struct Realm {
    pub hamlets: Vec<Place>,
    pub roads: Vec<Road>,
    pub offices: Vec<Road>,
    pub moves: Vec<Move>,
    pub removals: Vec<Removal>,
    pub pending: Option<Pending>,
}

impl Realm {
    /// The living hamlets.
    pub fn live(&self) -> impl Iterator<Item = &Place> {
        self.hamlets
            .iter()
            .filter(|h| h.removed.is_none() && !h.erased)
    }

    /// Whether `hamlet` lives.
    pub fn is_living(&self, hamlet: &str) -> bool {
        self.live().any(|h| h.id == hamlet)
    }
}

/// The realm as the transaction `db` holds it. A hamlet with a record
/// and no place yet is placed, unnamed, where it was founded.
pub(crate) fn read(db: &Connection) -> Result<Realm> {
    let mut realm = society::document::<Realm>(db, KIND, KIND)?;
    let mut ids = db
        .prepare("SELECT id FROM record_hamlet ORDER BY id")?
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    // Home comes first, as it was founded first.
    ids.sort_by_key(|id| id != hamlet_protocol::HOME);
    for id in ids {
        if !realm.hamlets.iter().any(|h| h.id == id) {
            realm.hamlets.push(Place {
                id,
                title: None,
                at: None,
                from: None,
                removed: None,
                erased: false,
            });
        }
    }
    Ok(realm)
}

/// Keeps the realm whole.
pub(crate) fn write(db: &Connection, realm: &Realm) -> Result<()> {
    society::keep(db, KIND, KIND, realm)
}

/// The ids of the living hamlets.
pub(crate) fn live(db: &Connection) -> Result<Vec<String>> {
    Ok(read(db)?.live().map(|h| h.id.clone()).collect())
}

/// A living agent an address names, as a tool resolves it from
/// `from`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Resolved {
    pub agent: AgentRef,
    /// Its office or name, with `@hamlet` beyond the sender's own.
    pub address: String,
}

/// Why an address names no living agent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Unresolved {
    Malformed,
    Unknown(String),
    Departed(String),
}

impl Unresolved {
    /// The words for the agent that gave `text`.
    pub fn text(&self, text: &str) -> String {
        match self {
            Self::Malformed => format!("no agent named `{text}`"),
            Self::Unknown(address) => {
                format!("no agent named `{address}`")
            }
            Self::Departed(farewell) => farewell.clone(),
        }
    }
}

/// The living agent `address` names from `from`: `name` or `mayor` in
/// `from` first, or `name@hamlet`.
pub(crate) fn resolve(
    db: &Connection,
    from: &str,
    address: &str,
) -> Result<std::result::Result<Resolved, Unresolved>> {
    let mut parts = address.split('@');
    let name = parts.next().unwrap_or_default();
    let place = parts.next();
    if name.is_empty() || parts.next().is_some() || place == Some("")
    {
        return Ok(Err(Unresolved::Malformed));
    }
    let realm = read(db)?;
    let mut records = Vec::new();
    for hamlet in realm.live() {
        let wanted = match place {
            None => {
                name != hamlet_protocol::MAYOR || hamlet.id == from
            }
            Some(place) => hamlet.id == place,
        };
        if wanted && let Some(record) = record::read(db, &hamlet.id)?
        {
            records.push(record);
        }
    }
    for record in &records {
        if let Some(agent) = record.named(name) {
            let address = if agent.number != record.mayor {
                agent.name.clone()
            } else if record.id == from {
                "mayor".into()
            } else {
                format!("mayor@{}", record.id)
            };
            return Ok(Ok(Resolved {
                agent: AgentRef {
                    hamlet: record.id.clone(),
                    number: agent.number,
                },
                address,
            }));
        }
    }
    for record in &records {
        if let Some(gone) = record
            .archive
            .iter()
            .rev()
            .find(|a| a.agent.name == name && a.back.is_none())
        {
            return Ok(Err(Unresolved::Departed(
                record.farewell(gone),
            )));
        }
    }
    Ok(Err(Unresolved::Unknown(address.into())))
}

/// The hamlet and mayor an end stands for while it lives.
fn end_of(
    end: &End,
    live: &[String],
    records: &[Hamlet],
) -> Option<AgentRef> {
    let hamlet = match end {
        End::Office { office } => office,
        End::Agent(agent) => &agent.hamlet,
    };
    if !live.contains(hamlet) {
        return None;
    }
    let record = records.iter().find(|r| r.id == *hamlet)?;
    let number = match end {
        End::Office { .. } => record.mayor,
        End::Agent(agent) => agent.number,
    };
    record.agents.iter().any(|a| a.number == number).then(|| {
        AgentRef {
            hamlet: hamlet.clone(),
            number,
        }
    })
}

/// A road between hamlets with its ends as living agents.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Joined {
    pub a: AgentRef,
    pub b: AgentRef,
    pub messages: u64,
    pub last_from: Option<AgentRef>,
    pub last_used: i64,
}

/// The roads between living agents of different hamlets, the mayors'
/// own office roads among them.
pub(crate) fn roads(db: &Connection) -> Result<Vec<Joined>> {
    let realm = read(db)?;
    let live = realm.live().map(|h| h.id.clone()).collect::<Vec<_>>();
    let mut records = Vec::new();
    for hamlet in &live {
        if let Some(record) = record::read(db, hamlet)? {
            records.push(record);
        }
    }
    let mut all = realm.roads.clone();
    for (i, a) in live.iter().enumerate() {
        for b in &live[i + 1..] {
            let pair = |r: &&Road| match (&r.a, &r.b) {
                (
                    End::Office { office: x },
                    End::Office { office: y },
                ) => (x == a && y == b) || (x == b && y == a),
                _ => false,
            };
            all.push(
                realm.offices.iter().find(pair).cloned().unwrap_or(
                    Road {
                        a: End::Office { office: a.clone() },
                        b: End::Office { office: b.clone() },
                        agreed: vec![a.clone(), b.clone()],
                        reason: "office".into(),
                        messages: 0,
                        last_from: None,
                        last_used: 0,
                    },
                ),
            );
        }
    }
    Ok(all
        .iter()
        .filter_map(|r| {
            let a = end_of(&r.a, &live, &records)?;
            let b = end_of(&r.b, &live, &records)?;
            (a.hamlet != b.hamlet
                && r.agreed.contains(&a.hamlet)
                && r.agreed.contains(&b.hamlet))
            .then(|| Joined {
                a,
                b,
                messages: r.messages,
                last_used: r.last_used,
                last_from: r
                    .last_from
                    .as_ref()
                    .and_then(|e| end_of(e, &live, &records)),
            })
        })
        .collect())
}

/// Counts one message from `from` to `to` on the road between their
/// hamlets, the mayors' office road if they are both mayors; whether
/// a road carried it.
pub(crate) fn carry(
    db: &Connection,
    from: &AgentRef,
    to: &AgentRef,
    at: i64,
) -> Result<bool> {
    let mut realm = read(db)?;
    let live = realm.live().map(|h| h.id.clone()).collect::<Vec<_>>();
    let mut mayors = Vec::new();
    for hamlet in &live {
        if let Some(record) = record::read(db, hamlet)? {
            mayors.push((hamlet.clone(), record.mayor));
        }
    }
    let is_mayor = |agent: &AgentRef| {
        mayors.contains(&(agent.hamlet.clone(), agent.number))
    };
    let matches = |end: &End, agent: &AgentRef| match end {
        End::Office { office } => {
            *office == agent.hamlet && is_mayor(agent)
        }
        End::Agent(end) => end == agent,
    };
    let joins = |road: &Road| {
        ((matches(&road.a, from) && matches(&road.b, to))
            || (matches(&road.b, from) && matches(&road.a, to)))
            && road.agreed.contains(&from.hamlet)
            && road.agreed.contains(&to.hamlet)
    };
    let road = if let Some(road) =
        realm.roads.iter_mut().find(|r| joins(r))
    {
        road
    } else if let Some(at) = realm.offices.iter().position(joins) {
        &mut realm.offices[at]
    } else if is_mayor(from) && is_mayor(to) {
        realm.offices.push(Road {
            a: End::Office {
                office: from.hamlet.clone(),
            },
            b: End::Office {
                office: to.hamlet.clone(),
            },
            agreed: vec![from.hamlet.clone(), to.hamlet.clone()],
            reason: "office".into(),
            messages: 0,
            last_from: None,
            last_used: 0,
        });
        realm.offices.last_mut().expect("an office road just added")
    } else {
        return Ok(false);
    };
    road.messages += 1;
    road.last_from = Some(if matches(&road.a, from) {
        road.a.clone()
    } else {
        road.b.clone()
    });
    road.last_used = at;
    write(db, &realm)?;
    Ok(true)
}
