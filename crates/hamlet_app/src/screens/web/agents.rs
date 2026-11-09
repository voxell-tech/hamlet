//! The agents' pages: each agent's own Web tab, which the server asks
//! this window to act on for the agent's web tools (ADR 0015).

use std::collections::BTreeMap;
use std::time::Duration;

use bevy::app::{App, Update};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::observer::On;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Local, Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::time::{Real, Time};
use bevy::window::{PrimaryWindow, Window};
use bevy_cef::prelude::{
    ConsoleLevel, ConsoleMessage, LoadError, LoadFinished,
    LoadingStateChanged, WebviewTitle,
};
use bevy_fynix::dock::{DockTree, DropTarget, Edge};
use hamlet_client::connection::Contact;
use hamlet_client::outbox::Outbox;
use hamlet_client::realm::{self, RealmFolder};
use hamlet_client::{Connection, WebAsked};
use hamlet_protocol::client::{
    self as protocol, ClientMessage, RequestId, WebAct, WebAnswer,
    WebWindow, is_on_this_machine,
};
use serde_json::json;

use super::page::{self, Page};
use super::panel::{PanelId, is_panel};
use super::{Addresses, AgentTabs, PANEL, bridge};
use crate::dock::open_numbered;

/// How long a page is waited for before the agent is told it is still
/// loading: short of the server's own wait for the answer.
const LOAD_WITHIN: Duration = Duration::from_secs(25);

/// The error Chromium gives a load another load replaced.
const ABORTED: i32 = -3;

/// The most console lines kept of a page, the newest.
const CONSOLE_MOST: usize = 200;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Asks>()
        .init_resource::<Waits>()
        .init_resource::<AgentNames>()
        .init_resource::<Consoles>()
        .init_resource::<Nets>()
        .add_observer(wrote)
        .add_observer(loading)
        .add_observer(asked)
        .add_observer(loaded)
        .add_observer(failed)
        .add_systems(
            Update,
            (
                announce,
                (act, give_up, close_gone, name_agents)
                    .chain()
                    .before(page::Opening),
            ),
        );
}

/// The asks received and not yet carried out.
#[derive(Resource, Default)]
struct Asks(Vec<protocol::WebAsk>);

/// The asks waiting on their pages, by panel id.
#[derive(Resource, Default)]
pub(super) struct Waits(BTreeMap<String, Wait>);

impl Waits {
    /// Whether `panel`'s page is waited on.
    pub(super) fn has(&self, panel: &str) -> bool {
        self.0.contains_key(panel)
    }
}

/// An ask waiting for its page to load.
struct Wait {
    id: RequestId,
    net: bool,
    until: Duration,
}

/// Whether each agent's page may be on another machine, as its
/// agent's last ask said, by panel id.
#[derive(Resource, Default)]
struct Nets(BTreeMap<String, bool>);

/// Whether a page is loading, as the engine last said.
#[derive(Component)]
pub(super) struct Loading(pub bool);

/// Keeps whether each page is loading.
fn loading(
    changed: On<LoadingStateChanged>,
    mut commands: bevy::ecs::system::Commands,
) {
    commands
        .entity(changed.webview)
        .try_insert(Loading(changed.is_loading));
}

/// What each agent's page wrote to its console since its agent last
/// looked, by panel id.
#[derive(Resource, Default)]
struct Consoles(BTreeMap<String, Vec<String>>);

impl Consoles {
    fn keep(&mut self, panel: &str, line: String) {
        let lines = self.0.entry(panel.to_owned()).or_default();
        lines.push(line);
        if lines.len() > CONSOLE_MOST {
            let over = lines.len() - CONSOLE_MOST;
            lines.drain(..over);
        }
    }
}

/// Keeps what an agent's page wrote to its console.
fn wrote(
    message: On<ConsoleMessage>,
    pages: Query<&Page>,
    agents: Res<AgentTabs>,
    mut consoles: ResMut<Consoles>,
) {
    let Ok(Page(panel)) = pages.get(message.webview) else {
        return;
    };
    if !agents.0.contains_key(&panel.0) {
        return;
    }
    let level = match message.level {
        ConsoleLevel::Error => "error",
        ConsoleLevel::Warning => "warning",
        ConsoleLevel::Info => "info",
        ConsoleLevel::Verbose => "debug",
    };
    let from = if message.source.is_empty() {
        String::new()
    } else {
        format!(" ({}:{})", message.source, message.line)
    };
    consoles.keep(
        &panel.0,
        format!("[{level}] {}{from}", message.message),
    );
}

fn asked(ask: On<WebAsked>, mut asks: ResMut<Asks>) {
    asks.0.push(ask.0.clone());
}

/// Tells the server this window draws Web panels whenever it connects
/// and whenever it comes to the front.
fn announce(
    connection: Res<Connection>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut outbox: ResMut<Outbox>,
    mut was: Local<(bool, bool)>,
) {
    let connected = connection.state == Contact::Connected;
    let front = window.iter().next().is_some_and(|w| w.focused);
    let (was_connected, was_front) = *was;
    if connected && (!was_connected || (front && !was_front)) {
        outbox.push(ClientMessage::WebWindow(WebWindow {}));
    }
    *was = (connected, front);
}

/// Sends the answer `text` to the ask `id`.
pub(super) fn answer(
    world: &mut World,
    id: RequestId,
    text: String,
    failed: bool,
) {
    world.resource_mut::<Outbox>().push(ClientMessage::Web(
        WebAnswer {
            to: id,
            text,
            failed,
        },
    ));
}

/// Carries out the asks received.
fn act(world: &mut World) {
    let asks = std::mem::take(&mut world.resource_mut::<Asks>().0);
    for ask in asks {
        let Some(panel) = tab_of(world, ask.agent.0) else {
            answer(
                world,
                ask.id,
                "The window found no place for your Web tab.".into(),
                true,
            );
            continue;
        };
        page::wake(world, &panel);
        world
            .resource_mut::<Nets>()
            .0
            .insert(panel.0.clone(), ask.net);
        match ask.act {
            WebAct::Open { address } => {
                world
                    .resource_mut::<Addresses>()
                    .0
                    .insert(panel.0.clone(), address.clone());
                page::navigate(world, &panel, address);
                let until = world.resource::<Time<Real>>().elapsed()
                    + LOAD_WITHIN;
                let wait = Wait {
                    id: ask.id,
                    net: ask.net,
                    until,
                };
                if let Some(old) = world
                    .resource_mut::<Waits>()
                    .0
                    .insert(panel.0, wait)
                {
                    answer(
                        world,
                        old.id,
                        "Another page was opened before this one \
                         loaded."
                            .into(),
                        true,
                    );
                }
            }
            WebAct::Wait {
                text: None,
                selector: None,
                seconds,
            } => {
                let page = page::of(world, &panel);
                let loading = page
                    .and_then(|page| world.get::<Loading>(page))
                    .is_some_and(|loading| loading.0);
                if page.is_none() {
                    on_page(
                        world,
                        &panel,
                        ask.id,
                        "wait",
                        json!({}),
                        Duration::ZERO,
                    );
                } else if loading {
                    let until =
                        world.resource::<Time<Real>>().elapsed()
                            + Duration::from_secs(u64::from(seconds));
                    let wait = Wait {
                        id: ask.id,
                        net: ask.net,
                        until,
                    };
                    world
                        .resource_mut::<Waits>()
                        .0
                        .insert(panel.0, wait);
                } else {
                    answer(
                        world,
                        ask.id,
                        "The page has finished loading.".into(),
                        false,
                    );
                }
            }
            WebAct::Wait {
                text,
                selector,
                seconds,
            } => {
                on_page(
                    world,
                    &panel,
                    ask.id,
                    "wait",
                    json!({"text": text, "selector": selector, "seconds": seconds}),
                    Duration::from_secs(u64::from(seconds)),
                );
            }
            WebAct::Click { target } => {
                on_page(
                    world,
                    &panel,
                    ask.id,
                    "click",
                    json!({"target": target}),
                    Duration::ZERO,
                );
            }
            WebAct::Console {} => {
                let lines = world
                    .resource_mut::<Consoles>()
                    .0
                    .remove(&panel.0)
                    .unwrap_or_default();
                let text = if lines.is_empty() {
                    "Nothing was written to the console since you \
                     last looked."
                        .to_owned()
                } else {
                    lines.join("\n")
                };
                answer(world, ask.id, text, false);
            }
            WebAct::Read { html } => {
                on_page(
                    world,
                    &panel,
                    ask.id,
                    "read",
                    json!({"html": html}),
                    Duration::ZERO,
                );
            }
        }
    }
}

/// Asks the page of `panel` to carry out `act`, or answers that the
/// tab has none yet.
fn on_page(
    world: &mut World,
    panel: &PanelId,
    id: RequestId,
    act: &str,
    arguments: serde_json::Value,
    waits: Duration,
) {
    match page::of(world, panel) {
        Some(page) => {
            bridge::ask(world, page, id, act, arguments, waits);
        }
        None => answer(
            world,
            id,
            "Your Web tab has no page yet. Open one with web_open."
                .into(),
            true,
        ),
    }
}

/// `agent`'s Web tab, opened behind the others if it has none.
fn tab_of(world: &mut World, agent: u64) -> Option<PanelId> {
    let kept = world
        .resource::<AgentTabs>()
        .0
        .iter()
        .find(|(_, of)| **of == agent)
        .map(|(panel, _)| panel.clone());
    if let Some(panel) = kept
        && world
            .resource::<DockTree>()
            .find_leaf_with_window(&panel)
            .is_some()
    {
        return Some(PanelId(panel));
    }
    let (target, front) = {
        let tree = world.resource::<DockTree>();
        let root = tree.root?;
        let leaves = tree.leaves_under(root);
        let with_web = leaves.iter().find(|(_, leaf)| {
            leaf.tabs().any(|(window, _)| is_panel(window))
        });
        match with_web {
            Some((leaf, _)) => (
                DropTarget::Area(*leaf),
                tree.active(*leaf).zip(Some(*leaf)),
            ),
            None => (
                DropTarget::Edge {
                    leaf: leaves.first()?.0,
                    edge: Edge::Right,
                },
                None,
            ),
        }
    };
    let id = open_numbered(world, PANEL, target)?;
    // Behind what the chief was looking at.
    if let Some((tab, leaf)) = front {
        world.resource_mut::<DockTree>().set_active(leaf, tab);
    }
    world
        .resource_mut::<AgentTabs>()
        .0
        .insert(id.clone(), agent);
    Some(PanelId(id))
}

/// Answers the wait of a page that loaded.
fn loaded(
    loaded: On<LoadFinished>,
    pages: Query<(&Page, &WebviewTitle)>,
    mut commands: bevy::ecs::system::Commands,
) {
    let Ok((Page(panel), title)) = pages.get(loaded.webview) else {
        return;
    };
    let (panel, title, status) =
        (panel.clone(), title.0.clone(), loaded.http_status_code);
    commands.queue(move |world: &mut World| {
        let wait = world.resource_mut::<Waits>().0.remove(&panel.0);
        let address = world
            .resource::<Addresses>()
            .0
            .get(&panel.0)
            .cloned()
            .unwrap_or_default();
        // An agent's page leaves for another machine only with the
        // network, however it got there: opened, or by a click.
        let agents =
            world.resource::<AgentTabs>().0.contains_key(&panel.0);
        let net = wait.as_ref().map(|wait| wait.net).or_else(|| {
            world.resource::<Nets>().0.get(&panel.0).copied()
        });
        if agents
            && net == Some(false)
            && !is_on_this_machine(&address)
        {
            page::navigate(world, &panel, "about:blank".into());
            let words = format!(
                "{address} is on another machine, which your page \
                 may reach only with the network. It was left."
            );
            world
                .resource_mut::<Consoles>()
                .keep(&panel.0, format!("[error] {words}"));
            if let Some(wait) = wait {
                answer(world, wait.id, words, true);
            }
            return;
        }
        let Some(wait) = wait else {
            return;
        };
        let named = if title.is_empty() {
            String::new()
        } else {
            format!(", titled \"{title}\"")
        };
        answer(
            world,
            wait.id,
            format!(
                "Loaded {address}{named}, with HTTP status {status}."
            ),
            status >= 400,
        );
    });
}

/// Answers the wait of a page that failed to load.
fn failed(
    failed: On<LoadError>,
    pages: Query<&Page>,
    mut commands: bevy::ecs::system::Commands,
) {
    if failed.error_code == ABORTED {
        return;
    }
    let Ok(Page(panel)) = pages.get(failed.webview) else {
        return;
    };
    let (panel, url, code) =
        (panel.clone(), failed.url.clone(), failed.error_code);
    commands.queue(move |world: &mut World| {
        if world.resource::<AgentTabs>().0.contains_key(&panel.0) {
            world.resource_mut::<Consoles>().keep(
                &panel.0,
                format!(
                    "[error] {url} did not load: Chromium's error \
                     {code}"
                ),
            );
        }
        if let Some(wait) =
            world.resource_mut::<Waits>().0.remove(&panel.0)
        {
            answer(
                world,
                wait.id,
                format!(
                    "{url} did not load: Chromium's error {code}."
                ),
                true,
            );
        }
    });
}

/// Tells the agents whose pages are still loading so.
fn give_up(world: &mut World) {
    let now = world.resource::<Time<Real>>().elapsed();
    let late = world
        .resource::<Waits>()
        .0
        .iter()
        .filter(|(_, wait)| wait.until <= now)
        .map(|(panel, _)| panel.clone())
        .collect::<Vec<_>>();
    for panel in late {
        if let Some(wait) =
            world.resource_mut::<Waits>().0.remove(&panel)
        {
            answer(
                world,
                wait.id,
                format!(
                    "The page is still loading after {} s. It may \
                     finish later.",
                    LOAD_WITHIN.as_secs()
                ),
                false,
            );
        }
    }
}

/// Closes the tabs of agents that ended, once the realm is known.
fn close_gone(world: &mut World) {
    if !world.contains_resource::<RealmFolder>() {
        return;
    }
    let gone = world
        .resource::<AgentTabs>()
        .0
        .iter()
        .filter(|(_, agent)| {
            realm::agent(world, protocol::AgentId(**agent)).is_none()
        })
        .map(|(panel, _)| panel.clone())
        .collect::<Vec<_>>();
    for panel in gone {
        world.resource_mut::<AgentTabs>().0.remove(&panel);
        world.resource_mut::<DockTree>().remove_window_kind(&panel);
    }
}

/// The name of the agent of each agent's tab, by panel id.
#[derive(Resource, Default, PartialEq)]
pub(super) struct AgentNames(pub BTreeMap<String, String>);

/// Keeps [`AgentNames`] to the agents' names.
fn name_agents(world: &mut World) {
    let names = world
        .resource::<AgentTabs>()
        .0
        .iter()
        .filter_map(|(panel, agent)| {
            let entity: Entity =
                realm::agent(world, protocol::AgentId(*agent))?;
            Some((panel.clone(), realm::name_of(world, entity)))
        })
        .collect::<BTreeMap<_, _>>();
    if world.resource::<AgentNames>().0 != names {
        world.resource_mut::<AgentNames>().0 = names;
    }
}
