//! The agents' pages of Settings: where models come from, the models
//! and their prices, the agents' own settings and lasting
//! permissions, the charter, and the limits of the realm, with a
//! footer that sets up a place for models again. What they show comes
//! from the agents service, and every change goes to it.

mod agents;
mod charter;
mod endpoints;
mod models;
mod parts;
mod realm;

use std::collections::{BTreeMap, HashMap};

use agents_ui::FailureExt;
use hamlet_engine_bevy::api::agents::{
    AgentsEvent, AgentsReply, AgentsRequest, CatalogueModel, Charter,
    Grant, RealmLimits, Settings, Stage,
};
use hamlet_engine_bevy::api::desktop::DesktopSettings;
use hamlet_engine_bevy::bevy_ecs::message::MessageCursor;
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::ui::open;
use hamlet_engine_bevy::{
    DesktopEvent, DesktopReply, DesktopRequest, Reply, Ticket, warn,
};

/// The most models the Models page lists at once.
const SHOWN: usize = 40;

/// The most requests remembered at once.
const REMEMBERED: usize = 256;

#[derive(Default)]
struct SettingsPages;

impl Plugin for SettingsPages {
    fn build(&self, app: &mut App) {
        app.init_resource::<Known>()
            .init_resource::<Asked>()
            .add_settings_page(
                "endpoints",
                "Endpoints",
                "Where models come from, and their keys.",
                "globe",
                endpoints::page,
            )
            .add_settings_page(
                "models",
                "Models",
                "The default model, which models are allowed and \
                 their prices.",
                "chip",
                models::page,
            )
            .add_settings_page(
                "agents",
                "Agents",
                "Reflection, network access and keeping awake.",
                "robot",
                agents::page,
            )
            .add_settings_page(
                "charter",
                "Charter",
                "What every agent is told.",
                "book",
                charter::page,
            )
            .add_settings_page(
                "realm",
                "Realm",
                "Rules that apply to every hamlet.",
                "signpost",
                realm::page,
            )
            .add_settings_footer("again", parts::again)
            .add_systems(Declare, start)
            .add_systems(Update, receive);
    }
}

hamlet_engine_bevy::export!(SettingsPages);

/// A page of these settings, whose problem a failed request shows.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Page {
    Endpoints,
    Models,
    Agents,
    Charter,
    Realm,
}

/// What the pages know of the agents service.
#[derive(Resource, Default)]
struct Known {
    settings: Option<Settings>,
    models: Vec<CatalogueModel>,
    /// The words the models listed must have.
    narrow: String,
    charter: Option<Charter>,
    /// The person is asked whether to go back to the default
    /// charter.
    confirming: bool,
    limits: Option<RealmLimits>,
    grants: Vec<Grant>,
    /// The machine is kept awake while an agent works, as the
    /// desktop service has it.
    keep_awake: Option<bool>,
    /// Why the last change asked on each page did not go through.
    problems: HashMap<Page, String>,
}

/// The requests that wait for an answer, by ticket, with the page
/// each was asked on.
#[derive(Resource, Default)]
struct Asked(BTreeMap<Ticket, Page>);

impl Asked {
    /// Remembers the page `ticket` was asked on. A reply the inbox
    /// left out never comes, so the oldest are forgotten past
    /// [`REMEMBERED`].
    fn keep(&mut self, ticket: Ticket, page: Page) {
        if ticket == 0 {
            return;
        }
        self.0.insert(ticket, page);
        if self.0.len() > REMEMBERED {
            self.0.pop_first();
        }
    }
}

/// Watches the agents and desktop services and asks for what the
/// pages show, as the plugin starts.
fn start(world: &mut World) {
    for request in [
        AgentsRequest::Watch,
        AgentsRequest::Settings,
        AgentsRequest::Catalogue,
        AgentsRequest::Charter,
        AgentsRequest::Limits,
        AgentsRequest::Grants,
    ] {
        world.agents(&request);
    }
    world.desktop(&DesktopRequest::Watch);
    world.desktop(&DesktopRequest::Settings);
}

/// Takes in the replies and the events of the agents and desktop
/// services.
fn receive(
    world: &mut World,
    mut replies: Local<MessageCursor<Replied>>,
    mut agents: Local<MessageCursor<AgentsEventReceived>>,
    mut desktop: Local<MessageCursor<DesktopEventReceived>>,
) {
    let replied = replies
        .read(world.resource::<Messages<Replied>>())
        .cloned()
        .collect::<Vec<_>>();
    let agents = agents
        .read(world.resource::<Messages<AgentsEventReceived>>())
        .cloned()
        .collect::<Vec<_>>();
    let desktop = desktop
        .read(world.resource::<Messages<DesktopEventReceived>>())
        .cloned()
        .collect::<Vec<_>>();
    for Replied { ticket, reply } in replied {
        let page = world.resource_mut::<Asked>().0.remove(&ticket);
        match (reply, page) {
            (Ok(Reply::Agents(reply)), page) => {
                world.hear(reply, page)
            }
            (
                Ok(Reply::Desktop(DesktopReply::Settings(settings))),
                page,
            ) => world.hear_desktop(settings, page),
            (Ok(_), _) => {}
            (Err(why), Some(page)) => world.refuse(page, why.plain()),
            (Err(why), None) => warn(&format!("the agents: {why}")),
        }
    }
    for AgentsEventReceived { ticket, event } in agents {
        let page = world.resource_mut::<Asked>().0.remove(&ticket);
        world.hear(AgentsReply::Event(event), page);
    }
    for DesktopEventReceived { ticket, event } in desktop {
        let page = world.resource_mut::<Asked>().0.remove(&ticket);
        if let DesktopEvent::Settings(settings) = event {
            world.hear_desktop(settings, page);
        }
    }
}

/// The settings pages' methods on [`World`].
trait SettingsWorldExt {
    /// Asks the agents service `request` on `page`, which shows why
    /// if it fails.
    fn ask(&mut self, page: Page, request: AgentsRequest);

    /// Asks the desktop service `request` on `page`, which shows why
    /// if it fails.
    fn ask_desktop(&mut self, page: Page, request: DesktopRequest);

    /// Shows on `page` why what the person did cannot be done.
    fn refuse(&mut self, page: Page, why: String);

    /// Writes `value` at `key` of `section` of the realm's limits.
    fn set_limit(&mut self, section: &str, key: &str, value: String);

    /// Takes in `reply`, to a request asked on `page` if any.
    fn hear(&mut self, reply: AgentsReply, page: Option<Page>);

    /// Takes in the desktop service's `settings`, the reply to a
    /// request asked on `page` if any.
    fn hear_desktop(
        &mut self,
        settings: DesktopSettings,
        page: Option<Page>,
    );
}

impl SettingsWorldExt for World {
    fn ask(&mut self, page: Page, request: AgentsRequest) {
        let ticket = self.agents(&request);
        self.resource_mut::<Asked>().keep(ticket, page);
    }

    fn ask_desktop(&mut self, page: Page, request: DesktopRequest) {
        let ticket = self.desktop(&request);
        self.resource_mut::<Asked>().keep(ticket, page);
    }

    fn refuse(&mut self, page: Page, why: String) {
        self.resource_mut::<Known>().problems.insert(page, why);
    }

    fn set_limit(&mut self, section: &str, key: &str, value: String) {
        self.ask(
            Page::Realm,
            AgentsRequest::SetLimit {
                section: section.into(),
                key: key.into(),
                value: Some(value),
            },
        );
    }

    fn hear(&mut self, reply: AgentsReply, page: Option<Page>) {
        match reply {
            AgentsReply::Done => {
                if let Some(page) = page {
                    self.resource_mut::<Known>()
                        .problems
                        .remove(&page);
                }
                // The file is read again when the service reads it;
                // the page shows it at once.
                match page {
                    Some(Page::Realm) => {
                        self.agents(&AgentsRequest::Limits);
                    }
                    Some(Page::Charter) => {
                        self.agents(&AgentsRequest::Charter);
                    }
                    _ => {}
                }
            }
            AgentsReply::Settings(settings)
            | AgentsReply::Event(AgentsEvent::Settings(settings)) => {
                self.resource_mut::<Known>().settings =
                    Some(settings);
            }
            AgentsReply::Catalogue(models) => {
                self.resource_mut::<Known>().models = models;
            }
            AgentsReply::Event(AgentsEvent::Catalogue) => {
                self.agents(&AgentsRequest::Catalogue);
            }
            AgentsReply::Charter(charter)
            | AgentsReply::Event(AgentsEvent::Charter(charter)) => {
                let mut known = self.resource_mut::<Known>();
                if !charter.own {
                    known.confirming = false;
                }
                known.charter = Some(charter);
            }
            AgentsReply::Limits(limits)
            | AgentsReply::Event(AgentsEvent::Limits(limits)) => {
                self.resource_mut::<Known>().limits = Some(limits);
            }
            AgentsReply::Grants(grants)
            | AgentsReply::Event(AgentsEvent::Grants(grants)) => {
                self.resource_mut::<Known>().grants = grants;
            }
            // A place for models was added: its models are for the
            // person to allow.
            AgentsReply::Event(AgentsEvent::Setup(setup))
                if setup.adding && setup.stage == Stage::Done =>
            {
                open("models");
            }
            _ => {}
        }
    }

    fn hear_desktop(
        &mut self,
        settings: DesktopSettings,
        page: Option<Page>,
    ) {
        let mut known = self.resource_mut::<Known>();
        known.keep_awake = Some(settings.keep_awake);
        if let Some(page) = page {
            known.problems.remove(&page);
        }
    }
}
