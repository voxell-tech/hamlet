//! An agent's workplace and the tools it is offered (H35): the Hand
//! in its worktree, an agent in its worktree and its hamlet's folder,
//! a worker in its scratch folder and where its starter works.

use std::path::PathBuf;

use anyhow::Result;
use hamlet_protocol::runtime::ConfigureTools;
use hamlet_store::Db;
use hamlet_store::ids::{AgentId, HamletId};

use crate::conversations::Kind;
use crate::realm::layout::Layout;
use crate::society::access::Access;
use crate::society::agents::{self, AgentKind};
use crate::society::oversight::gate::Caller;
use crate::society::{hamlet_agents, hamlets, workers};
use crate::tools::builtin::Place;
use crate::tools::catalogue::{Offered, Society};

/// The place an agent's tools work in, and how its calls are
/// overseen.
#[derive(Clone, Debug)]
pub(crate) struct Workplace {
    pub caller: Caller,
    pub place: Place,
    /// The hamlet whose limits its calls are under.
    pub hamlet: Option<HamletId>,
    /// How far its calls go without a chief's answer (C21).
    pub access: Access,
}

impl Workplace {
    /// Edits of the folders wait for a chief's answer (C21).
    pub fn holds_edits(&self) -> bool {
        self.place.folder.is_some() && self.access.holds_edits()
    }

    /// Commands wait for a chief's answer (C21).
    pub fn holds_commands(&self) -> bool {
        self.place.folder.is_some() && self.access.holds_commands()
    }
}

/// Where `agent`'s tools run, given `access`: with the sandbox off
/// for full access.
fn placed(
    mut place: Place,
    folders: Vec<PathBuf>,
    access: Access,
) -> Place {
    let mut folders = folders.into_iter();
    place.folder = folders.next();
    place.also = folders.collect();
    place.unconfined = access.is_full();
    place
}

/// The workplace of `agent`.
pub(crate) fn of(
    db: &mut Db,
    layout: &Layout,
    agent: AgentId,
) -> Result<Workplace> {
    let shared = vec![layout.library(), layout.tools()];
    let kept = agents::get(db, agent)?;
    Ok(match kept.kind {
        AgentKind::Hand => Workplace {
            caller: Caller::Hand,
            place: Place {
                worktree: layout.hand(),
                folder: None,
                also: Vec::new(),
                unconfined: false,
                realm: layout.root().to_owned(),
                shared,
            },
            hamlet: None,
            access: Access::Auto,
        },
        AgentKind::Hamlet => {
            let hamlet_agent = hamlet_agents::get(db, agent)?;
            let folders = hamlets::working(db, &hamlet_agent.hamlet)?
                .into_iter()
                .map(PathBuf::from)
                .collect();
            let access = hamlet_agent.access;
            Workplace {
                caller: Caller::Hamlet,
                place: placed(
                    Place {
                        worktree: layout.worktree(
                            &hamlet_agent.hamlet,
                            hamlet_agent.agent,
                        ),
                        folder: None,
                        also: Vec::new(),
                        unconfined: false,
                        realm: layout.root().to_owned(),
                        shared,
                    },
                    folders,
                    access,
                ),
                hamlet: Some(hamlet_agent.hamlet),
                access,
            }
        }
        AgentKind::Worker => {
            let worker = workers::get(db, agent)?;
            let mut shared = shared;
            let mut hamlet = worker.hamlet.clone();
            // A worker goes as far as the agent that started it.
            let mut access = None;
            if let Some(starter) = worker.starter {
                match agents::get(db, starter)?.kind {
                    AgentKind::Hamlet => {
                        let starter =
                            hamlet_agents::get(db, starter)?;
                        shared.push(layout.worktree(
                            &starter.hamlet,
                            starter.agent,
                        ));
                        access = Some(starter.access);
                        hamlet = hamlet.or(Some(starter.hamlet));
                    }
                    AgentKind::Hand => shared.push(layout.hand()),
                    AgentKind::Worker => {}
                }
            }
            let folders = match &hamlet {
                Some(hamlet) => hamlets::working(db, hamlet)?
                    .into_iter()
                    .map(PathBuf::from)
                    .collect(),
                None => Vec::new(),
            };
            let access = access.unwrap_or(match hamlet {
                Some(_) => Access::Supervised,
                None => Access::Auto,
            });
            Workplace {
                caller: Caller::Worker,
                place: placed(
                    Place {
                        worktree: layout.worker(agent),
                        folder: None,
                        also: Vec::new(),
                        unconfined: false,
                        realm: layout.root().to_owned(),
                        shared,
                    },
                    folders,
                    access,
                ),
                hamlet,
                access,
            }
        }
    })
}

/// The workplace of `conversation`: its agent's.
pub(crate) fn of_conversation(
    db: &mut Db,
    layout: &Layout,
    conversation: &crate::conversations::Conversation,
) -> Result<Workplace> {
    let mut at = of(db, layout, conversation.agent)?;
    // A dream writes the checkout of its profile's `main` in place of
    // a folder, and needs no chief's answer.
    if conversation.kind == Kind::Dream
        && let Some(dream) = crate::society::dreams::of_conversation(
            db,
            conversation.id,
        )?
    {
        at.place.folder = Some(crate::knowledge::dreams::checkout(
            layout, dream.id,
        ));
        at.place.also.clear();
        at.place.unconfined = false;
        at.access = Access::Auto;
    }
    Ok(at)
}

/// The tools a conversation of `agent` of `kind` offers, out of
/// `names`, the whole catalogue.
pub(crate) fn tools(
    db: &mut Db,
    agent: AgentId,
    kind: Kind,
    names: &[String],
) -> Result<ConfigureTools> {
    let built_in = names
        .iter()
        .filter(|name| Society::named(name).is_none())
        .cloned()
        .collect::<Vec<_>>();
    let kept = agents::get(db, agent)?;
    let mut only = match kept.kind {
        AgentKind::Worker => {
            let given = workers::get(db, agent)?.tools()?;
            built_in
                .into_iter()
                .filter(|name| {
                    given.is_empty() || given.contains(name)
                })
                .collect()
        }
        AgentKind::Hand => {
            let mut only = built_in;
            only.extend(Society::offered(Offered::Everyone));
            only.extend(Society::offered(Offered::Parents));
            only.extend(Society::offered(Offered::Hand));
            only
        }
        AgentKind::Hamlet => {
            let own = hamlet_agents::get(db, agent)?.hamlet;
            let off = hamlet_agents::tools_off(db, agent)?;
            let mut only = built_in
                .into_iter()
                .filter(|name| !hamlet_agents::is_off(&off, name))
                .collect::<Vec<_>>();
            only.extend(Society::offered(Offered::Everyone));
            only.extend(Society::offered(Offered::HamletAgents));
            // Any agent can become a parent: the tree has no floor.
            only.extend(Society::offered(Offered::Parents));
            if hamlet_agents::is_lead(db, agent)?
                && hamlets::all(db)?
                    .iter()
                    .any(|h| h.is_living() && h.id != own)
            {
                only.extend(Society::offered(Offered::Leads));
            }
            only
        }
    };
    if matches!(kind, Kind::Chat) && kept.kind != AgentKind::Worker {
        only.extend(Society::offered(Offered::Chats));
    }
    match kind {
        // An errand answers in words alone.
        Kind::Errand => only.clear(),
        // A dream offers the built-in tools alone.
        Kind::Dream => {
            only.retain(|name| Society::named(name).is_none());
        }
        _ => {}
    }
    only.retain(|name| names.contains(name));
    only.sort();
    only.dedup();
    Ok(ConfigureTools::Only { only })
}
