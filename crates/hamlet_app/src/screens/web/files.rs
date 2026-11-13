//! The Files panel: the folder a hamlet works on, in the web IDE the
//! server starts on it (`hamlet_server::web_ide`), drawn as a Web
//! panel's page is. It shows the hamlet of the agent the chat is
//! with, or the one chosen above it until the chat moves.

use bevy::app::{App, Update};
use bevy::ecs::entity::Entity;
use bevy::ecs::name::Name;
use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, Local, Res, ResMut};
use bevy::ecs::world::World;
use bevy::ui::{AlignItems, UiRect, percent, px};
use bevy_fynix::dock::{DockTree, DockWindowKind};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, button, column, dropdown,
    row,
};
use bevy_fynix::{ScopedExt, ViewExt, derived, every_frame, keyed};
use hamlet_client::Connection;
use hamlet_client::connection::Contact;
use hamlet_client::messages::Response;
use hamlet_client::outbox::Outbox;
use hamlet_client::realm::{HamletId, LivesIn, Works};
use hamlet_protocol::client::{
    ClientMessage, ReplyKind, RequestId, ServerMessage, WebIde,
};
use hamlet_ui::styles::{Fills, ICON, link_row, note};
use hamlet_ui::theme::HamletTheme;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::{AnyView, View, with_cx};

use super::panel::{self, PanelId};
use super::{Addresses, FILES, page};
use crate::convo::Convo;
use crate::dock::{PanelAppExt, flush_panel, on_a_desk};
use crate::roll::hamlets_moved;

pub(super) fn plugin(app: &mut App) {
    // Closed until asked for from a tab bar's plus.
    app.add_panel(FILES, window())
        .init_resource::<Browsing>()
        .init_resource::<Asked>()
        .add_observer(answered)
        .add_systems(
            Update,
            (follow_chat, forget_on_reconnect, ask)
                .chain()
                .before(page::Opening),
        );
}

fn window() -> DockWindowKind<HamletTheme> {
    DockWindowKind::new("FILES", || flush_panel(contents()))
        .icon(Glyph::Folder.image(ICON))
}

/// The hamlet chosen above the panel, until the chat moves.
#[derive(Resource, Default, Clone, Copy, PartialEq)]
struct Browsing(Option<Entity>);

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
    column((bar(), panel::body(&id(), blank)))
        .fills_width()
        .fills_height()
        .gap(0.0)
}

fn id() -> PanelId {
    PanelId(FILES.to_owned())
}

/// The choice of hamlet, and the press that opens its web IDE again.
fn bar() -> impl View {
    with_cx(|cx| {
        let gap = cx.theme().space.gap;
        row((picker(), reload()))
            .gap(gap / 2.0)
            .padding(UiRect::all(px(gap / 2.0)))
            .align(AlignItems::Center)
            .width(percent(100.0))
            .shrink(0.0)
    })
}

fn picker() -> impl View {
    let list =
        derived(|world: &World| hamlets(world)).when(hamlets_moved());
    keyed(list, |list| {
        if list.is_empty() {
            return column(()).boxed();
        }
        let names = list.iter().map(|(_, name)| name.clone());
        let entities = list
            .iter()
            .map(|(hamlet, _)| *hamlet)
            .collect::<Vec<_>>();
        let order = entities.clone();
        let at = derived(move |world: &World| {
            shown(world)
                .and_then(|hamlet| {
                    order.iter().position(|one| *one == hamlet)
                })
                .unwrap_or_default()
        })
        .when(every_frame());
        dropdown(
            names,
            at,
            Glyph::Chevron.image(ICON),
            move |world, at| {
                world.resource_mut::<Browsing>().0 =
                    entities.get(at).copied();
            },
        )
        .grow(1.0)
        .min_width(px(0.0))
        .boxed()
    })
    .within(row(()).grow(1.0).min_width(px(0.0)))
}

fn reload() -> AnyView {
    button(glyph(Glyph::Restore, ICON).tone(Tone::Dim))
        .padding(UiRect::all(px(4.0)))
        .rules(link_row)
        .on_activate_with(|world, _| {
            *world.resource_mut::<Asked>() = Asked::default();
        })
        .tooltip(|| note("Open again"))
        .boxed()
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
    let chats = world
        .get_resource::<Convo>()
        .and_then(|convo| world.get::<LivesIn>(convo.agent))
        .map(|lives| lives.0);
    world
        .resource::<Browsing>()
        .0
        .filter(standing)
        .or(chats.filter(standing))
        .or_else(|| hamlets(world).first().map(|(hamlet, _)| *hamlet))
}

/// Forgets the hamlet chosen once the chat moves to another agent.
fn follow_chat(
    convo: Option<Res<Convo>>,
    mut browsing: ResMut<Browsing>,
    mut was: Local<Option<Entity>>,
) {
    let Some(convo) = convo else {
        return;
    };
    if *was != Some(convo.agent) {
        if was.is_some() && browsing.0.is_some() {
            browsing.0 = None;
        }
        *was = Some(convo.agent);
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
/// and again when the hamlet or its folder changes.
fn ask(world: &mut World) {
    let open = world
        .resource::<DockTree>()
        .find_leaf_with_window(FILES)
        .is_some()
        || on_a_desk(world, FILES);
    if !open {
        if world.resource::<Asked>().for_.is_some() {
            *world.resource_mut::<Asked>() = Asked::default();
        }
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
        .and_then(|works| works.folder.clone());
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
