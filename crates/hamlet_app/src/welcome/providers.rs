//! Provider choices and the server's login steps.

use std::collections::BTreeSet;

use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::{ChildOf, Children};
use bevy::ecs::query::{Has, With};
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Query, Res};
use bevy::ecs::world::World;
use bevy::text::EditableText;
use bevy::ui::{AlignItems, InteractionDisabled};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, button, frame, label, row, text_field,
};
use bevy_fynix::{
    ScopedExt, ViewExt, derived, every_frame, keyed, resource,
};
use hamlet_client::messages::Response;
use hamlet_client::outbox::Outbox;
use hamlet_client::realm::Providers;
use hamlet_protocol::client as protocol;
use hamlet_ui::AnyView;
use hamlet_ui::styles::{
    ICON, Refused, bare, buttons, choice, close, good_action, group,
    note, parts, section, small_rules, text_input,
};
use hamlet_ui::widgets::hint::hinted;
use hamlet_ui::widgets::icons::Glyph;

use super::pages::{head, news, said};
use super::{Step, Welcome, commit, go, read_models};
use crate::models::mark;
use crate::secret::secret_field;

/// The browser used for the server's login URLs.
#[derive(Resource)]
pub struct Browser(pub fn(&mut World, &str));

impl Default for Browser {
    fn default() -> Self {
        Self(|_, url| {
            hamlet_desktop::open::browse(url);
        })
    }
}

#[derive(Resource, Default)]
pub(super) struct Login {
    pub provider: Option<String>,
    start: Option<protocol::RequestId>,
    id: Option<String>,
    abandoned: BTreeSet<protocol::RequestId>,
    replies: BTreeSet<protocol::RequestId>,
    urls: Vec<String>,
    instructions: Vec<String>,
    code: Option<String>,
    prompt: Option<(String, bool)>,
    progress: String,
    pub(super) answer: String,
    /// Whether `answer` is a key typed before the sign-in asked for
    /// it, to be its answer once it does.
    held: bool,
}

pub(super) fn start(world: &mut World, provider: &str, key: bool) {
    let method = if key {
        protocol::LoginMethod::Key
    } else {
        protocol::LoginMethod::Subscription
    };
    if !world.resource::<Providers>().0.iter().any(|known| {
        known.id == provider && known.methods.contains(&method)
    }) {
        return;
    }
    go(world, if key { Step::Key } else { Step::SignIn });
    let provider = provider.to_string();
    let id = world.resource_mut::<Outbox>().send(|id| {
        protocol::ClientMessage::Login(protocol::Login::Start {
            id,
            start: protocol::LoginStart {
                provider: provider.clone(),
                method,
            },
        })
    });
    let from = world
        .resource::<Providers>()
        .0
        .iter()
        .find(|known| known.id == provider)
        .map(|known| known.name.clone())
        .unwrap_or_default();
    let mut login = world.resource_mut::<Login>();
    login.provider = Some(provider);
    login.start = Some(id);
    let mut welcome = world.resource_mut::<Welcome>();
    welcome.asking = true;
    welcome.from = from;
}

fn send_cancel(world: &mut World, login: String) {
    world.resource_mut::<Outbox>().send(|id| {
        protocol::ClientMessage::Login(protocol::Login::Cancel {
            id,
            cancel: login,
        })
    });
}

pub(super) fn cancel(world: &mut World) {
    let mut login = world.resource_mut::<Login>();
    if let Some(id) = login.id.take() {
        send_cancel(world, id);
    } else if let Some(start) = login.start.take() {
        login.abandoned.insert(start);
    }
    let abandoned =
        std::mem::take(&mut world.resource_mut::<Login>().abandoned);
    *world.resource_mut::<Login>() = Login {
        abandoned,
        ..Default::default()
    };
    if let Some(mut pending) =
        world.get_resource_mut::<super::Committing>()
    {
        pending.0 = false;
    }
}

pub(super) fn reconnected(world: &mut World) {
    if world.resource::<Login>().start.is_some() {
        go(world, Step::Choice);
    }
}

/// The provider a key is for: the one chosen, else the first that
/// takes a key.
fn keyed_provider(world: &World) -> Option<String> {
    if let Some(chosen) = &world.resource::<Login>().provider {
        return Some(chosen.clone());
    }
    world
        .resource::<Providers>()
        .0
        .iter()
        .find(|p| p.methods.contains(&protocol::LoginMethod::Key))
        .map(|first| first.id.clone())
}

/// Starts a sign-in with the chosen provider that answers its prompt
/// with the key typed.
fn send_key(world: &mut World) {
    let answer = world.resource::<Login>().answer.clone();
    let Some(provider) = keyed_provider(world) else {
        return;
    };
    if answer.trim().is_empty() {
        return;
    }
    start(world, &provider, true);
    let mut login = world.resource_mut::<Login>();
    login.answer = answer;
    login.held = true;
}

pub(super) fn reply(world: &mut World) {
    let mut login = world.resource_mut::<Login>();
    let Some(id) =
        login.id.clone().filter(|_| login.prompt.is_some())
    else {
        if world.resource::<Welcome>().step == Step::Key {
            send_key(world);
        }
        return;
    };
    // The key stays in its field, to be put right after a failed try.
    let answer = login.answer.clone();
    login.prompt = None;
    let request = world.resource_mut::<Outbox>().send(|request| {
        protocol::ClientMessage::Login(protocol::Login::Reply {
            id: request,
            reply: protocol::LoginReply {
                login: id,
                value: answer,
            },
        })
    });
    world.resource_mut::<Login>().replies.insert(request);
    world.resource_mut::<Welcome>().asking = true;
}

/// Whether a response was one the sign-in waits for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Received {
    Login,
    Other,
}

pub(super) fn receive(
    world: &mut World,
    response: &Response,
) -> Received {
    if world.resource_mut::<Login>().abandoned.remove(&response.to) {
        if let protocol::ServerMessage::Login(progress) =
            &response.message
        {
            send_cancel(world, progress.login.clone());
        }
        return Received::Login;
    }
    if world.resource_mut::<Login>().replies.remove(&response.to) {
        let failure = match &response.message {
            protocol::ServerMessage::Fault(fault) => {
                Some(fault.text.clone())
            }
            protocol::ServerMessage::Reply(reply)
                if reply.kind == protocol::ReplyKind::Error =>
            {
                Some(reply.text.clone())
            }
            _ => None,
        };
        if let Some(problem) = failure {
            let mut welcome = world.resource_mut::<Welcome>();
            welcome.problem = Some(problem);
            welcome.asking = false;
        }
        return Received::Login;
    }
    if world.resource::<Login>().start.as_ref() != Some(&response.to)
    {
        return Received::Other;
    }
    let step = match &response.message {
        protocol::ServerMessage::Login(progress) => {
            world.resource_mut::<Login>().id =
                Some(progress.login.clone());
            progress.step.clone()
        }
        protocol::ServerMessage::Fault(fault) => {
            protocol::LoginStep::Failed {
                failed: fault.text.clone(),
            }
        }
        protocol::ServerMessage::Reply(reply)
            if reply.kind == protocol::ReplyKind::Error =>
        {
            protocol::LoginStep::Failed {
                failed: reply.text.clone(),
            }
        }
        _ => return Received::Login,
    };
    match step {
        protocol::LoginStep::Url { url, instructions } => {
            if !world.resource::<Login>().urls.contains(&url) {
                if is_browsable(&url) {
                    let browse = world.resource::<Browser>().0;
                    browse(world, &url);
                }
                world.resource_mut::<Login>().urls.push(url);
            }
            if let Some(instructions) = instructions {
                world
                    .resource_mut::<Login>()
                    .instructions
                    .push(instructions);
            }
        }
        protocol::LoginStep::Code { code } => {
            world.resource_mut::<Login>().code = Some(code);
        }
        protocol::LoginStep::Prompt { prompt, secret } => {
            let mut login = world.resource_mut::<Login>();
            login.prompt = Some((prompt, secret));
            // A key typed before the sign-in asked answers it.
            if login.held && secret {
                login.held = false;
                reply(world);
            } else {
                login.answer.clear();
                world.resource_mut::<Welcome>().asking = false;
            }
        }
        protocol::LoginStep::Progress { progress } => {
            world.resource_mut::<Login>().progress = progress;
        }
        protocol::LoginStep::Done { .. } => {
            let provider = world.resource::<Login>().provider.clone();
            let abandoned = std::mem::take(
                &mut world.resource_mut::<Login>().abandoned,
            );
            *world.resource_mut::<Login>() = Login {
                provider,
                abandoned,
                ..Default::default()
            };
            read_models(world, true);
        }
        protocol::LoginStep::Failed { failed } => {
            // The provider and the key stay, to try again with.
            let mut login = world.resource_mut::<Login>();
            *login = Login {
                provider: login.provider.take(),
                abandoned: std::mem::take(&mut login.abandoned),
                answer: std::mem::take(&mut login.answer),
                ..Default::default()
            };
            let mut welcome = world.resource_mut::<Welcome>();
            welcome.asking = false;
            welcome.problem = Some(failed);
        }
    }
    Received::Login
}

fn is_browsable(text: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(text) else {
        return false;
    };
    match url.scheme() {
        "https" => true,
        "http" => url.host_str().is_some_and(|host| {
            host == "localhost"
                || host
                    .trim_matches(['[', ']'])
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
        }),
        _ => false,
    }
}

fn prompt() -> AnyView {
    keyed(
        resource::<Login, _>(|login| login.prompt.clone()),
        |prompt| {
            let Some((words, secret)) = prompt else {
                return frame().boxed();
            };
            let answer =
                resource::<Login, _>(|login| login.answer.clone());
            let answered = |world: &mut World, answer| {
                world.resource_mut::<Login>().answer = answer;
            };
            let field = if *secret {
                secret_field(answer, answered).boxed()
            } else {
                text_field(answer, answered).rules(text_input).boxed()
            };
            group((
                said(words.clone()),
                field,
                note(
                    "This answer goes only to the server's login. \
                     Credentials stay on the server.",
                ),
                good_action(Glyph::Check, "Send", commit),
            ))
            .boxed()
        },
    )
    .within(bare())
    .boxed()
}

/// The page for a key: the providers to pick it for, the field it is
/// pasted in under the name of the one picked, why the last try
/// failed under it, and the button that sends it once it is typed.
pub(super) fn key_page() -> AnyView {
    let names = keyed(
        derived(|world: &World| {
            (
                world.resource::<Providers>().0.clone(),
                keyed_provider(world),
            )
        })
        .when(every_frame()),
        |(providers, chosen)| {
            let keyed = providers
                .iter()
                .filter(|provider| {
                    provider
                        .methods
                        .contains(&protocol::LoginMethod::Key)
                })
                .cloned()
                .collect::<Vec<_>>();
            let picked = keyed
                .iter()
                .position(|provider| {
                    Some(&provider.id) == chosen.as_ref()
                })
                .unwrap_or_default();
            // As many as a provider's list may hold, so they wrap
            // into rows rather than share one track.
            let options = keyed
                .iter()
                .enumerate()
                .map(|(at, provider)| {
                    let (id, name) =
                        (provider.id.clone(), provider.name.clone());
                    button(
                        row((
                            mark(&provider.id, ICON),
                            label(provider.name.clone()).wrap(false),
                        ))
                        .gap(6.0)
                        .align(AlignItems::Center),
                    )
                    .rules(choice(at == picked))
                    .on_activate(move |world| {
                        world.resource_mut::<Login>().provider =
                            Some(id.clone());
                        world.resource_mut::<Welcome>().from =
                            name.clone();
                    })
                    .boxed()
                })
                .collect();
            close((section("Provider"), buttons(options))).boxed()
        },
    )
    .within(bare());
    // The key's name, and under it the provider's own words where
    // they say more than its name.
    let named = keyed(
        derived(|world: &World| {
            let chosen = keyed_provider(world);
            world
                .resource::<Providers>()
                .0
                .iter()
                .find(|provider| {
                    Some(&provider.id) == chosen.as_ref()
                })
                .map(|provider| {
                    (provider.name.clone(), provider.label.clone())
                })
        })
        .when(every_frame()),
        |named| {
            let Some((name, says)) = named else {
                return section("Your provider's API key").boxed();
            };
            let says = match says != name {
                true => note(says.clone()).boxed(),
                false => frame().boxed(),
            };
            close((section(format!("{name} API key")), says)).boxed()
        },
    )
    .within(bare());
    let field = hinted(
        secret_field(
            resource::<Login, _>(|login| login.answer.clone()),
            |world: &mut World, answer| {
                world.resource_mut::<Login>().answer = answer;
            },
        )
        .tagged(KeyField),
        "Paste the key from the provider's site",
    );
    // The reason the last key failed, once one was sent.
    let under = keyed(
        resource::<Welcome, _>(|welcome| {
            (welcome.asking, welcome.problem.clone())
        }),
        |(asking, problem)| match problem {
            _ if *asking => {
                said("Waiting for the provider...").boxed()
            }
            Some(why) => label(why.clone())
                .tone(Tone::Critical)
                .rules(small_rules)
                .boxed(),
            None => note(
                "The key goes only to the server, which keeps it.",
            )
            .boxed(),
        },
    )
    .within(bare());
    // Pressed only once the field has text; see `hold_send`.
    let send =
        row((good_action(Glyph::Check, "Continue", commit)
            .tagged(KeySend),));
    parts((
        head(Glyph::Key, "Use a key", 2),
        names,
        close((named, field, under)),
        send,
    ))
    .boxed()
}

/// The field a key is pasted in.
#[derive(Component)]
pub(super) struct KeyField;

/// The button that sends a key.
#[derive(Component)]
pub(super) struct KeySend;

/// Holds back the button that sends a key while its field, as it is
/// typed in, is empty or still holds the key that was refused, and
/// marks the field [`Refused`] until it is changed.
pub(super) fn hold_send(
    mut commands: Commands,
    sends: Query<(Entity, Has<InteractionDisabled>), With<KeySend>>,
    fields: Query<Entity, With<KeyField>>,
    under: Query<&Children>,
    texts: Query<(&EditableText, &ChildOf)>,
    refusals: Query<Has<Refused>>,
    welcome: Res<Welcome>,
    login: Option<Res<Login>>,
) {
    let refused = welcome.problem.is_some() && !welcome.asking;
    let sent = login.map(|login| login.answer.clone());
    let mut empty = true;
    for field in &fields {
        for part in under.iter_descendants(field) {
            let Ok((text, frame)) = texts.get(part) else {
                continue;
            };
            // The look is the field's frame's, over its text.
            let frame = frame.parent();
            let marked = refusals.get(frame).unwrap_or_default();
            let typed = text.value().to_string();
            let still = refused
                && !typed.trim().is_empty()
                && Some(&typed) == sent.as_ref();
            match (still, marked) {
                (true, false) => {
                    commands.entity(frame).insert(Refused);
                }
                (false, true) => {
                    commands.entity(frame).remove::<Refused>();
                }
                _ => {}
            }
            empty &= typed.trim().is_empty() || still;
        }
    }
    for (send, held) in &sends {
        match (empty, held) {
            (true, false) => {
                commands.entity(send).insert(InteractionDisabled);
            }
            (false, true) => {
                commands.entity(send).remove::<InteractionDisabled>();
            }
            _ => {}
        }
    }
}

pub(super) fn sign_in_page() -> AnyView {
    let steps = keyed(
        resource::<Login, _>(|login| {
            (
                login.urls.clone(),
                login.instructions.clone(),
                login.code.clone(),
                login.progress.clone(),
            )
        }),
        |(urls, instructions, code, progress)| {
            let mut rows = urls
                .iter()
                .chain(instructions)
                .map(|text| said(text.clone()))
                .collect::<Vec<_>>();
            if let Some(code) = code {
                rows.push(label(code.clone()).bold(true).boxed());
            }
            rows.push(said(progress.clone()));
            group(rows).boxed()
        },
    )
    .within(bare());
    parts((
        head(Glyph::Globe, "Sign in", 2),
        steps,
        prompt(),
        news("Waiting for sign-in..."),
        row((good_action(Glyph::Gone, "Cancel sign-in", |world| {
            go(world, Step::Choice);
        }),))
        .align(AlignItems::Center),
    ))
    .boxed()
}
