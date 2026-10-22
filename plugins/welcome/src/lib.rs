//! The first-run card over the dock. It leads the person of a home
//! where no model was chosen to one that answers, by a provider's
//! key, a sign-in in the browser, or a server on this computer, and
//! adds another place for models the same way. The setup runs in the
//! agents service; the card drives it and shows how it goes. It
//! opens at start when no model was chosen, and whenever a setup is
//! begun, as Settings does.

mod pages;

use agents_ui::{AgentsWorldExt, FailureExt, Heard, start};
use hamlet_engine_bevy::api::agents::{
    AgentsEvent, AgentsReply, AgentsRequest, Provider, Setup,
    SignInTo, Stage,
};
use hamlet_engine_bevy::bevy_ecs::message::MessageCursor;
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::ui::show_overlay;
use serde::{Deserialize, Serialize};

/// The name of the card.
const CARD: &str = "welcome";

/// The endpoint name of the provider a sign-in gives a key for.
const OPENROUTER: &str = "openrouter";

#[derive(Default)]
struct WelcomeCard;

impl Plugin for WelcomeCard {
    fn build(&self, app: &mut App) {
        app.add_plugins(agents_ui::plugin)
            .init_saved_resource::<Welcome>()
            .add_overlay(CARD, pages::card)
            .add_systems(
                Declare,
                (
                    start(&[
                        AgentsRequest::Providers,
                        AgentsRequest::Setup,
                        AgentsRequest::Settings,
                    ]),
                    reopen,
                ),
            )
            .add_systems(Update, receive);
    }
}

hamlet_engine_bevy::export!(WelcomeCard);

/// The step of the card the person is at.
#[derive(
    Clone, Copy, PartialEq, Eq, Default, Debug, Serialize, Deserialize,
)]
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
/// choices and words on it. What is kept across a reload leaves out
/// the key, the providers and the last problem.
#[derive(Resource, Default, Serialize, Deserialize)]
struct Welcome {
    setup: Option<Setup>,
    #[serde(skip)]
    providers: Vec<Provider>,
    step: Step,
    /// The provider chosen, by its place in `providers`; past their
    /// end for an address the person types.
    provider: usize,
    url: String,
    /// The key typed, until it is handed to the service.
    #[serde(skip)]
    key: String,
    /// The words the models listed must have.
    narrow: String,
    /// Why the last request failed, when the setup does not say.
    #[serde(skip)]
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
        self.forget_problem();
    }

    /// Forgets why the last thing asked did not work.
    fn forget_problem(&mut self) {
        self.problem = None;
        if let Some(setup) = &mut self.setup {
            setup.problem = None;
        }
    }

    /// Whether the service is still at the last thing asked. A
    /// sign-in waits on the person, not the service.
    fn busy(&self) -> bool {
        self.setup.as_ref().is_some_and(|setup| {
            matches!(
                setup.stage,
                Stage::Asking | Stage::Looking | Stage::Trying
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
                    && self.step
                        == Step::SignIn(SignInTo::OpenRouter) =>
            {
                // Signing in did not finish, or the key it gave did
                // not answer: a key made by hand does as well.
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
        let mut welcome = self.resource_mut::<Welcome>();
        match reply {
            AgentsReply::Providers(providers) => {
                welcome.providers = providers;
            }
            AgentsReply::Setup(setup) => welcome.follow(setup),
            // At start, a home where no model was chosen is set up.
            AgentsReply::Settings(settings) if !welcome.looked => {
                welcome.looked = true;
                let begun = welcome
                    .setup
                    .as_ref()
                    .is_some_and(|setup| setup.begun);
                if !settings.chosen && !begun {
                    self.agents(&AgentsRequest::BeginSetup {
                        adding: false,
                    });
                }
            }
            _ => {}
        }
    }

    fn ask(&mut self, request: AgentsRequest) {
        self.resource_mut::<Welcome>().problem = None;
        self.agents(&request);
    }
}

/// Shows the card again after a reload while a setup is under way.
fn reopen(welcome: Res<Welcome>) {
    if welcome.setup.as_ref().is_some_and(|setup| setup.begun) {
        show_overlay(CARD, true);
    }
}

fn receive(
    world: &mut World,
    mut cursor: Local<MessageCursor<Heard>>,
) {
    for told in world.heard(&mut cursor) {
        match told {
            Heard::Reply(_, reply) => world.hear(reply),
            Heard::Event(AgentsEvent::Setup(setup)) => {
                world.resource_mut::<Welcome>().follow(setup);
            }
            // Shown on the card, as the service's own problems are.
            Heard::Failed(_, why) => {
                world.resource_mut::<Welcome>().problem =
                    Some(why.plain());
            }
            Heard::Event(_) => {}
        }
    }
}
