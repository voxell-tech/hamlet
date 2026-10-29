//! What of the home's can be sold: its agents and its hamlets, each
//! marked when a listing of the chief's already sells it.

use bevy::ecs::name::Name;
use bevy::ecs::query::{With, Without};
use bevy::ecs::world::World;
use hamlet_client::realm::{
    AgentNumber, HamletId, HouseSkills, Left, LivesIn, Purpose,
};
use hamlet_protocol::client::{Address, Packing};

use crate::bazaar::market::Market;
use crate::bazaar::market::deal::hamlets_of;
use crate::bazaar::market::lot::{AgentMeta, HamletMeta, Kind, Lot};
use crate::bazaar::market::versions::shown;

/// Something of the home's that can be sold.
#[derive(Clone, PartialEq, Debug)]
pub(crate) struct Item {
    /// The agent's address, or the hamlet's id.
    pub key: String,
    pub name: String,
    /// What it does, or what it holds.
    pub about: String,
    pub counts: Vec<String>,
    /// A listing of the chief's sells it already.
    pub listed: bool,
    pub wares: Packing,
}

/// The home's items of `kind`.
pub(crate) fn items_of(world: &World, kind: Kind) -> Vec<Item> {
    let mine = world
        .resource::<Market>()
        .mine
        .iter()
        .filter(|lot| lot.kind == kind)
        .map(|lot| {
            let named = match kind {
                Kind::Hamlet => lot.meta::<HamletMeta>().name,
                _ => lot.meta::<AgentMeta>().name,
            };
            (named, shown(lot).title)
        })
        .collect::<Vec<_>>();
    let listed = |name: &str, shown: &str| {
        mine.iter().any(|(named, title)| {
            named == name || title.eq_ignore_ascii_case(shown)
        })
    };
    let hamlets = hamlets_of(world);
    let agents = agents_of(world, &hamlets);
    match kind {
        Kind::Agent => agents
            .into_iter()
            .map(|agent| {
                let shown = format!(
                    "{} of {}",
                    agent.name, agent.hamlet_name
                );
                Item {
                    key: format!("{}@{}", agent.name, agent.hamlet),
                    listed: listed(&agent.name, &agent.name),
                    about: match agent.purpose.is_empty() {
                        true => shown.clone(),
                        false => agent.purpose.clone(),
                    },
                    counts: vec![counted(agent.skills, "skill")],
                    wares: Packing::Agent {
                        agent: Address {
                            agent: agent.name.clone(),
                            hamlet: agent.hamlet.clone(),
                        },
                        notes: false,
                    },
                    name: agent.name,
                }
            })
            .collect(),
        Kind::Hamlet => hamlets
            .iter()
            .map(|(id, name)| {
                let living = agents
                    .iter()
                    .filter(|agent| agent.hamlet == *id)
                    .count();
                let about = hamlet_client::realm::mayor_of(world, id)
                    .and_then(|mayor| world.get::<Purpose>(mayor))
                    .map(|one| one.0.trim().to_string())
                    .filter(|one| !one.is_empty())
                    .unwrap_or_else(|| format!("The hamlet {name}"));
                // The kind of hamlet it is, which the protocol holds
                // to 1 to 60 letters.
                let kind = about.chars().take(60).collect::<String>();
                Item {
                    key: id.clone(),
                    name: name.clone(),
                    about,
                    counts: vec![counted(living, "agent")],
                    listed: listed(id, name),
                    wares: Packing::Hamlet {
                        hamlet: id.clone(),
                        kind,
                        notes: false,
                    },
                }
            })
            .collect(),
        Kind::Plugin => Vec::new(),
    }
}

/// The item of the home's that `lot` sells, if it is still there.
pub(super) fn source_of(world: &World, lot: &Lot) -> Option<Item> {
    let (named, title) = match lot.kind {
        Kind::Hamlet => {
            (lot.meta::<HamletMeta>().name, shown(lot).title)
        }
        _ => (lot.meta::<AgentMeta>().name, shown(lot).title),
    };
    let all = items_of(world, lot.kind);
    let by_key = |item: &&Item| match lot.kind {
        Kind::Hamlet => item.key == named,
        _ => item.name == named,
    };
    all.iter()
        .find(by_key)
        .or_else(|| {
            all.iter()
                .find(|item| item.name.eq_ignore_ascii_case(&title))
        })
        .cloned()
}

fn counted(count: usize, noun: &str) -> String {
    match count {
        1 => format!("1 {noun}"),
        _ => format!("{count} {noun}s"),
    }
}

/// A living agent of the realm.
struct Living {
    name: String,
    hamlet: String,
    hamlet_name: String,
    purpose: String,
    skills: usize,
}

/// The living agents of the realm, by hamlet then name.
fn agents_of(
    world: &World,
    hamlets: &[(String, String)],
) -> Vec<Living> {
    let Some(mut query) = world.try_query_filtered::<(
        &Name,
        &LivesIn,
        Option<&Purpose>,
        Option<&HouseSkills>,
    ), (With<AgentNumber>, Without<Left>)>(
    ) else {
        return Vec::new();
    };
    let mut all = query
        .iter(world)
        .filter_map(|(name, lives, purpose, skills)| {
            let hamlet = world.get::<HamletId>(lives.0)?.0.clone();
            let hamlet_name = hamlets
                .iter()
                .find(|(id, _)| *id == hamlet)
                .map_or_else(
                    || hamlet.clone(),
                    |(_, shown)| shown.clone(),
                );
            Some(Living {
                name: name.as_str().to_string(),
                hamlet,
                hamlet_name,
                purpose: purpose
                    .map_or(String::new(), |one| one.0.clone()),
                skills: skills.map_or(0, |one| one.0.len()),
            })
        })
        .collect::<Vec<_>>();
    let order = |hamlet: &str| {
        hamlets.iter().position(|(id, _)| id == hamlet)
    };
    all.sort_by(|a, b| {
        (order(&a.hamlet), &a.name).cmp(&(order(&b.hamlet), &b.name))
    });
    all
}
