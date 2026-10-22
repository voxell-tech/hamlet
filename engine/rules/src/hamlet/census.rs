//! The hamlet as its record tells it: who lives there, who left and
//! how, the roads and the tasks.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::hamlet::roads::{Road, ends};
use crate::hamlet::{FOUNDER, Hamlet, MAYOR, Resident};
use crate::record::text;

/// A road an absorbed agent passed to the agent it was merged into.
#[derive(Clone, Debug, PartialEq)]
struct Inherited {
    /// The id of the agent at the other end, the mayor's for the
    /// office.
    peer: u64,
    messages: u32,
    /// The merge made the road; the kept agent had none there.
    fresh: bool,
}

/// The merge an agent left in.
#[derive(Clone, Debug, PartialEq)]
pub struct Merge {
    /// The agent it was merged into, as that was before.
    pub kept: Resident,
    inherited: Vec<Inherited>,
}

/// An agent that left, and what is kept of it.
#[derive(Clone, Debug, PartialEq)]
pub struct Archived {
    /// Its folder under `archive/`.
    pub id: String,
    /// The agent as it was when it left.
    pub resident: Resident,
    pub left: u64,
    pub reason: String,
    pub by: String,
    /// `None` when it was removed.
    pub merge: Option<Merge>,
    /// Its roads when it left, each with the id of the agent at the
    /// other end, the mayor's for the office.
    roads: Vec<(u64, Road)>,
    /// The name it came back under.
    pub back: Option<String>,
}

impl Archived {
    /// The words for someone who tries to reach it.
    pub(crate) fn farewell(&self) -> String {
        let name = &self.resident.name;
        let by = by_whom(&self.by);
        match &self.merge {
            None => format!(
                "`{name}` was removed from the hamlet by {by} \
                 (\"{}\"). Its house is archived and its roads went \
                 with it; only a chief can restore it.",
                self.reason
            ),
            Some(merge) => format!(
                "`{name}` was merged into `{kept}` by {by} \
                 (\"{}\"). `{kept}` has its notes, skills and \
                 roads; talk to `{kept}`.",
                self.reason,
                kept = merge.kept.name
            ),
        }
    }
}

/// A task a chief gave an agent directly.
#[derive(Clone, Debug, PartialEq)]
pub struct Task {
    pub id: u64,
    agent: u64,
    pub user: String,
    pub text: String,
    /// The time its turn ended well. `None` while it is owed.
    pub(crate) done: Option<u64>,
}

/// The hamlet as its record tells it.
pub struct Census {
    /// The living agents besides the mayor.
    pub residents: Vec<Resident>,
    /// The agent that holds the mayor's office.
    pub mayor: Resident,
    /// The house of the first mayor is the hamlet's own folder.
    rooted: bool,
    pub roads: Vec<Road>,
    /// Every departure, oldest first.
    pub archive: Vec<Archived>,
    pub tasks: Vec<Task>,
    /// The model the mayor chose for an agent, by its number.
    models: BTreeMap<u64, String>,
}

impl Default for Census {
    fn default() -> Self {
        Census {
            residents: Vec::new(),
            mayor: Resident {
                name: FOUNDER.to_string(),
                purpose: String::new(),
                id: 0,
                since: 0,
            },
            rooted: true,
            roads: Vec::new(),
            archive: Vec::new(),
            tasks: Vec::new(),
            models: BTreeMap::new(),
        }
    }
}

impl Census {
    /// The census a record's lines come to, read from the first.
    pub fn replay(events: &[Value]) -> Census {
        let mut census = Census::default();
        for (index, e) in events.iter().enumerate() {
            match e["type"].as_str() {
                Some("agent_created" | "agent_arrived") => {
                    let (Some(name), Some(purpose)) =
                        (e["name"].as_str(), e["purpose"].as_str())
                    else {
                        continue;
                    };
                    census.roads.push(Road::new(MAYOR, name));
                    census.residents.push(Resident {
                        name: name.to_string(),
                        purpose: purpose.to_string(),
                        id: index as u64 + 1,
                        since: e["ts"].as_u64().unwrap_or_default(),
                    });
                }
                Some("founded") => census.founded(
                    &text(e, "mayor"),
                    &text(e, "purpose"),
                    e["house"] == "root",
                ),
                Some("mayor_changed") => census.seat(&text(e, "to")),
                Some("agent_left") => {
                    census.move_out(&text(e, "name"))
                }
                Some("road_built") => {
                    let (a, b) = (text(e, "a"), text(e, "b"));
                    if !census.roads.iter().any(|r| r.joins(&a, &b)) {
                        census.roads.push(Road::new(&a, &b));
                    }
                }
                Some("road_removed") => {
                    let (a, b) = (text(e, "a"), text(e, "b"));
                    census.roads.retain(|r| !r.joins(&a, &b));
                }
                Some("message") => {
                    let (from, to) = (text(e, "from"), text(e, "to"));
                    if let Some(road) = census
                        .roads
                        .iter_mut()
                        .find(|r| r.joins(&from, &to))
                    {
                        road.carry(
                            &from,
                            e["ts"].as_u64().unwrap_or_default(),
                        );
                    }
                }
                _ => census.apply(e),
            }
        }
        // An agent's own roads sleep while it holds the office.
        let holder = census.mayor.name.clone();
        census.roads.retain(|r| !r.has(&holder));
        census
    }

    /// The number of the agent at `address`: the mayor's for the
    /// office.
    pub(crate) fn id_of(&self, address: &str) -> Option<u64> {
        if address == MAYOR {
            return Some(self.mayor.id);
        }
        self.resident_id(address)
    }

    fn resident_id(&self, name: &str) -> Option<u64> {
        self.residents.iter().find(|r| r.name == name).map(|r| r.id)
    }

    /// The address of the agent number `id`.
    fn name_of(&self, id: u64) -> Option<String> {
        if id == self.mayor.id {
            return Some(MAYOR.to_string());
        }
        self.residents
            .iter()
            .find(|r| r.id == id)
            .map(|r| r.name.clone())
    }

    /// Everyone who lives in the hamlet, the mayor first.
    pub fn members(&self) -> Vec<Resident> {
        std::iter::once(&self.mayor)
            .chain(&self.residents)
            .cloned()
            .collect()
    }

    /// True when the house of the living agent `name` is the hamlet's
    /// own folder.
    pub(crate) fn at_root(&self, name: &str) -> bool {
        self.rooted
            && self
                .members()
                .iter()
                .any(|r| r.id == 0 && r.name == name)
    }

    /// Names the first mayor and says where its house is.
    pub(crate) fn founded(
        &mut self,
        name: &str,
        purpose: &str,
        rooted: bool,
    ) {
        self.rooted = rooted;
        let first = std::iter::once(&mut self.mayor)
            .chain(&mut self.residents)
            .find(|r| r.id == 0);
        let Some(first) = first else {
            return;
        };
        let was =
            std::mem::replace(&mut first.name, name.to_string());
        first.purpose = purpose.to_string();
        // In office it is reached as the office, not by name.
        if self.mayor.id == 0 {
            return;
        }
        for road in &mut self.roads {
            if road.has(&was) {
                *road = Road {
                    ends: ends(road.other(&was), name),
                    ..road.clone()
                };
            }
        }
    }

    /// Gives the office to the agent `to`. Whoever held it stays,
    /// with a road to the office like any other agent.
    pub(crate) fn seat(&mut self, to: &str) {
        let Some(at) =
            self.residents.iter().position(|r| r.name == to)
        else {
            return;
        };
        let was = std::mem::replace(
            &mut self.mayor,
            self.residents.remove(at),
        );
        if !self.roads.iter().any(|r| r.joins(MAYOR, &was.name)) {
            self.roads.push(Road::new(MAYOR, &was.name));
        }
        self.residents.push(was);
    }

    /// The agent `name` moved to another hamlet with its house.
    pub(crate) fn move_out(&mut self, name: &str) {
        let Some(at) =
            self.residents.iter().position(|r| r.name == name)
        else {
            return;
        };
        let gone = self.residents.remove(at);
        self.roads.retain(|r| !r.has(name));
        self.models.remove(&gone.id);
        self.tasks.retain(|t| t.agent != gone.id);
    }

    /// The model the mayor chose for the living agent `name`.
    pub fn model(&self, name: &str) -> Option<String> {
        self.models.get(&self.resident_id(name)?).cloned()
    }

    /// The latest departure under `name`, unless an agent of that
    /// name lives now.
    pub fn gone(&self, name: &str) -> Option<&Archived> {
        if self.residents.iter().any(|r| r.name == name) {
            return None;
        }
        self.archive
            .iter()
            .rev()
            .find(|a| a.back.is_none() && a.resident.name == name)
    }

    /// A chief's task that protects the agent `id` at `now`: one
    /// still owed, or one answered less than `hold_ms` ago.
    pub fn owed(
        &self,
        id: u64,
        now: u64,
        hold_ms: u64,
    ) -> Option<&Task> {
        self.tasks.iter().find(|t| {
            t.agent == id
                && t.done
                    .is_none_or(|at| now.saturating_sub(at) < hold_ms)
        })
    }

    /// The lines of a prompt about agents that left, the latest
    /// `most` of them, as `reader` should hear it. Empty when nobody
    /// left.
    pub(crate) fn farewells(
        &self,
        reader: Option<&Resident>,
        most: usize,
    ) -> String {
        let mut lines = self
            .archive
            .iter()
            .rev()
            .filter(|a| self.gone(&a.resident.name) == Some(*a))
            .take(most)
            .map(|a| {
                let name = &a.resident.name;
                match &a.merge {
                    None => format!("- {name}: removed\n"),
                    Some(m)
                        if reader
                            .is_some_and(|r| r.id == m.kept.id) =>
                    {
                        format!(
                            "- {name}: merged into you; its notes, \
                             skills and roads are yours\n"
                        )
                    }
                    Some(m) => format!(
                        "- {name}: merged into {kept}; talk to \
                         {kept}\n",
                        kept = m.kept.name
                    ),
                }
            })
            .collect::<Vec<_>>();
        if lines.is_empty() {
            return String::new();
        }
        lines.reverse();
        format!(
            "No longer here, and not to be asked or told:\n{}",
            lines.concat()
        )
    }

    fn depart(
        &mut self,
        e: &Value,
        name: &str,
        into: Option<(&str, &str)>,
    ) {
        let Some(at) =
            self.residents.iter().position(|r| r.name == name)
        else {
            return;
        };
        // A merge into an agent that had left by the time the line
        // was written did not happen.
        if let Some((kept, _)) = into
            && (kept == name
                || !self.residents.iter().any(|r| r.name == kept))
        {
            return;
        }
        let resident = self.residents.remove(at);
        let (mine, rest) = std::mem::take(&mut self.roads)
            .into_iter()
            .partition::<Vec<_>, _>(|r| r.has(name));
        self.roads = rest;
        let roads = mine
            .into_iter()
            .filter_map(|r| Some((self.id_of(r.other(name))?, r)))
            .collect::<Vec<_>>();
        let kept = into.and_then(|(kept, purpose)| {
            let k =
                self.residents.iter_mut().find(|r| r.name == kept)?;
            let before = k.clone();
            k.purpose = purpose.to_string();
            Some(before)
        });
        let merge = kept.map(|kept| {
            let mut inherited = Vec::new();
            for (peer, road) in &roads {
                let other = road.other(name);
                if other == kept.name {
                    continue;
                }
                let last_from = road.last_from.as_ref().map(|f| {
                    if f == name {
                        kept.name.clone()
                    } else {
                        f.clone()
                    }
                });
                let had = self
                    .roads
                    .iter_mut()
                    .find(|r| r.joins(&kept.name, other));
                let fresh = had.is_none();
                match had {
                    Some(r) => {
                        r.messages += road.messages;
                        if road.last_used > r.last_used {
                            r.last_used = road.last_used;
                            r.last_from = last_from;
                        }
                    }
                    None => self.roads.push(Road {
                        ends: ends(&kept.name, other),
                        messages: road.messages,
                        last_from,
                        last_used: road.last_used,
                    }),
                }
                inherited.push(Inherited {
                    peer: *peer,
                    messages: road.messages,
                    fresh,
                });
            }
            Merge { kept, inherited }
        });
        self.tasks.retain(|t| t.agent != resident.id);
        let text = |key: &str| {
            e[key].as_str().unwrap_or_default().to_string()
        };
        self.archive.push(Archived {
            id: text("archive"),
            resident,
            left: e["ts"].as_u64().unwrap_or_default(),
            reason: text("reason"),
            by: text("by"),
            merge,
            roads,
            back: None,
        });
    }

    fn come_back(&mut self, id: &str, name: &str) {
        let Some(entry) = self
            .archive
            .iter_mut()
            .find(|a| a.id == id && a.back.is_none())
        else {
            return;
        };
        let name = if name.is_empty() {
            entry.resident.name.clone()
        } else {
            name.to_string()
        };
        entry.back = Some(name.clone());
        let entry = entry.clone();
        let kept = entry.merge.as_ref().and_then(|merge| {
            let k = self
                .residents
                .iter_mut()
                .find(|r| r.id == merge.kept.id)?;
            k.purpose = merge.kept.purpose.clone();
            Some((k.name.clone(), &merge.inherited))
        });
        // The kept agent gives back the use the absorbed one's
        // roads brought it, and a road it has not used since.
        for (kept, i) in kept.iter().flat_map(|(kept, all)| {
            all.iter().map(move |i| (kept, i))
        }) {
            let Some(peer) = self.name_of(i.peer) else {
                continue;
            };
            let Some(at) =
                self.roads.iter().position(|r| r.joins(kept, &peer))
            else {
                continue;
            };
            let road = &mut self.roads[at];
            road.messages = road.messages.saturating_sub(i.messages);
            if i.fresh && road.messages == 0 {
                self.roads.remove(at);
            }
        }
        // Its roads come back where the agent at the other end is
        // the one it knew.
        for (peer, road) in &entry.roads {
            let Some(peer) = self.name_of(*peer) else {
                continue;
            };
            if self.roads.iter().any(|r| r.joins(&name, &peer)) {
                continue;
            }
            self.roads.push(Road {
                ends: ends(&name, &peer),
                messages: road.messages,
                last_from: road.last_from.as_ref().map(|f| {
                    if *f == entry.resident.name {
                        name.clone()
                    } else {
                        f.clone()
                    }
                }),
                last_used: road.last_used,
            });
        }
        if !self.roads.iter().any(|r| r.joins(MAYOR, &name)) {
            self.roads.push(Road::new(MAYOR, &name));
        }
        self.residents.push(Resident {
            name,
            purpose: entry.resident.purpose,
            id: entry.resident.id,
            since: entry.resident.since,
        });
    }

    /// Takes in one line about a departure, a return, an agent's
    /// model or a chief's task.
    fn apply(&mut self, e: &Value) {
        let text = |key: &str| {
            e[key].as_str().unwrap_or_default().to_string()
        };
        let ts = e["ts"].as_u64().unwrap_or_default();
        match e["type"].as_str() {
            Some("agent_removed") => {
                self.depart(e, &text("name"), None)
            }
            Some("agents_merged") => self.depart(
                e,
                &text("gone"),
                Some((&text("kept"), &text("purpose"))),
            ),
            Some("agent_restored") => {
                self.come_back(&text("archive"), &text("name"))
            }
            // Nothing is left of it, not even that it left.
            Some("agent_erased") => {
                let id = text("archive");
                self.archive.retain(|a| a.id != id);
            }
            Some("model_chosen") => {
                let Some(id) = self.resident_id(&text("name")) else {
                    return;
                };
                match text("model") {
                    model if model.is_empty() => {
                        self.models.remove(&id);
                    }
                    model => {
                        self.models.insert(id, model);
                    }
                }
            }
            Some("instructed") => {
                let Some(agent) = self.resident_id(&text("name"))
                else {
                    return;
                };
                self.tasks.push(Task {
                    id: e["task"].as_u64().unwrap_or(ts),
                    agent,
                    user: text("user"),
                    text: text("text"),
                    // A record from before tasks were numbered:
                    // nothing says how it ended.
                    done: e["task"].is_null().then_some(ts),
                });
            }
            // A turn that failed ends its task as an answer does:
            // the agent is held for as long after, and no longer.
            Some("answered") => {
                let (Some(agent), Some(task)) =
                    (self.id_of(&text("name")), e["task"].as_u64())
                else {
                    return;
                };
                // It also closes older tasks left open by a crash:
                // the chief and the agent have talked since.
                for t in self.tasks.iter_mut().filter(|t| {
                    t.agent == agent
                        && t.id <= task
                        && t.done.is_none()
                }) {
                    t.done = Some(ts);
                }
            }
            _ => {}
        }
    }
}

impl Hamlet {
    /// The record replayed from its first line.
    pub fn census(&self) -> Census {
        Census::replay(&self.events())
    }
}

/// The decider, as a chief reads it.
pub(crate) fn by_whom(by: &str) -> String {
    match by {
        "" | MAYOR => "the mayor".to_string(),
        chief => format!("the chief {chief}"),
    }
}
