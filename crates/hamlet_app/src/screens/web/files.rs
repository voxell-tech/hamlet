//! The Files panel: the folder a hamlet works on, in the web IDE the
//! server starts on it (`hamlet_server::web_ide`), drawn as a Web
//! panel's page is. It shows the hamlet of the agent the chat is
//! with, or the one chosen in its tab until the chat moves. Its tab
//! names the hamlet and opens the list of them; the bar ends with
//! opening the folder again.

mod context;

use bevy::app::{App, Update};
use bevy::color::Alpha;
use bevy::ecs::entity::Entity;
use bevy::ecs::name::Name;
use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, Local, Res, ResMut};
use bevy::ecs::world::World;
use bevy::math::{Rect, Vec2};
use bevy::ui::{AlignItems, UiRect, px};
use bevy::ui_widgets::popover::{
    PopoverAlign, PopoverPlacement, PopoverSide,
};
use bevy_cef::prelude::{HostEmitEvent, Receive};
use bevy_fynix::dock::{DockTree, DockWindowKind};
use bevy_fynix::tokens::{MotionTokens, SurfaceTokens, Tone};
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, button, column, frame,
    label, overlay, popup, row,
};
use bevy_fynix::{
    ScopedExt, ViewExt, derived, every_frame, keyed, resource, style,
};
use hamlet_client::Connection;
use hamlet_client::connection::Contact;
use hamlet_client::messages::Response;
use hamlet_client::outbox::Outbox;
use hamlet_client::realm::{HamletId, LivesIn, Works};
use hamlet_protocol::client::{
    ClientMessage, ReplyKind, RequestId, ServerMessage, WebIde,
};
use hamlet_ui::styles::{Fills, ICON, hairline, link_row, note};
use hamlet_ui::theme::HamletTheme;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::widgets::platform::DisplayText;
use hamlet_ui::{AnyView, Cx, View, with_cx};
use serde::Deserialize;
use serde_json::{Value, json};

use super::panel::{self, PanelId};
use super::{Addresses, FILES, page};
use crate::convo::Convo;
use crate::dock::{PanelAppExt, flush_panel, on_a_desk};
use crate::menu::rect_of;

pub(super) fn plugin(app: &mut App) {
    // Closed until asked for from a tab bar's plus.
    app.add_panel(FILES, window())
        .init_resource::<Browsing>()
        .init_resource::<Asked>()
        .init_resource::<Picking>()
        .init_resource::<Told>()
        .add_plugins(context::plugin)
        .add_observer(answered)
        .add_observer(heard)
        .add_systems(
            Update,
            (
                (follow_chat, forget_on_reconnect, ask)
                    .chain()
                    .before(page::Opening),
                tell_heading,
            ),
        );
}

fn window() -> DockWindowKind<HamletTheme> {
    DockWindowKind::new("FILES", || flush_panel(contents()))
        .icon(Glyph::Folder.image(ICON))
        .actions(|| actions().boxed())
}

/// The hamlet chosen above the panel, until the chat moves.
#[derive(Resource, Default, Clone, Copy, PartialEq)]
struct Browsing(Option<Entity>);

/// Where the list of hamlets hangs from, while it is open.
#[derive(Resource, Default)]
struct Picking(Option<Rect>);

/// The web IDE asked for, and how the server answered.
#[derive(Resource, Default)]
struct Asked {
    /// The hamlet and the folder it was asked for.
    for_: Option<(String, Option<String>)>,
    /// The request whose answer is awaited.
    pending: Option<RequestId>,
    /// Why there is none, as the server said.
    refusal: Option<String>,
}

fn contents() -> impl View {
    column((panel::body(&id(), blank), picker()))
        .fills_width()
        .fills_height()
        .gap(0.0)
}

fn id() -> PanelId {
    PanelId(FILES.to_owned())
}

/// The name of the hamlet shown, and the agent whose chat it follows
/// while it follows one.
fn heading(world: &World) -> Option<(String, Option<String>)> {
    let hamlet = shown(world)?;
    let name = world.get::<Name>(hamlet)?.as_str().to_owned();
    let following = match world.resource::<Browsing>().0 {
        Some(_) => None,
        None => chat(world).and_then(|convo| convo.with.clone()),
    };
    Some((name, following))
}

/// What the web IDE's page asks of the window, beside a context.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum Asks {
    /// The heading again, as the page has just started.
    HamletHeading {},
    /// The list of hamlets, under the press of the hamlet's name that
    /// is at this box, in the page's pixels.
    HamletPick {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        #[serde(rename = "viewWidth")]
        view_width: f32,
        #[serde(rename = "viewHeight")]
        view_height: f32,
    },
}

/// Opens the list of hamlets where the page's press of the hamlet's
/// name is, or tells the page the heading again.
fn heard(emitted: On<Receive<Value>>, mut commands: Commands) {
    let Ok(asks) =
        serde_json::from_value::<Asks>(emitted.payload.clone())
    else {
        return;
    };
    let webview = emitted.webview;
    commands.queue(move |world: &mut World| {
        if world
            .get::<page::Page>(webview)
            .is_none_or(|p| p.0 != id())
        {
            return;
        }
        match asks {
            Asks::HamletHeading {} => {
                world.resource_mut::<Told>().0 = None;
            }
            Asks::HamletPick {
                x,
                y,
                width,
                height,
                view_width,
                view_height,
            } => {
                // The list hangs in the panel, whose corner the
                // page's is.
                let Some(on_page) =
                    rect_of(world, webview).map(|page| {
                        Rect::from_corners(Vec2::ZERO, page.size())
                    })
                else {
                    return;
                };
                let part = Rect::new(x, y, x + width, y + height);
                let view = Vec2::new(view_width, view_height);
                if let Some(anchor) =
                    context::on_the_page(on_page, part, view)
                {
                    world.resource_mut::<Picking>().0 = Some(anchor);
                }
            }
        }
    });
}

/// The heading the page was last told.
#[derive(Resource, Default)]
struct Told(Option<Option<(String, Option<String>)>>);

/// Tells the web IDE's page the hamlet shown and whom the panel
/// follows, once it changes, for the press of its name under the
/// tree.
fn tell_heading(world: &mut World) {
    let heading = heading(world);
    if world.resource::<Told>().0.as_ref() == Some(&heading) {
        return;
    }
    let Some(page) = page::of(world, &id()) else {
        return;
    };
    let (name, following) = heading.clone().unzip();
    world.trigger(HostEmitEvent::new(
        page,
        "hamletHeading",
        &json!({"name": name, "following": following.flatten()}),
    ));
    world.resource_mut::<Told>().0 = Some(heading);
}

/// The bar's end while the panel shows: opening the folder again.
fn actions() -> impl View {
    with_cx(|cx| {
        let gap = cx.theme().space.gap;
        row((reload(),))
            .padding(UiRect::right(px(gap / 2.0)))
            .align(AlignItems::Center)
    })
}

fn reload() -> AnyView {
    button(glyph(Glyph::Restore, ICON).tone(Tone::Dim))
        .padding(UiRect::all(px(4.0)))
        .rules(link_row)
        .on_activate_with(|world, _| {
            *world.resource_mut::<Asked>() = Asked::default();
        })
        .tooltip(|| note("Open the folder again"))
        .boxed()
}

/// A press in the bar or the list: words in the body face's weight,
/// dim at rest, with a faint fill under the pointer.
fn quiet_link(cx: &mut Cx) {
    style::<HamletTheme>()
        .frame(|f, t| {
            f.fill(t.palette.button.with_alpha(0.0)).radius(4.0)
        })
        .label(|l, t| {
            l.tone(Tone::Dim).size(t.text.small).bold(false)
        })
        .icon(|i, _| i.tone(Tone::Dim))
        .hovered(|s| {
            s.frame(|f, t| f.fill(t.palette.button)).tone(Tone::Body)
        })
        .pressed(|s| {
            s.frame(|f, t| f.fill(t.pressed())).tone(Tone::Body)
        })
        .transition(HamletTheme::interact)
        .apply(cx);
}

/// The list of hamlets, hanging from the tab while it is open:
/// following the chat, then each hamlet by name with its folder.
fn picker() -> impl View {
    keyed(resource::<Picking, _>(|picking| picking.0), |anchor| {
        let Some(anchor) = *anchor else {
            return frame().boxed();
        };
        // Over the hamlet's name at the foot of the tree.
        popup(anchor, (choices(),))
            .placements(vec![PopoverPlacement {
                side: PopoverSide::Top,
                align: PopoverAlign::Start,
                gap: 6.0,
            }])
            .on_dismiss(|world, _| {
                world.resource_mut::<Picking>().0 = None;
            })
            .min_width(px(340.0))
            .padding(UiRect::all(px(8.0)))
            .boxed()
    })
    .within(overlay(()))
}

fn choices() -> impl View {
    let list =
        derived(|world: &World| listed(world)).when(every_frame());
    keyed(list, |list| {
        let Listed {
            following,
            agent,
            shown,
            hamlets,
        } = list.clone();
        let follow_words = match &agent {
            Some(agent) => format!(
                "Show the hamlet of {agent}, whom you chat with"
            ),
            None => "Show the hamlet of the agent you chat with"
                .to_owned(),
        };
        let mut rows = vec![
            choice(
                following,
                label("Follow your chat").bold(true).boxed(),
                note(follow_words).boxed(),
                |world| {
                    world.resource_mut::<Browsing>().0 = None;
                    world.resource_mut::<Picking>().0 = None;
                },
            ),
            with_cx(|cx| {
                hairline(cx.theme().palette.hairline)
                    .margin(UiRect::vertical(px(4.0)))
            })
            .boxed(),
        ];
        for (hamlet, name, folder) in hamlets {
            let current = shown == Some(hamlet);
            rows.push(choice(
                current && !following,
                label(name).wrap(false).tagged(DisplayText).boxed(),
                note(folder).boxed(),
                move |world| {
                    world.resource_mut::<Browsing>().0 = Some(hamlet);
                    world.resource_mut::<Picking>().0 = None;
                },
            ));
        }
        column(rows).gap(2.0).fills_width().boxed()
    })
}

/// One row of the list: a tick while it is the choice, its name and
/// what it shows.
fn choice(
    ticked: bool,
    name: AnyView,
    detail: AnyView,
    act: impl Fn(&mut World) + Send + Sync + 'static,
) -> AnyView {
    let tick = match ticked {
        true => glyph(Glyph::Check, ICON).tone(Tone::Accent).boxed(),
        false => frame().width(px(ICON)).height(px(ICON)).boxed(),
    };
    button(
        row((tick, column((name, detail)).gap(1.0)))
            .gap(10.0)
            .align(AlignItems::Center),
    )
    .padding(UiRect::axes(px(8.0), px(6.0)))
    .fills_width()
    .rules(quiet_link)
    .on_activate(act)
    .boxed()
}

/// What the list of hamlets shows.
#[derive(Clone, PartialEq)]
struct Listed {
    /// Whether the panel follows the chat.
    following: bool,
    /// The chat's agent, by name.
    agent: Option<String>,
    shown: Option<Entity>,
    /// Each hamlet, its name and its folder, by name.
    hamlets: Vec<(Entity, String, String)>,
}

fn listed(world: &World) -> Listed {
    let hamlets = hamlets(world)
        .into_iter()
        .map(|(hamlet, name)| {
            let folder = world
                .get::<Works>(hamlet)
                .and_then(|works| works.folder().cloned())
                .unwrap_or_else(|| "No folder yet".to_owned());
            (hamlet, name, folder)
        })
        .collect();
    Listed {
        following: world.resource::<Browsing>().0.is_none(),
        agent: chat(world).and_then(|convo| convo.with.clone()),
        shown: shown(world),
        hamlets,
    }
}

/// What the panel says while it has no page.
fn blank(world: &World) -> String {
    if let Some(refusal) = &world.resource::<Asked>().refusal {
        return refusal.clone();
    }
    match shown(world) {
        None => "No hamlet yet. Found one, and the folder it works \
                 on shows here."
            .to_owned(),
        Some(_) => "Opening the folder...".to_owned(),
    }
}

/// Every hamlet and its name, by name.
fn hamlets(world: &World) -> Vec<(Entity, String)> {
    let Some(mut query) =
        world.try_query::<(Entity, &HamletId, &Name)>()
    else {
        return Vec::new();
    };
    let mut hamlets = query
        .iter(world)
        .map(|(hamlet, _, name)| (hamlet, name.as_str().to_owned()))
        .collect::<Vec<_>>();
    hamlets.sort_by_key(|(_, name)| name.to_lowercase());
    hamlets
}

/// The hamlet shown: the one chosen while it stands, else the chat's
/// agent's, else the first by name.
fn shown(world: &World) -> Option<Entity> {
    let standing =
        |hamlet: &Entity| world.get::<HamletId>(*hamlet).is_some();
    let chats = chat(world)
        .and_then(|convo| world.get::<LivesIn>(convo.agent))
        .map(|lives| lives.0);
    world
        .resource::<Browsing>()
        .0
        .filter(standing)
        .or(chats.filter(standing))
        .or_else(|| hamlets(world).first().map(|(hamlet, _)| *hamlet))
}

/// The chat the panel follows: that of the Hub the chief works in.
fn chat(world: &World) -> Option<&Convo> {
    world.get::<Convo>(crate::hub::active(world)?)
}

/// Forgets the hamlet chosen once the chat moves to another agent.
fn follow_chat(world: &mut World, mut was: Local<Option<Entity>>) {
    let Some(agent) = chat(world).map(|convo| convo.agent) else {
        return;
    };
    if *was != Some(agent) {
        if was.is_some() && world.resource::<Browsing>().0.is_some() {
            world.resource_mut::<Browsing>().0 = None;
        }
        *was = Some(agent);
    }
}

/// Forgets the web IDE asked for when the connection comes back, for
/// a server started again has started it again elsewhere.
fn forget_on_reconnect(
    connection: Res<Connection>,
    mut asked: ResMut<Asked>,
    mut was: Local<bool>,
) {
    let connected = connection.state == Contact::Connected;
    if connected && !*was && asked.for_.is_some() {
        *asked = Asked::default();
    }
    *was = connected;
}

/// Asks for the web IDE of the hamlet shown while the panel is open,
/// and again when the hamlet or its folder changes. One asked for is
/// kept while the panel is closed, as its page is.
fn ask(world: &mut World) {
    let open = world
        .resource::<DockTree>()
        .find_leaf_with_window(FILES)
        .is_some()
        || on_a_desk(world, FILES);
    if !open {
        return;
    }
    let Some(hamlet) = shown(world) else {
        return;
    };
    let Some(id) =
        world.get::<HamletId>(hamlet).map(|id| id.0.clone())
    else {
        return;
    };
    let folder = world
        .get::<Works>(hamlet)
        .and_then(|works| works.folder().cloned());
    let wanted = Some((id.clone(), folder));
    if world.resource::<Asked>().for_ == wanted {
        return;
    }
    let request = world.resource_mut::<Outbox>().send(|request| {
        ClientMessage::WebIde(WebIde {
            hamlet: id,
            id: request,
        })
    });
    *world.resource_mut::<Asked>() = Asked {
        for_: wanted,
        pending: Some(request),
        refusal: None,
    };
    world.resource_mut::<Addresses>().0.remove(FILES);
}

/// Loads the web IDE the server gave, or keeps why it gave none.
fn answered(response: On<Response>, mut commands: Commands) {
    let to = response.to.clone();
    let message = response.message.clone();
    commands.queue(move |world: &mut World| {
        let mut asked = world.resource_mut::<Asked>();
        if asked.pending.as_ref() != Some(&to) {
            return;
        }
        asked.pending = None;
        match message {
            ServerMessage::Reply(reply)
                if reply.kind == ReplyKind::Address =>
            {
                world
                    .resource_mut::<Addresses>()
                    .0
                    .insert(FILES.to_owned(), reply.text.clone());
                page::navigate(world, &id(), reply.text);
            }
            ServerMessage::Reply(reply) => {
                asked.refusal = Some(reply.text);
            }
            ServerMessage::Fault(fault) => {
                asked.refusal = Some(fault.text);
            }
            _ => {}
        }
    });
}
