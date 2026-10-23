//! The first-run card over the dock. It leads the person of a home
//! where no model was chosen to one that answers, by a provider's
//! key, a sign-in in the browser, or a server on this computer, and
//! adds another place for models the same way. The setup runs in the
//! agents service; the card drives it and shows how it goes. A key
//! and a sign-in are the engine's own card, which the card hands
//! over to and which hands back once a place answers. It opens at
//! start when no model was chosen, and whenever a setup is begun, as
//! Settings does.

mod pages;

use hamlet_engine_bevy::api::agents::{
    AgentsEvent, AgentsReply, AgentsRequest, Setup, Stage,
};
use hamlet_engine_bevy::bevy_ecs::message::MessageCursor;
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::ui::show_overlay;
use serde::{Deserialize, Serialize};

use crate::shared::{AgentsWorldExt, FailureExt, Heard, start};

/// The name of the card.
const CARD: &str = "welcome";

pub(crate) fn plugin(app: &mut App) {
    app.init_saved_resource::<Welcome>()
        .add_overlay(CARD, pages::card)
        .add_systems(
            Declare,
            (
                start(&[
                    AgentsRequest::Setup,
                    AgentsRequest::Settings,
                ]),
                reopen,
            ),
        )
        .add_systems(Update, receive);
}

/// The step of the card the person is at.
#[derive(
    Clone, Copy, PartialEq, Eq, Default, Debug, Serialize, Deserialize,
)]
enum Step {
    /// The ways in.
    #[default]
    Choice,
    /// Looking for a server on this computer.
    Local,
    /// The models of whoever answered.
    Models,
}

/// The card: the setup as the service tells it, and the person's
/// choices and words on it. What is kept across a reload leaves out
/// the last problem.
#[derive(Resource, Default, Serialize, Deserialize)]
struct Welcome {
    setup: Option<Setup>,
    step: Step,
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
        self.narrow.clear();
        self.problem = None;
        if let Some(setup) = &mut self.setup {
            setup.problem = None;
        }
    }

    /// Whether the service is still at the last thing asked.
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
            show_overlay(CARD, true);
        }
        if !setup.begun && was {
            self.go(Step::Choice);
            show_overlay(CARD, false);
        }
        let stage = before.as_ref().map(|before| before.stage);
        if stage != Some(setup.stage)
            && setup.stage == Stage::Found
            && !setup.adding
        {
            self.step = Step::Models;
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
        AgentsWorldExt::ask(self, request);
    }
}

/// Shows the card again after a reload while a setup is under way.
fn reopen(welcome: Res<Welcome>) {
    if welcome.setup.as_ref().is_some_and(|setup| setup.begun) {
        show_overlay(CARD, true);
    }
}

/// Whether `request` is one the card asks.
fn of_card(request: &AgentsRequest) -> bool {
    matches!(
        request,
        AgentsRequest::ShowKeyCard(_)
            | AgentsRequest::FindLocal
            | AgentsRequest::PickModel(_)
            | AgentsRequest::EndSetup
    )
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
            Heard::Failed(Some(request), why)
                if of_card(&request) =>
            {
                world.resource_mut::<Welcome>().problem =
                    Some(why.plain());
            }
            Heard::Failed(..) | Heard::Event(_) => {}
        }
    }
}
