//! Provider choices and the server's login steps.

use std::collections::BTreeSet;

use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use bevy::ui::AlignItems;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, label, row, text_field,
};
use bevy_fynix::{ScopedExt, ViewExt, keyed, resource};
use hamlet_client::messages::Response;
use hamlet_client::outbox::Outbox;
use hamlet_client::realm::Providers;
use hamlet_protocol::client as protocol;
use hamlet_ui::AnyView;
use hamlet_ui::styles::{
    bare, buttons, close, good_action, group, lettered, note, parts,
    section, text_input,
};
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::icons::Glyph;

use super::pages::{head, news, said};
use super::{Step, Welcome, commit, go, read_models};
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

/// The spend limit in dollars OpenRouter is asked to put on the key
/// it makes for the hamlet.
#[derive(Resource, Clone, PartialEq, Debug)]
pub(crate) struct SpendLimit(pub String);

impl Default for SpendLimit {
    fn default() -> Self {
        Self("10".into())
    }
}

/// The dollars of a spend limit typed, if it is one.
fn dollars(typed: &str) -> Option<f64> {
    let typed = typed.trim().trim_start_matches('$').trim();
    typed
        .parse::<f64>()
        .ok()
        .filter(|dollars| dollars.is_finite() && *dollars > 0.0)
}

/// OpenRouter's page that makes a key, `url`, asked to fill in the
/// spend limit chosen.
fn limited(url: &str, limit: &str) -> String {
    let (Ok(mut parsed), Some(dollars)) =
        (reqwest::Url::parse(url), dollars(limit))
    else {
        return url.to_string();
    };
    if parsed.host_str() != Some("openrouter.ai")
        || parsed.path() != "/auth"
    {
        return url.to_string();
    }
    parsed
        .query_pairs_mut()
        .append_pair("limit", &dollars.to_string());
    parsed.to_string()
}

/// Starts connecting OpenRouter with the spend limit typed, or says
/// why it will not do.
pub(super) fn connect(world: &mut World) {
    if dollars(&world.resource::<SpendLimit>().0).is_none() {
        let mut welcome = world.resource_mut::<Welcome>();
        welcome.problem =
            Some("Type a spend limit in dollars, such as 10.".into());
        return;
    }
    let known = world.resource::<Providers>().0.iter().any(|known| {
        known.id == "openrouter"
            && known
                .methods
                .contains(&protocol::LoginMethod::Subscription)
    });
    if !known {
        world.resource_mut::<Welcome>().problem =
            Some("This home cannot connect OpenRouter.".into());
        return;
    }
    start(world, "openrouter", false);
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
    answer: String,
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

pub(super) fn reply(world: &mut World) {
    let mut login = world.resource_mut::<Login>();
    let Some(id) =
        login.id.clone().filter(|_| login.prompt.is_some())
    else {
        return;
    };
    let answer = std::mem::take(&mut login.answer);
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
            let url =
                match world.resource::<Login>().provider.as_deref() {
                    Some("openrouter") => limited(
                        &url,
                        &world.resource::<SpendLimit>().0,
                    ),
                    _ => url,
                };
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
            login.answer.clear();
            world.resource_mut::<Welcome>().asking = false;
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
            let abandoned = std::mem::take(
                &mut world.resource_mut::<Login>().abandoned,
            );
            *world.resource_mut::<Login>() = Login {
                abandoned,
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
                return bevy_fynix::views::frame().boxed();
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
                good_action(Glyph::Check, "SEND", commit),
            ))
            .boxed()
        },
    )
    .within(bare())
    .boxed()
}

pub(super) fn key_page() -> AnyView {
    let names = keyed(
        resource::<Providers, _>(|providers| providers.0.clone()),
        |providers| {
            let choices = providers
                .iter()
                .filter(|provider| {
                    provider
                        .methods
                        .contains(&protocol::LoginMethod::Key)
                })
                .map(|provider| {
                    let id = provider.id.clone();
                    close((
                        lettered(provider.name.clone())
                            .tagged(Variant::Default)
                            .on_activate(move |world| {
                                start(world, &id, true);
                            }),
                        note(provider.label.clone()),
                    ))
                    .boxed()
                })
                .collect::<Vec<_>>();
            buttons(choices).boxed()
        },
    )
    .within(bare());
    parts((
        head(Glyph::Globe, "USE A KEY"),
        names,
        prompt(),
        news("Waiting for the provider..."),
    ))
    .boxed()
}

/// The spend limit chosen before OpenRouter opens.
pub(super) fn limit_page() -> AnyView {
    let value = resource::<SpendLimit, _>(|limit| limit.0.clone());
    let typed = |world: &mut World, text: String| {
        world.resource_mut::<SpendLimit>().0 = text;
    };
    parts((
        head(Glyph::Signal, "CONNECT OPENROUTER"),
        said(
            "OpenRouter makes a key for your hamlet, and you pay \
             OpenRouter for what it uses. Choose the most the key \
             may spend.",
        ),
        close((
            section("SPEND LIMIT, IN DOLLARS"),
            text_field(value, typed).rules(text_input),
        )),
        news("Opening OpenRouter..."),
        row((good_action(Glyph::Signal, "CONTINUE", |world| {
            crate::account::commit(world, connect);
        }),))
        .align(AlignItems::Center),
    ))
    .boxed()
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
    let head = keyed(
        resource::<Login, _>(|login| {
            login.provider.as_deref() == Some("openrouter")
        }),
        |openrouter| match openrouter {
            true => group((
                head(Glyph::Signal, "CONNECT OPENROUTER"),
                said(
                    "Approve the key in your browser. OpenRouter \
                     shows the spend limit again there, and you can \
                     change it later on OpenRouter.",
                ),
            ))
            .boxed(),
            false => head(Glyph::Signal, "SIGN IN").boxed(),
        },
    )
    .within(bare());
    parts((
        head,
        steps,
        prompt(),
        news("Waiting for sign-in..."),
        row((good_action(Glyph::Gone, "CANCEL SIGN-IN", |world| {
            go(world, Step::Choice);
        }),))
        .align(AlignItems::Center),
    ))
    .boxed()
}

#[cfg(test)]
mod tests {
    use super::{dollars, limited};

    #[test]
    fn a_spend_limit_is_asked_of_openrouters_key_page_alone() {
        let page = "https://openrouter.ai/auth?code_challenge=c";
        assert_eq!(limited(page, "$10"), format!("{page}&limit=10"));
        assert_eq!(limited(page, "2.5"), format!("{page}&limit=2.5"));
        assert_eq!(limited(page, "none"), page);
        let other = "https://login.example/auth?x=1";
        assert_eq!(limited(other, "10"), other);
        assert_eq!(
            (dollars("0"), dollars("-1"), dollars("")),
            (None, None, None)
        );
    }
}
