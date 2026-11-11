//! Provider choices and the server's login steps.

use std::collections::BTreeSet;

use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::{ChildOf, Children};
use bevy::ecs::query::{Added, Has, With};
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Query, Res};
use bevy::ecs::world::World;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::text::EditableText;
use bevy::ui::{AlignItems, InteractionDisabled};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, frame, label, row, text_field,
};
use bevy_fynix::{ScopedExt, ViewExt, derived, keyed, resource};
use hamlet_client::messages::Response;
use hamlet_client::outbox::Outbox;
use hamlet_client::realm::{Providers, Settings};
use hamlet_protocol::client as protocol;
use hamlet_ui::AnyView;
use hamlet_ui::styles::{
    Fills, ICON, Looks, Refused, bare, close, good_action, group,
    lettered, marked, note, parts, small_rules, text_input,
};
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::ellipsis::one_line;
use hamlet_ui::widgets::hint::hinted;
use hamlet_ui::widgets::icons::Glyph;

use super::pages::{card, footed, linked, news, said, to_providers};
use super::{Pick, Step, Welcome, commit, go, read_models};
use crate::models::mark;
use crate::roll::{either, moved};
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

/// Back to the providers after a reconnection, as a sign-in under
/// way was lost with the old connection.
pub(super) fn reconnected(world: &mut World) {
    if world.resource::<Login>().start.is_some() {
        go(world, Step::Provider);
    }
}

/// The providers signed in to with a plan, offered first, in this
/// order.
const PLANS: [&str; 3] = ["chatgpt", "claude", "openrouter"];

/// The providers that take a key offered after this computer, in
/// this order.
const POPULAR: [&str; 4] =
    ["anthropic", "openai", "google", "deepseek"];

/// How many tiles the provider step shows before Show more.
pub(super) const FEW: usize = 8;

/// A tile of the provider step.
#[derive(Clone, PartialEq)]
pub(crate) struct Tile {
    pub pick: Pick,
    /// The provider whose mark it shows; none for this computer.
    pub mark: Option<String>,
    pub name: String,
    pub way: Way,
}

/// How a tile's provider is reached, which groups the tiles.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Way {
    /// With no key: a plan signed in to, or this computer.
    Keyless,
    /// A popular provider, with a key.
    Key,
    /// Every other provider.
    Other,
}

impl Way {
    /// The heading over its tiles.
    pub(crate) fn heading(self) -> &'static str {
        match self {
            Self::Keyless => "No key needed",
            Self::Key => "With an API key",
            Self::Other => "More providers",
        }
    }
}

/// The provider step's tiles: the plans, this computer and the
/// popular keys first, then every other provider A to Z. With a
/// search, only those whose name holds every word of it, best first.
/// The server's endpoints are left to Settings.
pub(crate) fn tiles(world: &World) -> Vec<Tile> {
    let providers = &world.resource::<Providers>().0;
    let endpoints = &world.resource::<Settings>().0.endpoints;
    let offered = |id: &str, method| {
        providers.iter().find(|provider| {
            provider.id == id && provider.methods.contains(&method)
        })
    };
    let tile = |way| {
        move |provider: &protocol::ProviderView| Tile {
            pick: Pick::Provider(provider.id.clone()),
            mark: Some(provider.id.clone()),
            name: provider.name.clone(),
            way,
        }
    };
    let mut all = PLANS
        .iter()
        .filter_map(|id| {
            offered(id, protocol::LoginMethod::Subscription)
        })
        .map(tile(Way::Keyless))
        .collect::<Vec<_>>();
    all.push(Tile {
        pick: Pick::Local,
        mark: None,
        name: "This computer".into(),
        way: Way::Keyless,
    });
    all.extend(
        POPULAR
            .iter()
            .filter_map(|id| offered(id, protocol::LoginMethod::Key))
            .map(tile(Way::Key)),
    );
    let mut rest = providers
        .iter()
        .filter(|provider| {
            !provider.methods.is_empty()
                && !endpoints.iter().any(|at| at.name == provider.id)
                && !all.iter().any(|shown| {
                    shown.pick == Pick::Provider(provider.id.clone())
                })
        })
        .map(tile(Way::Other))
        .collect::<Vec<_>>();
    rest.sort_by_key(|shown| shown.name.to_lowercase());
    all.extend(rest);
    let seek = world.resource::<Welcome>().seek.trim().to_lowercase();
    if seek.is_empty() {
        return all;
    }
    let mut found = all
        .into_iter()
        .filter_map(|shown| Some((rank(&shown.name, &seek)?, shown)))
        .collect::<Vec<_>>();
    found.sort_by_key(|(score, shown)| {
        (*score, shown.name.to_lowercase())
    });
    found.into_iter().map(|(_, shown)| shown).collect()
}

/// How well `name` answers every word of `search`, best lowest; none
/// when a word is not in it.
fn rank(name: &str, search: &str) -> Option<usize> {
    let name = name.to_lowercase();
    search.split_whitespace().try_fold(0, |sum, word| {
        let score = if name.starts_with(word) {
            0
        } else if name.split(' ').any(|part| part.starts_with(word)) {
            1
        } else if name.contains(word) {
            2
        } else {
            return None;
        };
        Some(sum + score)
    })
}

/// The provider a key is for.
fn keyed_provider(world: &World) -> Option<String> {
    world.resource::<Login>().provider.clone()
}

/// Shows every provider, narrowed to those `text` finds, picking the
/// first found when the one picked is not.
pub fn search(world: &mut World, text: &str) {
    let mut welcome = world.resource_mut::<Welcome>();
    welcome.more = true;
    if welcome.seek == text {
        return;
    }
    welcome.seek = text.to_string();
    let found = tiles(world);
    let picked = world
        .resource::<Welcome>()
        .picked
        .clone()
        .or_else(|| found.first().map(|tile| tile.pick.clone()));
    if !found
        .iter()
        .any(|shown| Some(&shown.pick) == picked.as_ref())
        && let Some(first) = found.first()
    {
        world.resource_mut::<Welcome>().picked =
            Some(first.pick.clone());
    }
}

/// Goes on with the provider `id`: a sign-in when it has one, else
/// the page for its key.
pub(super) fn proceed(world: &mut World, id: &str) {
    let Some(provider) = world
        .resource::<Providers>()
        .0
        .iter()
        .find(|provider| provider.id == id)
        .cloned()
    else {
        return;
    };
    if provider
        .methods
        .contains(&protocol::LoginMethod::Subscription)
    {
        start(world, id, false);
    } else {
        ask_key(world, &provider);
    }
}

/// The page for a key to `provider`.
fn ask_key(world: &mut World, provider: &protocol::ProviderView) {
    go(world, Step::Key);
    world.resource_mut::<Login>().provider =
        Some(provider.id.clone());
    world.resource_mut::<Welcome>().from = provider.name.clone();
}

/// The provider signed in to, when it also takes a key.
fn keyed_too(world: &World) -> Option<protocol::ProviderView> {
    let id = world.resource::<Login>().provider.clone()?;
    world
        .resource::<Providers>()
        .0
        .iter()
        .find(|provider| {
            provider.id == id
                && provider
                    .methods
                    .contains(&protocol::LoginMethod::Key)
        })
        .cloned()
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

/// The page for the key of the provider picked: its mark and name,
/// where it hands keys out, the field the key is pasted in, why the
/// last try failed under it, and the button that sends it once it is
/// typed.
pub(super) fn key_page() -> AnyView {
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
                    (provider.id.clone(), provider.name.clone())
                })
        })
        .when(either(moved::<Login>(), moved::<Providers>())),
        |named| {
            let Some((id, name)) = named else {
                return frame().boxed();
            };
            let found = match keys_at(id) {
                Some((site, url)) => linked(
                    &format!("No key yet? Get one at {site}."),
                    site,
                    url,
                ),
                None => frame().boxed(),
            };
            close((
                row((mark(id, ICON), label(name.clone()).bold(true)))
                    .gap(8.0)
                    .align(AlignItems::Center),
                found,
            ))
            .boxed()
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
    card(
        2,
        parts((named, close((field, under)))),
        vec![
            to_providers("Back"),
            // Pressed only once the field has text; see `hold_send`.
            good_action(Glyph::Check, "Continue", commit)
                .tagged(KeySend)
                .boxed(),
        ],
    )
}

/// Where the provider `id` hands out keys: the address as shown, and
/// as opened.
fn keys_at(id: &str) -> Option<(&'static str, &'static str)> {
    Some(match id {
        "openrouter" => {
            ("openrouter.ai/keys", "https://openrouter.ai/keys")
        }
        "openai" => (
            "platform.openai.com/api-keys",
            "https://platform.openai.com/api-keys",
        ),
        "anthropic" => (
            "console.anthropic.com",
            "https://console.anthropic.com/settings/keys",
        ),
        "google" => (
            "aistudio.google.com/apikey",
            "https://aistudio.google.com/apikey",
        ),
        "groq" => {
            ("console.groq.com/keys", "https://console.groq.com/keys")
        }
        "deepseek" => (
            "platform.deepseek.com",
            "https://platform.deepseek.com/api_keys",
        ),
        "mistral" => (
            "console.mistral.ai",
            "https://console.mistral.ai/api-keys",
        ),
        _ => return None,
    })
}

/// The field a key is pasted in.
#[derive(Component)]
pub(super) struct KeyField;

/// The button that sends a key.
#[derive(Component)]
pub(super) struct KeySend;

/// Around the search of the providers.
#[derive(Component)]
pub(super) struct ProviderSearch;

/// Gives the focus to the field under the `C` shown.
fn focus_field<C: Component>(world: &mut World) {
    let mut under = world
        .query_filtered::<Entity, With<C>>()
        .iter(world)
        .collect::<Vec<_>>();
    while let Some(node) = under.pop() {
        if world.get::<EditableText>(node).is_some() {
            world
                .resource_mut::<InputFocus>()
                .set(node, FocusCause::Navigated);
            return;
        }
        if let Some(children) = world.get::<Children>(node) {
            under.extend(children.iter());
        }
    }
}

/// Gives the focus to the search of the providers once it is shown.
pub(super) fn focus_search(
    shown: Query<(), Added<ProviderSearch>>,
    mut commands: Commands,
) {
    if !shown.is_empty() {
        commands.queue(focus_field::<ProviderSearch>);
    }
}

/// Gives the focus to the key's field once it is shown.
pub(super) fn focus_key(
    shown: Query<(), Added<KeyField>>,
    mut commands: Commands,
) {
    if !shown.is_empty() {
        commands.queue(focus_field::<KeyField>);
    }
}

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

/// A sign-in address on one line, cut where it runs past the card,
/// with the ways to open it again and to copy it.
fn address(url: &str) -> AnyView {
    let (opened, copied) = (url.to_string(), url.to_string());
    let open = is_browsable(url).then(|| {
        marked(Glyph::Globe, "Open")
            .variant(Variant::Default)
            .on_activate(move |world| {
                let browse = world.resource::<Browser>().0;
                browse(world, &opened);
            })
            .boxed()
    });
    let copy = lettered("Copy")
        .variant(Variant::Default)
        .on_activate(move |world| {
            if let Some(mut clipboard) =
                world.get_resource_mut::<bevy::clipboard::Clipboard>()
                && let Err(e) = clipboard.set_text(copied.clone())
            {
                bevy::log::warn!("could not copy the address: {e:?}");
            }
        })
        .boxed();
    let mut parts = vec![
        row((one_line(label(url.to_string()).tone(Tone::Faint)),))
            .fills_width()
            .boxed(),
    ];
    parts.extend(open);
    parts.push(copy);
    row(parts).gap(8.0).align(AlignItems::Center).boxed()
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
                .map(|url| address(url))
                .collect::<Vec<_>>();
            rows.extend(
                instructions.iter().map(|text| said(text.clone())),
            );
            if let Some(code) = code {
                rows.push(label(code.clone()).bold(true).boxed());
            }
            rows.push(said(progress.clone()));
            group(rows).boxed()
        },
    )
    .within(bare());
    // A provider that also takes a key offers it instead.
    let instead = keyed(
        derived(keyed_too)
            .when(either(moved::<Login>(), moved::<Providers>())),
        |provider| match provider {
            Some(provider) => {
                let provider = provider.clone();
                marked(Glyph::Key, "Use a key instead")
                    .variant(Variant::Default)
                    .on_activate(move |world| {
                        ask_key(world, &provider);
                    })
                    .boxed()
            }
            None => frame().boxed(),
        },
    )
    .within(footed())
    .boxed();
    card(
        2,
        parts((steps, prompt(), news("Waiting for sign-in..."))),
        vec![to_providers("Cancel sign-in"), instead],
    )
}
