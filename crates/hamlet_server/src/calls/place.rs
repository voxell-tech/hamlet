//! An agent's workplace and the tools it is offered (H35): the Hand
//! in its lodge, a hire in its lodge and its hamlet's folder, a
//! worker in its scratch folder and where its starter works.

use std::path::PathBuf;

use anyhow::Result;
use hamlet_protocol::runtime::ConfigureTools;
use hamlet_store::Db;
use hamlet_store::ids::{AgentId, HamletId};

use crate::conversations::Kind;
use crate::realm::layout::Layout;
use crate::society::agents::{self, AgentKind};
use crate::society::hamlets::{self, Mode};
use crate::society::oversight::gate::Caller;
use crate::society::{hires, workers};
use crate::tools::builtin::Place;
use crate::tools::catalogue::{Offered, Society};

/// The place an agent's tools work in, and how its calls are
/// overseen.
#[derive(Clone, Debug)]
pub(crate) struct Workplace {
    pub caller: Caller,
    pub place: Place,
    /// The hamlet whose limits and mode its calls are under.
    pub hamlet: Option<HamletId>,
    /// Set when edits in the folder and commands wait for a chief's
    /// answer (C21).
    pub manual: bool,
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
                lodge: layout.hand(),
                folder: None,
                realm: layout.root().to_owned(),
                shared,
            },
            hamlet: None,
            manual: false,
        },
        AgentKind::Hire => {
            let hire = hires::get(db, agent)?;
            let (folder, manual) = folder(db, &hire.hamlet)?;
            Workplace {
                caller: Caller::Hire,
                place: Place {
                    lodge: layout.lodge(&hire.hamlet, hire.agent),
                    folder,
                    realm: layout.root().to_owned(),
                    shared,
                },
                hamlet: Some(hire.hamlet),
                manual,
            }
        }
        AgentKind::Worker => {
            let worker = workers::get(db, agent)?;
            let mut shared = shared;
            let mut hamlet = worker.hamlet.clone();
            if let Some(starter) = worker.starter {
                match agents::get(db, starter)?.kind {
                    AgentKind::Hire => {
                        let hire = hires::get(db, starter)?;
                        shared.push(
                            layout.lodge(&hire.hamlet, hire.agent),
                        );
                        hamlet = hamlet.or(Some(hire.hamlet));
                    }
                    AgentKind::Hand => shared.push(layout.hand()),
                    AgentKind::Worker => {}
                }
            }
            let (folder, manual) = match &hamlet {
                Some(hamlet) => folder(db, hamlet)?,
                None => (None, false),
            };
            Workplace {
                caller: Caller::Worker,
                place: Place {
                    lodge: layout.worker(agent),
                    folder,
                    realm: layout.root().to_owned(),
                    shared,
                },
                hamlet,
                manual,
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
        at.manual = false;
    }
    Ok(at)
}

/// The folder `hamlet` works on, and whether its work there waits for
/// a chief.
fn folder(
    db: &mut Db,
    hamlet: &HamletId,
) -> Result<(Option<PathBuf>, bool)> {
    let kept = hamlets::get(db, hamlet)?;
    let folder = kept.folder.map(PathBuf::from);
    let manual = folder.is_some() && kept.mode == Mode::Manual;
    Ok((folder, manual))
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
        AgentKind::Hire => {
            let hire = hires::get(db, agent)?;
            let off = hires::tools_off(db, agent)?;
            let mut only = built_in
                .into_iter()
                .filter(|name| !off.contains(name))
                .collect::<Vec<_>>();
            only.extend(Society::offered(Offered::Everyone));
            only.extend(Society::offered(Offered::Hires));
            // Any hire can become a parent: the tree has no floor.
            only.extend(Society::offered(Offered::Parents));
            if hires::is_lead(db, agent)?
                && hamlets::all(db)?
                    .iter()
                    .any(|h| h.is_living() && h.id != hire.hamlet)
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
