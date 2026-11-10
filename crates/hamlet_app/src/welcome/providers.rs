//! Provider choices and the server's login steps.

use std::collections::BTreeSet;

use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::{ChildOf, Children};
use bevy::ecs::observer::On;
use bevy::ecs::query::{Added, Has, With};
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Local, Query, Res};
use bevy::ecs::world::World;
use bevy::input::keyboard::{KeyCode, KeyboardInput};
use bevy::input_focus::{FocusCause, FocusedInput, InputFocus};
use bevy::math::Vec2;
use bevy::text::EditableText;
use bevy::ui::{
    AlignItems, ComputedNode, InteractionDisabled, ScrollPosition,
    UiGlobalTransform, UiRect, px,
};
use bevy_fynix::scroll::ScrollGoal;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, frame, label, row, text_field,
};
use bevy_fynix::{
    ScopedExt, ViewExt, derived, every_frame, keyed, resource,
};
use hamlet_client::messages::Response;
use hamlet_client::outbox::Outbox;
use hamlet_client::realm::Providers;
use hamlet_protocol::client as protocol;
use hamlet_ui::styles::{
    ICON, RailRow, Refused, bare, close, good_action, group,
    hairline, note, parts, rail_item, scroll_area, small_rules,
    text_input, upright,
};
use hamlet_ui::widgets::hint::hinted;
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::{AnyView, built, with_cx};

use super::pages::{head, linked, news, said};
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

/// The providers the key page offers first, in this order, of those
/// that take a key.
const POPULAR: [&str; 4] =
    ["openrouter", "anthropic", "openai", "google"];

/// The providers that take a key, each by its id and name, as the
/// key page lists them.
#[derive(Clone, PartialEq)]
struct Offered {
    /// The popular ones, while nothing is searched for.
    popular: Vec<(String, String)>,
    /// The rest A to Z, or what the search finds, best first.
    rest: Vec<(String, String)>,
}

impl Offered {
    fn iter(&self) -> impl Iterator<Item = &(String, String)> {
        self.popular.iter().chain(&self.rest)
    }
}

/// The providers that take a key, narrowed to those whose name holds
/// every word of `search`.
fn offered(world: &World, search: &str) -> Offered {
    let keyed = world
        .resource::<Providers>()
        .0
        .iter()
        .filter(|p| p.methods.contains(&protocol::LoginMethod::Key))
        .map(|p| (p.id.clone(), p.name.clone()));
    let search = search.trim().to_lowercase();
    if !search.is_empty() {
        let mut found = keyed
            .filter_map(|p| Some((rank(&p.1, &search)?, p)))
            .collect::<Vec<_>>();
        found.sort_by_key(|(score, (_, name))| {
            (*score, name.to_lowercase())
        });
        return Offered {
            popular: Vec::new(),
            rest: found.into_iter().map(|(_, p)| p).collect(),
        };
    }
    let (mut popular, mut rest) = keyed
        .partition::<Vec<_>, _>(|(id, _)| POPULAR.contains(&&**id));
    popular.sort_by_key(|(id, _)| {
        POPULAR.iter().position(|popular| popular == id)
    });
    rest.sort_by_key(|(_, name)| name.to_lowercase());
    Offered { popular, rest }
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

/// The provider a key is for: the one chosen, else the first offered.
fn keyed_provider(world: &World) -> Option<String> {
    if let Some(chosen) = &world.resource::<Login>().provider {
        return Some(chosen.clone());
    }
    offered(world, "").iter().next().map(|(id, _)| id.clone())
}

fn choose(world: &mut World, (id, name): &(String, String)) {
    world.resource_mut::<Login>().provider = Some(id.clone());
    world.resource_mut::<Welcome>().from = name.clone();
}

/// Narrows the key page's providers to `text`, choosing the first
/// found when the one chosen is not.
pub fn search(world: &mut World, text: &str) {
    if world.resource::<Welcome>().narrow == text {
        return;
    }
    world.resource_mut::<Welcome>().narrow = text.to_string();
    let found = offered(world, text);
    let chosen = keyed_provider(world);
    if !found.iter().any(|(id, _)| Some(id) == chosen.as_ref())
        && let Some(first) = found.iter().next()
    {
        choose(world, first);
    }
}

/// Chooses the provider `by` places along the list from the one
/// chosen, or the first when it is not listed.
fn step(world: &mut World, by: isize) {
    let listed = offered(world, &world.resource::<Welcome>().narrow)
        .iter()
        .cloned()
        .collect::<Vec<_>>();
    let chosen = keyed_provider(world);
    let to = listed
        .iter()
        .position(|(id, _)| Some(id) == chosen.as_ref())
        .map_or(0, |at| {
            at.saturating_add_signed(by).min(listed.len() - 1)
        });
    if let Some(provider) = listed.get(to) {
        choose(world, provider);
    }
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

/// The width of the providers on the key page.
const PROVIDERS: f32 = 264.0;

/// The height of the providers and their search, whatever the list
/// holds.
const TALL: f32 = 300.0;

/// The page for a key: the providers to pick it for, searched and
/// scrolled, beside the field it is pasted in under the one picked,
/// why the last try failed under it, and the button that sends it
/// once it is typed.
pub(super) fn key_page() -> AnyView {
    // Built once, so it keeps its focus and text as the list changes.
    let search = built(|cx| {
        let narrow = cx.world.resource::<Welcome>().narrow.clone();
        cx.build(crate::search::search(
            "Search providers",
            narrow,
            |world, text| search(world, &text),
        ))
    });
    let list = keyed(
        derived(|world: &World| {
            let narrow = &world.resource::<Welcome>().narrow;
            (offered(world, narrow), keyed_provider(world))
        })
        .when(every_frame()),
        |(offered, chosen)| {
            if offered.iter().next().is_none() {
                return column((label("No providers found")
                    .tone(Tone::Dim)
                    .rules(small_rules),))
                .padding(UiRect::all(px(8.0)))
                .boxed();
            }
            let row = |provider: &(String, String)| {
                let (id, name) = provider;
                let is_chosen = Some(id) == chosen.as_ref();
                let picked = provider.clone();
                let state = if is_chosen {
                    RailRow::Chosen
                } else {
                    RailRow::Plain
                };
                let face = mark(id, ICON);
                let item = rail_item(face, name.clone(), state)
                    .on_activate(move |world| choose(world, &picked));
                if is_chosen {
                    item.tagged(ChosenProvider).boxed()
                } else {
                    item.boxed()
                }
            };
            let mut rows =
                offered.popular.iter().map(row).collect::<Vec<_>>();
            if !offered.popular.is_empty() && !offered.rest.is_empty()
            {
                rows.push(
                    with_cx(|cx| hairline(cx.theme().palette.gold_6))
                        .boxed(),
                );
            }
            rows.extend(offered.rest.iter().map(row));
            column(rows).gap(2.0).boxed()
        },
    )
    .within(bare());
    let providers = column((
        column((search,)).tagged(ProviderSearch),
        scroll_area((list,)),
    ))
    .gap(8.0)
    .width(px(PROVIDERS))
    .min_width(px(PROVIDERS))
    .height(px(TALL));
    // The provider's mark and name, and under it where it hands keys
    // out.
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
        .when(every_frame()),
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
    // Pressed only once the field has text; see `hold_send`.
    let send =
        row((good_action(Glyph::Check, "Continue", commit)
            .tagged(KeySend),));
    let key = with_cx(move |cx| {
        column((named, close((field, under)), send))
            .gap(cx.theme().space.gap)
            .grow(1.0)
            .min_width(px(0.0))
    });
    parts((
        head(Glyph::Key, "Use a key", 2),
        with_cx(move |cx| {
            row((providers, upright(), key))
                .gap(cx.theme().space.section)
                .align(AlignItems::Stretch)
        }),
    ))
    .boxed()
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

/// Around the search of the key page's providers.
#[derive(Component)]
pub(super) struct ProviderSearch;

/// On the provider chosen in the key page's list.
#[derive(Component)]
pub(super) struct ChosenProvider;

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

/// Moves the choice of provider with Up and Down in its search, and
/// the focus to the key's field with Enter.
pub(super) fn search_keys(
    key: On<FocusedInput<KeyboardInput>>,
    texts: Query<(), With<EditableText>>,
    parents: Query<&ChildOf>,
    searches: Query<(), With<ProviderSearch>>,
    mut commands: Commands,
) {
    let field = key.focused_entity;
    // Once, at the field, and not again at each node it bubbles up.
    if !texts.contains(field)
        || !key.input.state.is_pressed()
        || !parents
            .iter_ancestors(field)
            .any(|up| searches.contains(up))
    {
        return;
    }
    match key.input.key_code {
        KeyCode::ArrowUp => commands.queue(|world: &mut World| {
            step(world, -1);
        }),
        KeyCode::ArrowDown => commands.queue(|world: &mut World| {
            step(world, 1);
        }),
        KeyCode::Enter => commands.queue(focus_field::<KeyField>),
        _ => {}
    }
}

/// Scrolls the providers to the one chosen once it is laid out, each
/// time another row is chosen or the list is built again.
pub(super) fn reveal_chosen(
    chosen: Query<
        (Entity, &ComputedNode, &UiGlobalTransform),
        With<ChosenProvider>,
    >,
    parents: Query<&ChildOf>,
    mut areas: Query<(
        &ComputedNode,
        &UiGlobalTransform,
        &mut ScrollPosition,
        &mut ScrollGoal,
    )>,
    mut shown: Local<Option<Entity>>,
) {
    let Ok((row, laid, at)) = chosen.single() else {
        return;
    };
    if *shown == Some(row) || laid.size().y <= 0.0 {
        return;
    }
    let Some(area) =
        parents.iter_ancestors(row).find(|up| areas.contains(*up))
    else {
        return;
    };
    let Ok((area_laid, area_at, mut position, mut goal)) =
        areas.get_mut(area)
    else {
        return;
    };
    *shown = Some(row);
    let scale = laid.inverse_scale_factor();
    let top = |laid: &ComputedNode, at: &UiGlobalTransform| {
        at.translation.y - laid.size().y / 2.0
    };
    let from = (top(laid, at) - top(area_laid, area_at)) * scale;
    let tall = laid.size().y * scale;
    let room = area_laid.size().y * scale;
    let by = if from < 0.0 {
        from
    } else if from + tall > room {
        from + tall - room
    } else {
        return;
    };
    let most = ((area_laid.content_size().y - area_laid.size().y)
        * scale)
        .max(0.0);
    goal.scroll_by(
        &mut position,
        Vec2::new(0.0, by),
        Vec2::new(0.0, most),
        false,
    );
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
