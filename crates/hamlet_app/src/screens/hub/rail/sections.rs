//! The rail's sections, built from the realm as the client holds it.

use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;
use hamlet_client::realm::{
    HamletId, Kind, Left, Lived, LivesIn, MergedInto, Profile,
    Skills, name_of, shown_of,
};
use hamlet_client::waits::Waiting;
use hamlet_protocol::client as protocol;
use hamlet_ui::widgets::icons::Glyph;

use super::lines::{
    Agent, Group, Line, Node, Section, Target, agents, family_lines,
    fold_leads, is_current, kind_glyph, matches, page_line, tree,
};
use super::{LEFT_SHOWN, LINE_SHOWN, Rail, Tab};
use crate::standing::{Standing, need_you};

/// What waits for the chief, one row for each who asks: the words of
/// its oldest wait, how long it has waited, and how many more; and
/// how many waits there are in all.
pub(super) fn needs(
    world: &World,
    all: &[Agent],
) -> (Vec<Line>, usize) {
    let Some(mut query) = world.try_query::<&Waiting>() else {
        return (Vec::new(), 0);
    };
    let mut waits = query.iter(world).collect::<Vec<_>>();
    let total = waits.len();
    waits.sort_by_key(|wait| wait.since);
    let mut lines = Vec::<Line>::new();
    let mut counts = Vec::<usize>::new();
    for wait in waits {
        // A newcomer is about the profile at the harbor, not about
        // whoever made it, and leads to its page.
        if let protocol::WaitDetail::Newcomer { profile } =
            &wait.detail
            && let Some(entity) =
                hamlet_client::realm::profile(world, *profile)
        {
            let name = world
                .get::<Profile>(entity)
                .map(|s| s.0.name.clone())
                .unwrap_or_default();
            lines.push(Line {
                target: Target::Profile(entity),
                glyph: Glyph::Profile,
                name: format!("{name} · harbor"),
                title: "Wants to move into the city".into(),
                since: wait.since,
                mark: Some(Standing::Waits),
                address: name,
                depth: 0,
                last: true,
                rails: Vec::new(),
                current: false,
                fold: None,
                worker: false,
            });
            counts.push(1);
            continue;
        }
        if let Some(at) =
            lines.iter().position(|line| line.address == wait.by)
        {
            counts[at] += 1;
            continue;
        }
        let line = wait.line(LINE_SHOWN);
        let said = line
            .strip_prefix(wait.shown.as_str())
            .unwrap_or(&line)
            .trim_start();
        let asker = all.iter().find(|agent| agent.address == wait.by);
        let (target, glyph, name) = match asker {
            Some(agent)
                if agent.kind == protocol::AgentKind::Hand =>
            {
                (Target::Hand, Glyph::Tower, "The Hand".to_string())
            }
            // What it goes by; its address, hamlet and all, is the
            // row's tooltip.
            Some(agent) => (
                Target::Agent(agent.entity),
                kind_glyph(agent),
                agent.called.clone(),
            ),
            // The realm's page has every wait in full.
            None => (Target::Realm, Glyph::Bell, wait.shown.clone()),
        };
        lines.push(Line {
            target,
            glyph,
            name,
            title: capitalised(said),
            since: wait.since,
            mark: Some(Standing::Waits),
            address: wait.by.clone(),
            depth: 0,
            last: true,
            rails: Vec::new(),
            current: false,
            fold: None,
            // What waits is never set lighter.
            worker: false,
        });
        counts.push(1);
    }
    for (line, count) in lines.iter_mut().zip(counts) {
        if count > 1 {
            line.title = format!("{count} waits · {}", line.title);
        }
    }
    (lines, total)
}

/// `text` with its first letter a capital, as every row's line
/// starts.
fn capitalised(text: &str) -> String {
    let mut letters = text.chars();
    letters.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(letters).collect()
    })
}

/// The city's profiles, the harbor, the library and those that left.
fn society(world: &World, sections: &mut Vec<Section>) {
    let mut city = Vec::new();
    let mut harbor = Vec::new();
    let mut gone = Vec::new();
    // A newcomer waits for the chief only while the chief holds the
    // door; under the Hand or an open door nothing waits on them.
    let chief_holds_door = world
        .get_resource::<hamlet_client::realm::Settings>()
        .is_some_and(|s| s.0.newcomers == protocol::Newcomers::Chief);
    if let Some(mut query) = world.try_query::<(Entity, &Profile)>() {
        let mut found =
            query.iter(world).collect::<Vec<(Entity, &Profile)>>();
        found.sort_by_key(|(_, profile)| {
            profile.0.name.to_lowercase()
        });
        for (entity, profile) in found {
            let profile = &profile.0;
            // Where it is hired, by hamlet.
            let mut hamlets = profile
                .hires
                .iter()
                .filter_map(|hire| {
                    hamlet_client::realm::agent(world, *hire)
                })
                .filter(|hire| world.get::<Left>(*hire).is_none())
                .filter_map(|hire| world.get::<LivesIn>(hire))
                .map(|lives| name_of(world, lives.0))
                .collect::<Vec<_>>();
            hamlets.sort();
            hamlets.dedup();
            let (title, mark) = match (&profile.left, profile.arrival)
            {
                (Some(_), _) => {
                    ("Left the city".into(), Some(Standing::Gone))
                }
                (None, protocol::Arrival::Waiting) => (
                    "Waits to be let in".into(),
                    if chief_holds_door {
                        Some(Standing::Waits)
                    } else {
                        None
                    },
                ),
                (None, protocol::Arrival::Turned) => {
                    ("Turned away".into(), Some(Standing::Turned))
                }
                (None, protocol::Arrival::Housed)
                    if hamlets.is_empty() =>
                {
                    ("Not hired".into(), None)
                }
                (None, protocol::Arrival::Housed) => {
                    (format!("In {}", hamlets.join(", ")), None)
                }
                // A blank hire's is not of the city: the hire is in
                // its hamlet's tree.
                (None, protocol::Arrival::Apart) => continue,
            };
            let line = page_line(
                world,
                Target::Profile(entity),
                Glyph::Profile,
                &profile.name,
                title,
                mark,
            );
            match (&profile.left, profile.arrival) {
                (Some(_), protocol::Arrival::Apart) => {}
                (Some(_), _) => gone.push(line),
                (None, protocol::Arrival::Housed) => city.push(line),
                (None, _) => harbor.push(line),
            }
        }
    }
    for (group, title, lines) in [
        (Group::City, "City", city),
        (Group::Harbor, "At the harbor", harbor),
    ] {
        sections.push(Section {
            group,
            title: title.into(),
            opens: None,
            mark: None,
            count: lines.len(),
            folded: false,
            lines,
            current: false,
        });
    }
    if let Some(mut query) = world.try_query::<(
        Entity,
        &Lived,
        Option<&MergedInto>,
        &Kind,
    )>() {
        let mut left = query
            .iter(world)
            .filter(|(entity, ..)| {
                world.get::<Left>(*entity).is_some()
            })
            .filter(|(_, _, _, kind)| {
                kind.0 != protocol::AgentKind::Worker
            })
            .collect::<Vec<_>>();
        left.sort_by_key(|(_, lived, ..)| {
            std::cmp::Reverse(lived.until.unwrap_or(0))
        });
        for (entity, _, into, _) in left.into_iter().take(LEFT_SHOWN)
        {
            let address = name_of(world, entity);
            let mut line = page_line(
                world,
                Target::Gone(entity),
                Glyph::Hire,
                &shown_of(world, entity),
                match into {
                    Some(host) => {
                        format!(
                            "Merged into {}",
                            shown_of(world, host.0)
                        )
                    }
                    None => "Ended".into(),
                },
                Some(if into.is_some() {
                    Standing::Merged
                } else {
                    Standing::Gone
                }),
            );
            line.address = address;
            gone.push(line);
        }
    }
    let count = gone.len();
    sections.push(Section {
        group: Group::Left,
        title: "Left".into(),
        opens: None,
        mark: None,
        count,
        folded: false,
        lines: gone,
        current: false,
    });
}

/// The rail's rows in their groups. A group with no row is left out;
/// while searching every group is open and keeps only what matches.
pub(super) fn sections(world: &World) -> Vec<Section> {
    let (search, folded, opened, hidden) =
        world.get_resource::<Rail>().map_or_else(
            || (String::new(), Vec::new(), Vec::new(), Vec::new()),
            |rail| {
                (
                    rail.search.trim().to_lowercase(),
                    rail.folded.clone(),
                    rail.opened.clone(),
                    rail.hidden.clone(),
                )
            },
        );
    // Workers are folded until opened, but a search finds them.
    let open =
        |group: Group| !search.is_empty() || opened.contains(&group);
    let tab =
        world.get_resource::<Tab>().copied().unwrap_or_default();
    let all = agents(world);
    let mut sections = Vec::new();

    // The realm, with how many wait for the chief in it, and the
    // Hand with what it started outside any hamlet.
    let waiting = world
        .try_query::<&Waiting>()
        .map_or(0, |mut waits| waits.iter(world).count());
    let mut top = vec![page_line(
        world,
        Target::Realm,
        Glyph::Globe,
        "Realm",
        match waiting {
            0 => String::new(),
            n => format!("{n} {}", need_you(n)),
        },
        (waiting > 0).then_some(Standing::Waits),
    )];
    if let Some(hand) =
        all.iter().find(|a| a.kind == protocol::AgentKind::Hand)
    {
        let outside = all
            .iter()
            .filter(|agent| agent.hamlet.is_none())
            .collect::<Vec<_>>();
        top.extend(family_lines(world, &tree(hand, &outside), &open));
    }
    // The library is the realm's, beside the Hand.
    let shelf =
        world.get_resource::<Skills>().map_or(0, |s| s.0.len());
    top.push(page_line(
        world,
        Target::Library,
        Glyph::Book,
        "Library",
        match shelf {
            1 => "1 skill on its shelves".into(),
            n => format!("{n} skills on its shelves"),
        },
        None,
    ));
    sections.push(Section {
        group: Group::Top,
        title: String::new(),
        opens: None,
        mark: None,
        count: 0,
        folded: false,
        lines: top,
        current: false,
    });

    let (needs, count) = needs(world, &all);
    sections.push(Section {
        group: Group::Needs,
        title: "Needs you".into(),
        opens: Some(Target::Realm),
        mark: None,
        count,
        folded: false,
        lines: needs,
        current: false,
    });

    // Each hamlet as the tree under its mayor.
    for (hamlet, name) in hamlet_names(world) {
        let here = all
            .iter()
            .filter(|agent| agent.hamlet == Some(hamlet))
            .collect::<Vec<_>>();
        let mut roots = here
            .iter()
            .copied()
            .filter(|agent| {
                agent.parent.is_none_or(|parent| {
                    !here.iter().any(|a| a.id == parent)
                })
            })
            .collect::<Vec<_>>();
        roots.sort_by_key(|agent| {
            (!agent.mayor, agent.called.clone())
        });
        // The workers a chief started in the hamlet answer to no one
        // in it, and fold into one row after its tree.
        let (chiefs, roots): (Vec<_>, Vec<_>) =
            roots.into_iter().partition(|agent| agent.is_worker());
        let mut family = Vec::new();
        for root in roots {
            family.extend(tree(root, &here));
        }
        if !chiefs.is_empty() {
            let group = Group::Workers(hamlet);
            family.push((
                Node::Workers {
                    group,
                    workers: chiefs,
                },
                0,
            ));
        }
        let mut lines = family_lines(world, &family, &open);
        let group = Group::Hamlet(hamlet);
        let mark = Some(Standing::of_all(
            lines.iter().filter_map(|line| line.mark),
        ));
        // A search finds rows a lead would fold away.
        if search.is_empty() {
            lines = fold_leads(lines, &folded);
        }
        sections.push(Section {
            group,
            title: name,
            opens: Some(Target::Hamlet(hamlet)),
            mark,
            count: here.len(),
            folded: folded.contains(&group)
                && mark != Some(Standing::Waits),
            lines,
            current: is_current(world, Target::Hamlet(hamlet)),
        });
    }

    society(world, &mut sections);
    for section in &mut sections {
        // Every part with a header folds, but never over a wait.
        if matches!(
            section.group,
            Group::City | Group::Harbor | Group::Left
        ) {
            section.folded = folded.contains(&section.group)
                && !section
                    .lines
                    .iter()
                    .any(|l| l.mark == Some(Standing::Waits));
        }
        if !search.is_empty() {
            section.folded = false;
            section.lines.retain(|line| matches(line, &search));
        }
        // Of those that left, the tab's own: hires on the hamlets',
        // profiles on the city's.
        if section.group == Group::Left {
            section.lines.retain(|line| {
                matches!(line.target, Target::Profile(_))
                    == (tab == Tab::City)
            });
            section.count = section.lines.len();
        }
    }
    sections.retain(|section| {
        let shown = match section.group {
            Group::Top
            | Group::Needs
            | Group::Left
            | Group::Lead(_)
            | Group::Workers(_) => true,
            Group::Hamlet(hamlet) => {
                tab == Tab::Hamlets
                    && (!hidden.contains(&hamlet)
                        || section.mark == Some(Standing::Waits))
            }
            Group::City | Group::Harbor => tab == Tab::City,
        };
        shown && !section.lines.is_empty()
    });
    sections
}

/// Every hamlet with its name, in name order.
pub(super) fn hamlet_names(world: &World) -> Vec<(Entity, String)> {
    let mut found = world
        .try_query::<(Entity, &HamletId)>()
        .map(|mut query| {
            query
                .iter(world)
                .map(|(hamlet, _)| (hamlet, name_of(world, hamlet)))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    found.sort_by_key(|(_, name)| name.to_lowercase());
    found
}
