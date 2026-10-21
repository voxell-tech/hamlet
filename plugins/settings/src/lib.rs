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
use hamlet_engine_sdk::api::agents::{
    AgentsEvent, AgentsReply, AgentsRequest, CatalogueModel, Charter,
    Grant, RealmLimits, Settings, Stage,
};
use hamlet_engine_sdk::ui::{AnyView, ViewExt, column, open};
use hamlet_engine_sdk::{
    DesktopEvent, DesktopReply, DesktopRequest, Entry, Message,
    Plugin, Reply, Ticket, World, agents, desktop, warn,
};

/// The most models the Models page lists at once.
const SHOWN: usize = 40;

/// The most requests remembered at once.
const REMEMBERED: usize = 256;

#[derive(Default)]
struct SettingsPages;

impl Plugin for SettingsPages {
    fn declare(&self, entry: &mut Entry) {
        entry
            .settings_page(
                "endpoints",
                "Endpoints",
                "Where models come from, and their keys.",
                "globe",
            )
            .settings_page(
                "models",
                "Models",
                "The default model, which models are allowed and \
                 their prices.",
                "chip",
            )
            .settings_page(
                "agents",
                "Agents",
                "Reflection, network access and keeping awake.",
                "robot",
            )
            .settings_page(
                "charter",
                "Charter",
                "What every agent is told.",
                "book",
            )
            .settings_page(
                "realm",
                "Realm",
                "Rules that apply to every hamlet.",
                "signpost",
            )
            .settings_footer("again");
        for request in [
            AgentsRequest::Watch,
            AgentsRequest::Settings,
            AgentsRequest::Catalogue,
            AgentsRequest::Charter,
            AgentsRequest::Limits,
            AgentsRequest::Grants,
        ] {
            agents(&request);
        }
        desktop(&DesktopRequest::Watch);
        desktop(&DesktopRequest::Settings);
    }

    fn mount(&mut self, name: &str, world: &mut World) -> AnyView {
        world.init_resource::<Known>();
        world.init_resource::<Asked>();
        match name {
            "endpoints" => endpoints::page(),
            "models" => models::page(),
            "agents" => agents::page(),
            "charter" => charter::page(),
            "realm" => realm::page(),
            "again" => parts::again(),
            _ => column(()).boxed(),
        }
    }

    fn receive(&mut self, message: Message, world: &mut World) {
        let Message::Reply { ticket, reply } = message else {
            return;
        };
        let page =
            world.resource_or_init::<Asked>().0.remove(&ticket);
        match (reply, page) {
            (Ok(Reply::Agents(reply)), page) => {
                world.hear(reply, page)
            }
            (
                Ok(Reply::Desktop(
                    DesktopReply::Settings(settings)
                    | DesktopReply::Event(DesktopEvent::Settings(
                        settings,
                    )),
                )),
                page,
            ) => {
                let known = world.resource_or_init::<Known>();
                known.keep_awake = Some(settings.keep_awake);
                if let Some(page) = page {
                    known.problems.remove(&page);
                }
            }
            (Ok(_), _) => {}
            (Err(why), Some(page)) => world.refuse(page, why.plain()),
            (Err(why), None) => warn(&format!("the agents: {why}")),
        }
    }
}

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
#[derive(Default)]
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
#[derive(Default)]
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
}

impl SettingsWorldExt for World {
    fn ask(&mut self, page: Page, request: AgentsRequest) {
        let ticket = agents(&request);
        self.resource_or_init::<Asked>().keep(ticket, page);
    }

    fn ask_desktop(&mut self, page: Page, request: DesktopRequest) {
        let ticket = desktop(&request);
        self.resource_or_init::<Asked>().keep(ticket, page);
    }

    fn refuse(&mut self, page: Page, why: String) {
        self.resource_or_init::<Known>().problems.insert(page, why);
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
        let known = self.resource_or_init::<Known>();
        match reply {
            AgentsReply::Done => {
                if let Some(page) = page {
                    known.problems.remove(&page);
                }
                // The file is read again when the service reads it;
                // the page shows it at once.
                match page {
                    Some(Page::Realm) => {
                        agents(&AgentsRequest::Limits);
                    }
                    Some(Page::Charter) => {
                        agents(&AgentsRequest::Charter);
                    }
                    _ => {}
                }
            }
            AgentsReply::Settings(settings)
            | AgentsReply::Event(AgentsEvent::Settings(settings)) => {
                known.settings = Some(settings);
            }
            AgentsReply::Catalogue(models) => known.models = models,
            AgentsReply::Event(AgentsEvent::Catalogue) => {
                agents(&AgentsRequest::Catalogue);
            }
            AgentsReply::Charter(charter)
            | AgentsReply::Event(AgentsEvent::Charter(charter)) => {
                if !charter.own {
                    known.confirming = false;
                }
                known.charter = Some(charter);
            }
            AgentsReply::Limits(limits)
            | AgentsReply::Event(AgentsEvent::Limits(limits)) => {
                known.limits = Some(limits);
            }
            AgentsReply::Grants(grants)
            | AgentsReply::Event(AgentsEvent::Grants(grants)) => {
                known.grants = grants;
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
}

hamlet_engine_sdk::export!(SettingsPages);
