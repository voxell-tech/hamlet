//! The engine's own card of a setup, over the dock: a provider's key
//! typed, or a sign-in in the browser. What is typed goes from here
//! to the service, so no plugin ever holds a key. A plugin's setup
//! card hands over to it with `ShowKeyCard`, and it hands back once
//! a place answers.

use bevy::app::{App, Startup, Update};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::query::{Has, With};
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Local, Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::ui::AlignItems;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, label, row,
};
use bevy_fynix::{ViewExt, keyed, resource};
use hamlet_core::core::SignIn;
use hamlet_engine_api::PanelId;
use hamlet_engine_api::agents::{KeyWay, SignInTo, Stage};
use hamlet_engine_api::id::ENGINE;
use hamlet_engine_shell::fonts::DisplayText;
use hamlet_engine_shell::icons::{Glyph, glyph};
use hamlet_engine_shell::style::{
    ButtonStyle, action, buttons, description, field, icon_button,
    icon_row, raised, section,
};
use hamlet_engine_shell::{AnyView, Cx, Overlay, OverlayShown};
use hamlet_model::config::providers::{OPENROUTER, PROVIDERS};

use crate::AgentsWorldExt;
use crate::setup::SetupState;

/// The name of the card.
const CARD: &str = "keys";

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<KeyCard>()
        .add_systems(Startup, add)
        .add_systems(Update, follow);
}

/// The card: the way the person gives the setup what it needs while
/// it is shown, and what they typed.
#[derive(Resource, Default)]
pub(crate) struct KeyCard {
    pub way: Option<KeyWay>,
    /// The provider chosen, by its place in [`PROVIDERS`]; past
    /// their end for an address the person types.
    provider: usize,
    url: String,
    /// The key typed, until it is handed to the service.
    key: String,
}

/// On the card's overlay.
#[derive(Component)]
struct KeyOverlay;

fn add(mut commands: Commands) {
    commands.spawn((
        Overlay {
            id: PanelId::new(ENGINE, CARD).expect("a valid id"),
            content: view,
        },
        KeyOverlay,
    ));
}

/// Shows the card while it has a way and a setup is under way, and
/// moves it on as the setup moves.
fn follow(
    setup: Res<SetupState>,
    mut card: ResMut<KeyCard>,
    mut stage: Local<Option<Stage>>,
    overlay: Query<(Entity, Has<OverlayShown>), With<KeyOverlay>>,
    mut commands: Commands,
) {
    if !setup.begun && card.way.is_some() {
        card.way = None;
    }
    if *stage != Some(setup.stage) {
        *stage = Some(setup.stage);
        match setup.stage {
            // The plugin's card picks a model of it, or it is added.
            Stage::Found if card.way.is_some() => card.way = None,
            // Signing in did not finish, or the key it gave did not
            // answer: a key made by hand does as well.
            Stage::Idle
                if setup.problem.is_some()
                    && card.way
                        == Some(KeyWay::SignIn(
                            SignInTo::OpenRouter,
                        )) =>
            {
                card.way = Some(KeyWay::Key);
                card.provider = OPENROUTER;
            }
            _ => {}
        }
    }
    let wanted = card.way.is_some();
    for (entity, shown) in &overlay {
        if wanted && !shown {
            commands.entity(entity).insert(OverlayShown);
        } else if !wanted && shown {
            commands.entity(entity).remove::<OverlayShown>();
        }
    }
}

/// The card's methods on [`World`].
pub(crate) trait KeyCardWorldExt {
    /// Shows the card for `way`, afresh, and begins a sign-in.
    fn show_key_card(&mut self, way: KeyWay);

    /// Asks the provider chosen, or the address typed, with the key
    /// typed, which is forgotten here.
    fn connect(&mut self);

    /// Forgets why the last thing asked did not work.
    fn forget_problem(&mut self);
}

impl KeyCardWorldExt for World {
    fn show_key_card(&mut self, way: KeyWay) {
        *self.resource_mut::<KeyCard>() = KeyCard {
            way: Some(way),
            ..KeyCard::default()
        };
        self.forget_problem();
        let KeyWay::SignIn(to) = way else {
            return;
        };
        let mut setup = self.resource_mut::<SetupState>();
        setup.stage = Stage::SigningIn(to);
        setup.signing = Some(to);
        setup.models.clear();
        self.trigger(match to {
            SignInTo::OpenRouter => SignIn::OpenRouter,
            SignInTo::ChatGpt => SignIn::ChatGpt,
        });
    }

    fn connect(&mut self) {
        let mut card = self.resource_mut::<KeyCard>();
        let key = std::mem::take(&mut card.key);
        let (provider, url) = (
            PROVIDERS.get(card.provider).map(|known| known.endpoint),
            card.url.clone(),
        );
        match SetupState::endpoint(provider, &url, &key) {
            Ok((endpoint, from)) => self.ask_endpoint(endpoint, from),
            Err(why) => {
                self.resource_mut::<SetupState>().problem = Some(why);
            }
        }
    }

    fn forget_problem(&mut self) {
        let mut setup = self.resource_mut::<SetupState>();
        if setup.problem.is_some() {
            setup.problem = None;
        }
    }
}

fn view(_: Entity) -> AnyView {
    keyed(resource::<KeyCard, _>(|card| card.way), |way| match way {
        Some(KeyWay::Key) => key(),
        Some(KeyWay::SignIn(to)) => signing_in(*to),
        None => column(()).boxed(),
    })
    .boxed()
}

/// `parts` one under another, as far apart as the card's steps are.
fn spaced(parts: Vec<AnyView>) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let gap = cx.theme().space.gap * 2.0;
        cx.build(column(parts).gap(gap))
    })
}

/// `parts` one under another, close together.
fn close(parts: Vec<AnyView>) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let gap = cx.theme().space.gap / 2.0;
        cx.build(column(parts).gap(gap))
    })
}

fn title(which: Glyph, text: &'static str) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let (icon, size) =
            (cx.theme().space.icon, cx.theme().text.heading);
        cx.build(icon_row((
            raised(glyph(which, icon).tone(Tone::Accent)),
            label(text).size(size).bold(true).tagged(DisplayText),
        )))
    })
}

fn said(text: &'static str) -> AnyView {
    label(text).tone(Tone::Dim).boxed()
}

/// The head of the card: the way back to the plugin's card, and its
/// name.
fn head(which: Glyph, text: &'static str) -> AnyView {
    row((
        icon_button(ButtonStyle::Quiet, Glyph::Back, "BACK")
            .on_activate(|world| {
                world.forget_problem();
                world.resource_mut::<KeyCard>().way = None;
            }),
        title(which, text),
    ))
    .align(AlignItems::Center)
    .boxed()
}

/// The news of the last thing asked: that it is awaited, or why it
/// did not work.
fn news(waiting: &'static str) -> AnyView {
    keyed(
        resource::<SetupState, _>(|setup| {
            let busy = matches!(
                setup.stage,
                Stage::Asking | Stage::Looking | Stage::Trying
            );
            (busy, setup.problem.clone())
        }),
        move |(busy, problem)| match problem {
            _ if *busy => said(waiting),
            Some(why) => {
                label(why.clone()).tone(Tone::Critical).boxed()
            }
            None => column(()).boxed(),
        },
    )
    .boxed()
}

fn key() -> AnyView {
    spaced(vec![
        head(Glyph::Globe, "USE A KEY"),
        keyed(
            resource::<KeyCard, _>(|card| card.provider),
            |chosen| key_form(*chosen),
        )
        .boxed(),
    ])
}

/// The provider's key, and for an address of the person's own, the
/// address.
fn key_form(chosen: usize) -> AnyView {
    let names = PROVIDERS
        .iter()
        .map(|provider| provider.name.to_uppercase())
        .chain(["OTHER".to_string()])
        .enumerate()
        .map(|(at, name)| {
            let style = if at == chosen {
                ButtonStyle::Primary
            } else {
                ButtonStyle::Plain
            };
            action(style, name)
                .on_activate(move |world| {
                    world.resource_mut::<KeyCard>().provider = at;
                    world.forget_problem();
                })
                .boxed()
        })
        .collect::<Vec<_>>();
    let known = PROVIDERS.get(chosen);
    let help = match known {
        Some(provider) => format!(
            "Make a key at {} and paste it here. It stays on this \
             computer.",
            provider.keys
        ),
        None => "Paste the key your provider gave you. Leave this \
                 empty if it needs none."
            .into(),
    };
    let mut parts = vec![close(vec![
        section("WHO GIVES YOU YOUR MODEL"),
        buttons(names),
    ])];
    if known.is_none() {
        parts.push(close(vec![
            section("WHERE YOUR MODEL RUNS"),
            field(
                resource::<KeyCard, _>(|card| card.url.clone()),
                "https://".into(),
                |world, text| {
                    world.resource_mut::<KeyCard>().url = text
                },
            ),
            description(
                "The web address your provider gave you. It usually \
                 ends in /v1.",
            ),
        ]));
    }
    parts.extend([
        close(vec![
            section("YOUR KEY"),
            field(
                resource::<KeyCard, _>(|card| card.key.clone()),
                "Paste your key".into(),
                |world, text| {
                    world.resource_mut::<KeyCard>().key = text
                },
            ),
            description(help),
        ]),
        news("Asking for its models..."),
        row((icon_button(
            ButtonStyle::Primary,
            Glyph::Check,
            "CONNECT",
        )
        .on_activate(|world| world.connect()),))
        .boxed(),
    ]);
    spaced(parts)
}

/// The page shown while the person signs in in the browser.
fn signing_in(to: SignInTo) -> AnyView {
    let asked = match to {
        SignInTo::ChatGpt => {
            "Sign in to ChatGPT in your browser and allow Hamlet to \
             use your plan. Hamlet carries on once you have."
        }
        SignInTo::OpenRouter => {
            "Sign in to OpenRouter in your browser and choose \
             Authorize. Hamlet carries on once you have."
        }
    };
    spaced(vec![
        head(Glyph::Signal, "SIGN IN"),
        said(asked),
        news("Asking for its models..."),
        description(
            "Nothing opened? Go back and choose I Have a Key \
             instead.",
        ),
    ])
}
