//! The first-run card over the dock. It leads the person of a home
//! where no model was chosen to one that answers, by a provider's
//! key, a sign-in in the browser, or a server on this computer, and
//! adds another place for models the same way. The setup runs in the
//! agents service; the card drives it and shows how it goes. It
//! opens at start when no model was chosen, and whenever a setup is
//! begun, as Settings does.

mod pages;

use hamlet_engine_sdk::api::agents::{
    AgentsEvent, AgentsReply, AgentsRequest, Provider, Setup,
    SignInTo, Stage,
};
use hamlet_engine_sdk::ui::{AnyView, show_overlay};
use hamlet_engine_sdk::{
    Entry, Message, Plugin, Reply, World, agents,
};

/// The name of the card.
const CARD: &str = "welcome";

/// The endpoint name of the provider a sign-in gives a key for.
const OPENROUTER: &str = "openrouter";

#[derive(Default)]
struct WelcomeCard;

impl Plugin for WelcomeCard {
    fn declare(&self, entry: &mut Entry) {
        entry.overlay(CARD);
        for request in [
            AgentsRequest::Watch,
            AgentsRequest::Providers,
            AgentsRequest::Setup,
            AgentsRequest::Settings,
        ] {
            agents(&request);
        }
    }

    fn mount(&mut self, _: &str, world: &mut World) -> AnyView {
        world.init_resource::<Welcome>();
        pages::card()
    }

    fn receive(&mut self, message: Message, world: &mut World) {
        let Message::Reply { reply, .. } = message else {
            return;
        };
        match reply {
            Ok(Reply::Agents(reply)) => world.hear(reply),
            Ok(_) => {}
            // Shown on the card, as the service's own problems are.
            Err(why) => {
                world.resource_or_init::<Welcome>().problem =
                    Some(why.to_string());
            }
        }
    }
}

/// The step of the card the person is at.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
enum Step {
    /// The ways in.
    #[default]
    Choice,
    /// A provider and its key.
    Key,
    /// Signing in in the browser.
    SignIn(SignInTo),
    /// Looking for a server on this computer.
    Local,
    /// The models of whoever answered.
    Models,
}

/// The card: the setup as the service tells it, and the person's
/// choices and words on it.
#[derive(Default)]
struct Welcome {
    setup: Option<Setup>,
    providers: Vec<Provider>,
    step: Step,
    /// The provider chosen, by its place in `providers`; past their
    /// end for an address the person types.
    provider: usize,
    url: String,
    /// The key typed, until it is handed to the service.
    key: String,
    /// The words the models listed must have.
    narrow: String,
    /// Why the last request failed, when the setup does not say.
    problem: Option<String>,
    /// The settings were heard of, as the card first looks.
    looked: bool,
}

impl Welcome {
    /// Goes to `step`, with nothing typed and no problem.
    fn go(&mut self, step: Step) {
        self.step = step;
        self.key.clear();
        self.narrow.clear();
        self.problem = None;
    }

    /// Whether the service is still at the last thing asked.
    fn busy(&self) -> bool {
        self.setup.as_ref().is_some_and(|setup| {
            matches!(
                setup.stage,
                Stage::Asking
                    | Stage::Looking
                    | Stage::Trying
                    | Stage::SigningIn(_)
            )
        })
    }

    /// Why the last thing asked did not work, in plain words.
    fn trouble(&self) -> Option<String> {
        self.problem.clone().or_else(|| {
            self.setup
                .as_ref()
                .and_then(|setup| setup.problem.clone())
        })
    }

    /// Takes in `setup`, and moves the card on as it moved.
    fn follow(&mut self, setup: Setup) {
        let before = self.setup.take();
        let was = before.as_ref().is_some_and(|setup| setup.begun);
        if setup.begun && !was {
            self.go(Step::Choice);
            self.provider = 0;
            self.url.clear();
            show_overlay(CARD, true);
        }
        if !setup.begun && was {
            self.go(Step::Choice);
            show_overlay(CARD, false);
        }
        let stage = before.as_ref().map(|before| before.stage);
        if stage == Some(setup.stage) {
            self.setup = Some(setup);
            return;
        }
        match setup.stage {
            Stage::SigningIn(to) => self.step = Step::SignIn(to),
            Stage::Found if !setup.adding => self.step = Step::Models,
            Stage::Idle
                if setup.problem.is_some()
                    && stage
                        == Some(Stage::SigningIn(
                            SignInTo::OpenRouter,
                        )) =>
            {
                // Signing in did not finish: a key made by hand
                // does as well.
                self.step = Step::Key;
                self.provider = self
                    .providers
                    .iter()
                    .position(|known| known.endpoint == OPENROUTER)
                    .unwrap_or_default();
            }
            _ => {}
        }
        self.setup = Some(setup);
    }
}

/// The card's methods on [`World`].
trait WelcomeWorldExt {
    /// Takes in `reply` from the agents service.
    fn hear(&mut self, reply: AgentsReply);

    /// Asks the agents service `request` for the card, forgetting the
    /// last problem.
    fn ask(&mut self, request: AgentsRequest);
}

impl WelcomeWorldExt for World {
    fn hear(&mut self, reply: AgentsReply) {
        let welcome = self.resource_or_init::<Welcome>();
        match reply {
            AgentsReply::Providers(providers) => {
                welcome.providers = providers;
            }
            AgentsReply::Setup(setup)
            | AgentsReply::Event(AgentsEvent::Setup(setup)) => {
                welcome.follow(setup);
            }
            // At start, a home where no model was chosen is set up.
            AgentsReply::Settings(settings) if !welcome.looked => {
                welcome.looked = true;
                let begun = welcome
                    .setup
                    .as_ref()
                    .is_some_and(|setup| setup.begun);
                if !settings.chosen && !begun {
                    agents(&AgentsRequest::BeginSetup {
                        adding: false,
                    });
                }
            }
            _ => {}
        }
    }

    fn ask(&mut self, request: AgentsRequest) {
        self.resource_or_init::<Welcome>().problem = None;
        agents(&request);
    }
}

hamlet_engine_sdk::export!(WelcomeCard);
