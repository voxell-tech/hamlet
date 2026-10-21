//! The agent core's state as the protocol tells it, with no key.

use std::path::Path;

use bevy::ecs::entity::Entity;
use bevy::ecs::query::With;
use bevy::ecs::system::{Query, Res, SystemParam, SystemState};
use bevy::ecs::world::World;
use hamlet_core::agent::Event as AgentEvent;
use hamlet_core::agents::{Settlement, Treasury};
use hamlet_core::core::{self, Sandboxing, Settings as CoreSettings};
use hamlet_core::oversight::Waiting;
use hamlet_core::oversight::grants::Grants;
use hamlet_core::session::{
    self, Began, Item, Reply, SessionKey, Status as Work, TalksTo,
    Transcript, Usage, reply_in,
};
use hamlet_core::tools::{Change, change};
use hamlet_engine_api::agents::{
    Bylaws, CatalogueModel, ChangeLine, Charter, Endpoint, Entry,
    Grant, Link, Listed, Listing, Mark, Output, Priced, Provider,
    RealmLimits, Session, SessionMessage, SessionName, Settings,
    Status, Trial,
};
use hamlet_home::charter;
use hamlet_home::limits::{Limits, Period};
use hamlet_model::catalogue::{self, Catalogue};
use hamlet_model::config::providers::PROVIDERS;
use hamlet_model::config::{self, Config};
use hamlet_rules::hamlet::HOME;

use crate::Chief;

/// The home's settings with no key, the link and the listing.
pub(crate) fn settings(world: &World, home: &Path) -> Settings {
    settings_of(
        world.resource::<CoreSettings>(),
        world.resource::<core::Link>(),
        world.resource::<core::Listing>(),
        home,
    )
}

/// `settings` with no key, with `found` and `listed`, of the home at
/// `home`.
pub(crate) fn settings_of(
    settings: &CoreSettings,
    found: &core::Link,
    listed: &core::Listing,
    home: &Path,
) -> Settings {
    let config = &settings.config;
    Settings {
        model: config.model.clone(),
        reflect_every: u32::try_from(config.reflect_every)
            .unwrap_or(u32::MAX),
        allow_net: config.allow_net,
        endpoints: config.endpoints.iter().map(endpoint).collect(),
        chosen: Config::saved(home)
            || std::env::var_os("HAMLET_MODEL").is_some(),
        problem: settings.error.clone(),
        link: link(found),
        listing: listing(listed),
    }
}

/// An endpoint with only whether it has a key.
pub(crate) fn endpoint(endpoint: &config::Endpoint) -> Endpoint {
    Endpoint {
        name: endpoint.name.clone(),
        url: endpoint.base_url.clone(),
        has_key: endpoint.api_key.is_some(),
        chatgpt: endpoint.chatgpt,
    }
}

pub(crate) fn link(link: &core::Link) -> Link {
    match link {
        core::Link::Unknown => Link::Untested,
        core::Link::Testing => Link::Testing,
        core::Link::Ok { models } => Link::Connected(models.clone()),
        core::Link::Failed(why) => Link::Failed(why.clone()),
    }
}

pub(crate) fn listing(listing: &core::Listing) -> Listing {
    Listing {
        asking: u32::try_from(listing.asking).unwrap_or(u32::MAX),
        said: listing
            .said
            .iter()
            .map(|(endpoint, said)| Listed {
                endpoint: endpoint.clone(),
                models: said.clone().map(|count| {
                    u32::try_from(count).unwrap_or(u32::MAX)
                }),
            })
            .collect(),
    }
}

pub(crate) fn trial(trial: &core::Trial) -> Trial {
    match trial {
        core::Trial::Untried => Trial::Untried,
        core::Trial::Trying => Trial::Trying,
        core::Trial::Answered => Trial::Answered,
        core::Trial::Failed(why) => Trial::Failed(why.clone()),
    }
}

pub(crate) fn catalogue(all: &Catalogue) -> Vec<CatalogueModel> {
    all.models
        .iter()
        .map(|model| CatalogueModel {
            name: model.name(),
            allowed: model.allowed,
            input: model.input,
            output: model.output,
            priced: match model.priced {
                catalogue::Priced::Unknown => Priced::Unknown,
                catalogue::Priced::Listed => Priced::Listed,
                catalogue::Priced::Assigned => Priced::Assigned,
                catalogue::Priced::Yours => Priced::Yours,
            },
            context: model.context,
            note: model.note.clone(),
        })
        .collect()
}

pub(crate) fn providers() -> Vec<Provider> {
    PROVIDERS
        .iter()
        .map(|provider| Provider {
            name: provider.name.into(),
            endpoint: provider.endpoint.into(),
            url: provider.base_url.into(),
            keys_at: provider.keys.into(),
        })
        .collect()
}

pub(crate) fn charter(home: &Path) -> Charter {
    let own = charter::own(home);
    Charter {
        own: own.is_some(),
        text: own.unwrap_or_else(|| charter::DEFAULT.to_string()),
    }
}

/// The limits of the realm from the home's file, or the defaults and
/// why it cannot be read.
pub(crate) fn limits(home: &Path) -> RealmLimits {
    let (limits, problem) = match Limits::load(home) {
        Ok(limits) => (limits, None),
        Err(why) => (Limits::default(), Some(format!("{why:#}"))),
    };
    let wide = |n: usize| n as u64;
    RealmLimits {
        most_hamlets: wide(limits.hamlets.most),
        roads_between: limits.hamlets.roads_between,
        removal_mayors: wide(limits.hamlets.removal_mayors),
        grows_over_days: limits.road.grows_over_days,
        pass: limits.votes.pass,
        least_voters: wide(limits.votes.least_voters),
        sealed: limits.votes.sealed,
        grounded_only: limits.votes.grounded_only,
        ballot_minutes: limits.votes.ballot_minutes,
        open_minutes: limits.votes.open_minutes,
        retry_minutes: limits.votes.retry_minutes,
        problem,
    }
}

/// The limits of the hamlet at `root` that are its own, or the
/// defaults and why its file cannot be read.
pub(crate) fn bylaws(root: &Path) -> Bylaws {
    let (limits, problem) = match Limits::load(root) {
        Ok(limits) => (limits, None),
        Err(why) => (Limits::default(), Some(format!("{why:#}"))),
    };
    let wide = |n: usize| n as u64;
    Bylaws {
        monthly: limits.budget.period == Period::Month,
        budget_cost: limits.budget.cost,
        task_cost: limits.task.cost,
        task_messages: limits.task.messages,
        road_cost: limits.road.cost,
        create_agents: limits.mayor.create_agents,
        remove_agents: limits.mayor.remove_agents,
        merge_agents: limits.mayor.merge_agents,
        build_roads: limits.mayor.build_roads,
        told_days: limits.mayor.told_days,
        most_agents: wide(limits.most_agents),
        most_roads: wide(limits.most_roads),
        models: limits.models,
        model: limits.model,
        problem,
    }
}

/// Whether `key` of `section` is a limit of the realm, which the
/// home's file holds for every hamlet.
pub(crate) fn of_the_realm(section: &str, key: &str) -> bool {
    matches!(section, "votes" | "hamlets")
        || (section, key) == ("road", "grows_over_days")
}

/// Why `value` cannot be written at `key` of `section` of a limits
/// file for a plugin: a section or key that is not one plain word,
/// a value of more than one line, a table where the root's values
/// are, or the lasting permissions, which only the chief gives.
pub(crate) fn unwritable(
    section: &str,
    key: &str,
    value: Option<&str>,
) -> Option<String> {
    let word = |name: &str| {
        name.chars().all(|c| {
            c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'
        })
    };
    let value = value.unwrap_or_default();
    if key.is_empty() || !word(key) || !word(section) {
        Some(format!("{section}.{key} is not a limit"))
    } else if value.contains(['\n', '\r']) {
        Some("a value is one line".into())
    } else if section.is_empty()
        && value.trim_start().starts_with('{')
    {
        Some(format!("{key} is a section"))
    } else if section == "permissions"
        || (section.is_empty() && key == "permissions")
    {
        Some("the lasting permissions are the chief's to give".into())
    } else {
        None
    }
}

pub(crate) fn grants(grants: &Grants) -> Vec<Grant> {
    grants
        .held()
        .iter()
        .map(|grant| Grant {
            agent: grant.agent,
            name: grant.name.clone(),
            permission: grant.permission.name().into(),
            by: grant.by.clone(),
        })
        .collect()
}

/// What the status line is made of.
#[derive(SystemParam)]
pub(crate) struct StatusFacts<'w, 's> {
    sandbox: Res<'w, Sandboxing>,
    settings: Res<'w, CoreSettings>,
    link: Res<'w, core::Link>,
    chief: Res<'w, Chief>,
    sessions: Query<'w, 's, (&'static Usage, &'static Work)>,
    treasuries:
        Query<'w, 's, (&'static Settlement, &'static Treasury)>,
    waiting: Query<'w, 's, (), With<Waiting>>,
}

impl StatusFacts<'_, '_> {
    /// The facts of the status line, of the chief's open session.
    pub(crate) fn status(&self) -> Status {
        let treasury = self
            .treasuries
            .iter()
            .find(|(settlement, _)| settlement.name == HOME)
            .map(|(_, treasury)| treasury.0.summary())
            .unwrap_or_default();
        let open = self.sessions.get(self.chief.session).ok();
        Status {
            sandbox: self.sandbox.0.name().into(),
            model: self.settings.config.model.clone(),
            link: link(&self.link),
            usage: open
                .map(|(usage, _)| usage.summary())
                .unwrap_or_default(),
            treasury,
            waiting: u32::try_from(self.waiting.iter().count())
                .unwrap_or(u32::MAX),
            working: open.is_some_and(|(_, work)| work.working()),
        }
    }
}

/// The status as it stands in `world`.
pub(crate) fn status(world: &mut World) -> Option<Status> {
    let mut facts = SystemState::<StatusFacts>::new(world);
    facts.get(world).ok().map(|facts| facts.status())
}

pub(crate) fn session_name(key: &SessionKey) -> SessionName {
    SessionName {
        channel: key.channel.clone(),
        chat: key.chat.clone(),
        thread: key.thread.clone(),
    }
}

pub(crate) fn session_key(name: &SessionName) -> SessionKey {
    SessionKey {
        channel: name.channel.clone(),
        chat: name.chat.clone(),
        thread: name.thread.clone(),
    }
}

/// The session `entity`, if it is one.
pub(crate) fn session(
    world: &World,
    entity: Entity,
) -> Option<Session> {
    let session = world.get_entity(entity).ok()?;
    let key = session.get::<SessionKey>()?;
    let agent = session
        .get::<TalksTo>()
        .map(|with| hamlet_core::agents::name_of(world, with.0));
    let said = session.get::<Transcript>();
    Some(Session {
        name: session_name(key),
        agent,
        working: session.get::<Work>().is_some_and(Work::working),
        usage: session
            .get::<Usage>()
            .map(Usage::summary)
            .unwrap_or_default(),
        last: said
            .and_then(|said| said.items().last())
            .map(|item| item.id),
        began: session.get::<Began>().map_or(0, |began| began.0),
        answering: said.is_some_and(Transcript::answering),
    })
}

pub(crate) fn sessions(world: &mut World) -> Vec<Session> {
    let all = world
        .query_filtered::<Entity, With<SessionKey>>()
        .iter(world)
        .collect::<Vec<_>>();
    all.into_iter()
        .filter_map(|entity| session(world, entity))
        .collect()
}

/// The last `most` messages of `said` after the message `after`.
pub(crate) fn messages(
    said: &Transcript,
    after: Option<u64>,
    most: u32,
) -> Vec<SessionMessage> {
    let items = said
        .items()
        .iter()
        .filter(|item| after.is_none_or(|after| item.id > after))
        .collect::<Vec<_>>();
    let skip = items.len().saturating_sub(most as usize);
    items[skip..]
        .iter()
        .map(|item| message(said, item))
        .collect()
}

/// The message `item` of `said`.
pub(crate) fn message(
    said: &Transcript,
    item: &Item,
) -> SessionMessage {
    SessionMessage {
        id: item.id,
        at: item.at,
        entry: entry(&item.entry),
        another: said.of_another(item.id),
        reply: matches!(&item.entry, session::Entry::User { user, text }
            if reply_in(user, text).is_some()),
    }
}

/// The lines of a change a message carries at most.
const CHANGE_LINES: usize = 60;

pub(crate) fn change(
    change: &Change,
) -> hamlet_engine_api::agents::Change {
    let wide = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
    hamlet_engine_api::agents::Change {
        path: change.path.clone(),
        created: change.created,
        lines: change
            .lines
            .iter()
            .take(CHANGE_LINES)
            .map(|line| ChangeLine {
                mark: match line.mark {
                    change::Mark::Same => Mark::Same,
                    change::Mark::Out => Mark::Out,
                    change::Mark::In => Mark::In,
                },
                text: line.text.clone(),
            })
            .collect(),
        total: wide(change.total),
        added: wide(change.added),
        removed: wide(change.removed),
        failure: change.failure.clone(),
    }
}

/// The shell command a tool's arguments run, or the arguments.
fn command(summary: &str) -> String {
    serde_json::from_str::<serde_json::Value>(summary)
        .ok()
        .and_then(|args| args["command"].as_str().map(str::to_string))
        .unwrap_or_else(|| summary.to_string())
}

pub(crate) fn entry(entry: &session::Entry) -> Entry {
    match entry {
        session::Entry::User { user, text } => Entry::User {
            user: user.clone(),
            text: text.clone(),
        },
        session::Entry::Assistant(text) => {
            Entry::Assistant(text.clone())
        }
        session::Entry::Reasoning(text) => {
            Entry::Reasoning(text.clone())
        }
        session::Entry::Tool {
            name,
            summary,
            exit,
            change: asked,
        } => Entry::Tool {
            name: name.clone(),
            summary: summary.clone(),
            command: command(summary),
            exit: *exit,
            change: asked.as_ref().map(change),
        },
        session::Entry::Error(text) => Entry::Error(text.clone()),
        session::Entry::Notice(text) => Entry::Notice(text.clone()),
        session::Entry::Answered(text) => {
            Entry::Answered(text.clone())
        }
        session::Entry::Agent { name, text } => Entry::Agent {
            name: name.clone(),
            text: text.clone(),
        },
    }
}

/// What a reply of a working session tells a watcher. Nothing for
/// what only the core follows.
pub(crate) fn output(reply: &Reply) -> Option<Output> {
    Some(match reply {
        Reply::Event(AgentEvent::Text(text)) => {
            Output::Text(text.clone())
        }
        Reply::Event(AgentEvent::Reasoning(text)) => {
            Output::Reasoning(text.clone())
        }
        Reply::Event(AgentEvent::Tool { name, summary, .. }) => {
            Output::Tool {
                name: name.clone(),
                summary: summary.clone(),
            }
        }
        Reply::Event(AgentEvent::ToolDone {
            name,
            exit_code,
            failure,
            ..
        }) => Output::ToolDone {
            name: name.clone(),
            exit: *exit_code,
            failure: failure.clone(),
        },
        Reply::Event(AgentEvent::Notice(text))
        | Reply::Notice(text) => Output::Notice(text.clone()),
        Reply::Event(AgentEvent::Agent { name, .. }) => {
            Output::Agent(name.clone())
        }
        Reply::Done(text) => Output::Done(text.clone()),
        Reply::Error(text) => Output::Error(text.clone()),
        Reply::Event(_) => return None,
    })
}
