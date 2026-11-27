//! The rows of the rail and the agents they are built from.

use bevy::ecs::entity::Entity;
use bevy::ecs::query::Has;
use bevy::ecs::world::World;
use hamlet_client::chats::{Entry, Transcript};
use hamlet_client::realm::{
    Activity, Busy, Identity, Kind, Left, Lived, LivesIn, Mayor,
    Parent, Purpose, Shown, name_of,
};
use hamlet_protocol::client as protocol;
use hamlet_realm::pick::Thing;
use hamlet_ui::widgets::ellipsis::cut;
use hamlet_ui::widgets::icons::Glyph;

use super::super::Showing;
use super::{DEEPEST, LINE_SHOWN};
use crate::age::Age;
use crate::convo::Convo;
use crate::standing::Standing;

/// A part of the rail.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Group {
    /// The realm and the Hand, with the Hand's own workers.
    Top,
    Needs,
    Hamlet(Entity),
    /// The hires under a lead, which fold into its row.
    Lead(Entity),
    /// The workers an agent started, or those a chief started in a
    /// hamlet, which fold into one row.
    Workers(Entity),
    City,
    /// Newcomers waiting to be let in, and those turned away.
    Harbor,
    Left,
}

/// What a row or a header leads to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Target {
    Realm,
    Hand,
    Agent(Entity),
    Hamlet(Entity),
    Profile(Entity),
    Library,
    Gone(Entity),
    /// The group it is folded or opened.
    Fold(Group),
}

impl Target {
    /// The thing whose menu a right click on its row opens, as in the
    /// world.
    pub(super) fn thing(self) -> Option<Thing> {
        match self {
            Target::Hand => Some(Thing::Hand),
            Target::Agent(agent) => Some(Thing::Agent(agent)),
            Target::Hamlet(hamlet) => Some(Thing::Common(hamlet)),
            Target::Profile(profile) => Some(Thing::Profile(profile)),
            Target::Library => Some(Thing::Library),
            Target::Gone(agent) => Some(Thing::Ruin(agent)),
            Target::Realm | Target::Fold(_) => None,
        }
    }
}

/// A row of the rail.
#[derive(Clone, PartialEq, Debug)]
pub(super) struct Line {
    pub target: Target,
    pub glyph: Glyph,
    pub name: String,
    pub title: String,
    /// When what its age counts from happened, in milliseconds; 0
    /// for none. The age is worked out as it is drawn, so the rows
    /// are not built anew each minute.
    pub since: u64,
    pub mark: Option<Standing>,
    /// What the search also finds it by: its address.
    pub address: String,
    /// How far under another agent it is set in.
    pub depth: u8,
    /// Whether no row after it is set in under the same agent.
    pub last: bool,
    /// For each level between the top and its own, whether the guide
    /// of the agent there carries on past this row.
    pub rails: Vec<bool>,
    pub current: bool,
    /// For a lead: the fold of the rows under it, and whether it is
    /// folded.
    pub fold: Option<(Group, bool)>,
    /// Whether it is a worker's, or the row its group folds into:
    /// one short line, lighter than a hire's, so the hires stand out
    /// of their workers.
    pub worker: bool,
}

/// A group, its header and its rows.
#[derive(Clone, PartialEq, Debug)]
pub(super) struct Section {
    pub group: Group,
    pub title: String,
    /// Where pressing its title leads, if anywhere.
    pub opens: Option<Target>,
    /// The state of all of it, shown on its header.
    pub mark: Option<Standing>,
    pub count: usize,
    pub folded: bool,
    pub lines: Vec<Line>,
    /// Whether the page its title opens is the one shown.
    pub current: bool,
}

/// An agent as the rail reads it.
pub(super) struct Agent {
    pub(super) entity: Entity,
    pub(super) id: protocol::AgentId,
    pub(super) kind: protocol::AgentKind,
    pub(super) parent: Option<protocol::AgentId>,
    pub(super) hamlet: Option<Entity>,
    pub(super) mayor: bool,
    pub(super) mark: Option<Standing>,
    /// Its address, and what it goes by: its nickname, or its
    /// profile's name.
    pub(super) address: String,
    pub(super) called: String,
    pub(super) title: String,
    pub(super) since: u64,
}

/// How long ago `then` was at `now`, short, or nothing for a time
/// never set.
pub(super) fn age(now: u64, then: u64) -> String {
    if then == 0 {
        return String::new();
    }
    Age::of(now, then).short()
}

/// The first line of `text` that has words, cut at `most` letters.
fn first_line(text: &str, most: usize) -> String {
    let line = text.lines().find(|line| !line.trim().is_empty());
    cut(line.unwrap_or(""), most)
}

/// The last thing the chief said to the agent, and when the chat
/// was last added to.
pub(super) fn thread(
    world: &World,
    id: protocol::AgentId,
) -> (Option<String>, u64) {
    let reference = protocol::ChatRef {
        kind: None,
        from: None,
        with: id,
        past: None,
    };
    let Some(chat) = hamlet_client::chats::find(world, &reference)
    else {
        return (None, 0);
    };
    let Some(transcript) = world.get::<Transcript>(chat) else {
        return (None, 0);
    };
    let items = transcript.items();
    let said =
        items.iter().rev().find_map(|item| match &item.entry {
            Entry::Chief { text, .. } => {
                let (_, own) =
                    crate::screens::chat::reply::own_words(text);
                Some(first_line(&own, LINE_SHOWN))
            }
            _ => None,
        });
    (said, items.last().map_or(0, |item| item.at))
}

/// What an agent with `address` is called in the rail: what it is
/// shown by, else its address.
pub(super) fn called(address: &str, shown: Option<&Shown>) -> String {
    shown.map_or_else(|| address.to_string(), |shown| shown.0.clone())
}

/// What an agent's row says under its name: its work while it works,
/// else the chief's last words to it, else what it was asked, and
/// nothing where its mark already says the same. Then when its chat
/// was last added to.
pub(crate) fn under_name(
    world: &World,
    agent: Entity,
) -> (String, u64) {
    let (said, last) = world
        .get::<Identity>(agent)
        .map_or((None, 0), |id| thread(world, id.0));
    let words = world
        .get::<Activity>(agent)
        .map(|activity| activity.words.clone())
        .unwrap_or_default();
    let title = world
        .get::<Busy>(agent)
        .map(|_| words.clone())
        .or(said.map(|said| format!("You: {said}")))
        .or_else(|| {
            world
                .get::<Purpose>(agent)
                .map(|p| first_line(&p.0, LINE_SHOWN))
                .filter(|line| !line.is_empty())
        })
        .unwrap_or(words);
    if title.eq_ignore_ascii_case(Standing::of(world, agent).word()) {
        (String::new(), last)
    } else {
        (title, last)
    }
}

pub(super) fn agents(world: &World) -> Vec<Agent> {
    let Some(mut query) = world.try_query::<(
        Entity,
        &Identity,
        &Kind,
        Option<&Parent>,
        Option<&LivesIn>,
        &Lived,
        Option<&Shown>,
        (Has<Mayor>, Has<Left>),
    )>() else {
        return Vec::new();
    };
    query
        .iter(world)
        .filter(|(.., (_, left))| !left)
        .map(
            |(
                entity,
                id,
                kind,
                parent,
                lives,
                lived,
                shown,
                (mayor, _),
            )| {
                let address = name_of(world, entity);
                let (title, last) = under_name(world, entity);
                Agent {
                    entity,
                    id: id.0,
                    kind: kind.0,
                    parent: parent.map(|parent| parent.0),
                    hamlet: lives.map(|lives| lives.0),
                    mayor,
                    mark: Some(Standing::of(world, entity)),
                    called: called(&address, shown),
                    address,
                    title,
                    since: last.max(lived.since),
                }
            },
        )
        .collect()
}

/// A place in a tree: an agent, or the workers one started, which
/// fold into a row of their own under the hires beside them.
pub(super) enum Node<'a> {
    Agent(&'a Agent),
    Workers {
        group: Group,
        workers: Vec<&'a Agent>,
    },
}

impl Agent {
    pub(super) fn is_worker(&self) -> bool {
        self.kind == protocol::AgentKind::Worker
    }
}

/// `root` and everyone under it among `all`, each with its depth
/// below `root`: hires by name, then the workers each agent started
/// in a group under it.
pub(super) fn tree<'a>(
    root: &'a Agent,
    all: &[&'a Agent],
) -> Vec<(Node<'a>, u8)> {
    fn walk<'a>(
        agent: &'a Agent,
        all: &[&'a Agent],
        depth: u8,
        seen: &mut Vec<protocol::AgentId>,
        out: &mut Vec<(Node<'a>, u8)>,
    ) {
        if seen.contains(&agent.id) {
            return;
        }
        seen.push(agent.id);
        out.push((Node::Agent(agent), depth));
        let mut under = all
            .iter()
            .copied()
            .filter(|other| other.parent == Some(agent.id))
            .collect::<Vec<_>>();
        under.sort_by_key(|other| other.called.to_lowercase());
        let (workers, hires): (Vec<_>, Vec<_>) =
            under.into_iter().partition(|other| other.is_worker());
        let depth = depth.saturating_add(1);
        for child in hires {
            walk(child, all, depth, seen, out);
        }
        if !workers.is_empty() {
            let group = Group::Workers(agent.entity);
            out.push((Node::Workers { group, workers }, depth));
        }
    }
    let mut out = Vec::new();
    walk(root, all, 0, &mut Vec::new(), &mut out);
    out
}

/// The rows of `family`, each set in as deep as it is in the tree,
/// knowing whether it is the last under the agent above it and which
/// guides above it carry on past it. A group of workers is one row,
/// with a row for each of them under it while `open` says it is.
pub(super) fn family_lines(
    world: &World,
    family: &[(Node<'_>, u8)],
    open: &dyn Fn(Group) -> bool,
) -> Vec<Line> {
    // Each row with the depth it has in the tree.
    let mut rows = Vec::<(Line, u8)>::new();
    for (node, depth) in family {
        let depth = *depth;
        match node {
            Node::Agent(agent) => {
                let line = agent_line(world, agent, depth, false);
                rows.push((line, depth));
            }
            Node::Workers { group, workers } => {
                let shown = open(*group);
                rows.push((
                    workers_line(*group, workers, shown, depth),
                    depth,
                ));
                if shown {
                    let under = depth.saturating_add(1);
                    for worker in workers {
                        let line =
                            agent_line(world, worker, under, false);
                        rows.push((line, under));
                    }
                }
            }
        }
    }
    let depths =
        rows.iter().map(|(_, depth)| *depth).collect::<Vec<_>>();
    // Whether the agent last seen at each depth has more after it.
    let mut more = Vec::<bool>::new();
    rows.into_iter()
        .enumerate()
        .map(|(at, (mut line, depth))| {
            let last = depths.get(at + 1..).is_none_or(|after| {
                after
                    .iter()
                    .take_while(|d| **d >= depth)
                    .all(|d| *d > depth)
            });
            let depth = depth.min(DEEPEST);
            let level = usize::from(depth);
            line.depth = depth;
            line.last = last;
            // The guides of the levels between the top and its own.
            line.rails =
                more.get(1..level).unwrap_or_default().to_vec();
            more.resize(level + 1, false);
            more[level] = !last;
            line
        })
        .collect()
}

/// The row a group of `workers` folds into: how many there are, and
/// while it is folded the most pressing of their marks.
fn workers_line(
    group: Group,
    workers: &[&Agent],
    open: bool,
    depth: u8,
) -> Line {
    let count = workers.len();
    Line {
        target: Target::Fold(group),
        glyph: Glyph::Worker,
        name: match count {
            1 => "1 worker".into(),
            n => format!("{n} workers"),
        },
        title: String::new(),
        since: 0,
        mark: (!open).then(|| {
            Standing::of_all(workers.iter().filter_map(|w| w.mark))
        }),
        address: workers
            .iter()
            .map(|worker| worker.address.as_str())
            .collect::<Vec<_>>()
            .join(" "),
        depth,
        last: true,
        rails: Vec::new(),
        current: false,
        fold: Some((group, !open)),
        worker: true,
    }
}

pub(super) fn kind_glyph(agent: &Agent) -> Glyph {
    match agent.kind {
        protocol::AgentKind::Hand => Glyph::Tower,
        protocol::AgentKind::Hire if agent.mayor => Glyph::Mayor,
        protocol::AgentKind::Hire => Glyph::Hire,
        protocol::AgentKind::Worker => Glyph::Worker,
    }
}

/// Whether the Hub is turned to `target`.
pub(super) fn is_current(world: &World, target: Target) -> bool {
    let showing = world.get_resource::<Showing>().copied();
    let seated = world.get_resource::<Convo>().map(|c| c.agent);
    match (target, showing) {
        (
            Target::Hand | Target::Agent(_),
            Some(
                Showing::Thread | Showing::Sessions | Showing::Prompt,
            ),
        ) => match target {
            Target::Agent(agent) => seated == Some(agent),
            _ => seated.is_some_and(|agent| {
                world.get::<Kind>(agent).map(|kind| kind.0)
                    == Some(protocol::AgentKind::Hand)
            }),
        },
        (Target::Profile(a), Some(Showing::Profile(b)))
        | (Target::Gone(a), Some(Showing::Gone(b)))
        | (Target::Hamlet(a), Some(Showing::Hamlet(b))) => a == b,
        (Target::Library, Some(Showing::Library))
        | (Target::Realm, Some(Showing::Realm)) => true,
        _ => false,
    }
}

fn agent_line(
    world: &World,
    agent: &Agent,
    depth: u8,
    last: bool,
) -> Line {
    let hand = agent.kind == protocol::AgentKind::Hand;
    let target = if hand {
        Target::Hand
    } else {
        Target::Agent(agent.entity)
    };
    Line {
        target,
        glyph: kind_glyph(agent),
        name: if hand {
            "The Hand".into()
        } else {
            agent.called.clone()
        },
        title: agent.title.clone(),
        since: agent.since,
        mark: agent.mark,
        address: agent.address.clone(),
        depth,
        last,
        rails: Vec::new(),
        current: is_current(world, target),
        fold: None,
        worker: agent.kind == protocol::AgentKind::Worker,
    }
}

/// Lets each lead in `lines` stand for the rows set in under it:
/// while folded they are left out and its mark becomes the most
/// pressing of theirs. What waits under it is still in the rows of
/// what needs the chief.
pub(super) fn fold_leads(
    lines: Vec<Line>,
    folded: &[Group],
) -> Vec<Line> {
    let mut lines = lines;
    for at in 0..lines.len() {
        let depth = lines[at].depth;
        let under = lines[at + 1..]
            .iter()
            .take_while(|line| line.depth > depth)
            .count();
        let Target::Agent(agent) = lines[at].target else {
            continue;
        };
        if under == 0 {
            continue;
        }
        let range = &lines[at + 1..at + 1 + under];
        let group = Group::Lead(agent);
        let shut = folded.contains(&group);
        // Open, each row shows its own state; folded, the lead
        // stands for all of them.
        let mark = Some(Standing::of_all(
            std::iter::once(lines[at].mark)
                .chain(range.iter().map(|line| line.mark))
                .flatten(),
        ));
        let line = &mut lines[at];
        line.fold = Some((group, shut));
        if shut {
            line.mark = mark;
        }
    }
    // Leave out what folded leads hold.
    let mut kept = Vec::with_capacity(lines.len());
    let mut hidden_below: Option<u8> = None;
    for line in lines {
        if let Some(depth) = hidden_below {
            if line.depth > depth {
                continue;
            }
            hidden_below = None;
        }
        if matches!(line.fold, Some((_, true))) {
            hidden_below = Some(line.depth);
        }
        kept.push(line);
    }
    kept
}

/// A row that leads to a page.
pub(super) fn page_line(
    world: &World,
    target: Target,
    glyph: Glyph,
    name: &str,
    title: String,
    mark: Option<Standing>,
) -> Line {
    Line {
        target,
        glyph,
        name: name.into(),
        title,
        since: 0,
        mark,
        address: String::new(),
        depth: 0,
        last: true,
        rails: Vec::new(),
        current: is_current(world, target),
        fold: None,
        worker: false,
    }
}

/// Whether `line` matches what is typed in the search.
pub(super) fn matches(line: &Line, search: &str) -> bool {
    search.is_empty()
        || line.name.to_lowercase().contains(search)
        || line.address.to_lowercase().contains(search)
        || line.title.to_lowercase().contains(search)
}
