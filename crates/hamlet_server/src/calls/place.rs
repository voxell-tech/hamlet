//! Where an agent's tools work and which it is offered (H35): the
//! Hand in its lodge, a hire in its lodge and its hamlet's folder, a
//! worker in its scratch folder and where its starter works.

use std::path::PathBuf;

use anyhow::Result;
use hamlet_protocol::runtime::ConfigureTools;
use hamlet_store::Db;
use hamlet_store::ids::{AgentId, HamletId};

use crate::layout::Layout;
use crate::oversight::gate::Caller;
use crate::society::agents::{self, AgentKind};
use crate::society::hamlets::{self, Mode};
use crate::society::{hires, limits, tree, workers};
use crate::talk::conversations::Kind;
use crate::work::tools::Place;

/// Every agent's tools but a worker's.
const EVERYONE: [&str; 6] = [
    "tell",
    "ask",
    "start_worker",
    "submit_skill",
    "ask_librarian",
    "ask_chief",
];

/// A lead's tools over its own tree, which the Hand has over all.
const LEADS: [&str; 10] = [
    "create_scholar",
    "hire",
    "end_hire",
    "merge_hires",
    "set_model",
    "set_tools",
    "set_allowance",
    "nickname",
    "lay_road",
    "take_up_road",
];

/// The Hand's own tools.
const HAND: [&str; 8] = [
    "found_hamlet",
    "seat_mayor",
    "remove_hamlet",
    "read_record",
    "read_scholar",
    "edit_scholar",
    "tag",
    "revert",
];

/// Where an agent's tools work, and how its calls are overseen.
#[derive(Clone, Debug)]
pub(crate) struct Where {
    pub caller: Caller,
    pub place: Place,
    /// The hamlet whose limits and mode its calls are under.
    pub hamlet: Option<HamletId>,
    /// Whether edits in the folder and commands ask a chief (C21).
    pub manual: bool,
}

/// Where `agent`'s tools work.
pub(crate) fn of(
    db: &mut Db,
    layout: &Layout,
    agent: AgentId,
) -> Result<Where> {
    let shared = vec![layout.library(), layout.tools()];
    let kept = agents::get(db, agent)?;
    Ok(match kept.kind {
        AgentKind::Hand => Where {
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
            Where {
                caller: Caller::Hire,
                place: Place {
                    lodge: layout.lodge(&hire.hamlet, hire.number),
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
                            layout.lodge(&hire.hamlet, hire.number),
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
            Where {
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

/// Where the tools of `conversation` work: its agent's place, but a
/// dream writes the checkout of its scholar's `main` in place of a
/// folder, and asks nothing.
pub(crate) fn of_conversation(
    db: &mut Db,
    layout: &Layout,
    conversation: &crate::talk::conversations::Conversation,
) -> Result<Where> {
    let mut at = of(db, layout, conversation.agent)?;
    if conversation.kind == Kind::Dream
        && let Some(dream) = crate::society::dreams::of_conversation(
            db,
            conversation.id,
        )?
    {
        at.place.folder = Some(crate::knowledge::dreaming::checkout(
            layout, dream.id,
        ));
        at.manual = false;
    }
    Ok(at)
}

/// The folder `hamlet` works on, and whether its work there asks.
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
/// `names`, the whole catalogue. An errand and a dream offer what
/// their own instructions need, which is the built-in tools.
pub(crate) fn tools(
    db: &mut Db,
    agent: AgentId,
    kind: Kind,
    names: &[String],
    society: &[&str],
) -> Result<ConfigureTools> {
    let built_in = names
        .iter()
        .filter(|name| !society.contains(&name.as_str()))
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
            only.extend(EVERYONE.map(String::from));
            only.extend(LEADS.map(String::from));
            only.extend(HAND.map(String::from));
            only
        }
        AgentKind::Hire => {
            let hire = hires::get(db, agent)?;
            let off = hires::tools_off(db, agent)?;
            let mut only = built_in
                .into_iter()
                .filter(|name| !off.contains(name))
                .collect::<Vec<_>>();
            only.extend(EVERYONE.map(String::from));
            only.push("request_road".into());
            let most = limits::of(db, &hire.hamlet)?.depth;
            if tree::depth(db, agent)? < most {
                only.extend(LEADS.map(String::from));
            }
            if hires::is_mayor(db, agent)?
                && hamlets::all(db)?
                    .iter()
                    .any(|h| h.is_living() && h.id != hire.hamlet)
            {
                only.push("agree_road".into());
            }
            only
        }
    };
    if matches!(kind, Kind::Chat) && kept.kind != AgentKind::Worker {
        only.push("object".into());
    }
    match kind {
        // An errand answers in words alone.
        Kind::Errand => only.clear(),
        Kind::Dream => {
            only.retain(|name| !society.contains(&name.as_str()));
        }
        _ => {}
    }
    only.retain(|name| names.contains(name));
    only.sort();
    only.dedup();
    Ok(ConfigureTools::Only { only })
}
