//! The menus in the top bar.

use bevy::app::{App, First, Update};
use bevy::color::Color;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::input::ButtonInput;
use bevy::input::mouse::MouseButton;
use bevy::math::Rect;
use bevy::ui::{
    AlignItems, ComputedNode, UiGlobalTransform, UiRect, percent, px,
};
use bevy::window::Window;
use bevy_fynix::shortcut::Scope;
use bevy_fynix::tokens::{Motion, Tone};
use bevy_fynix::views::{
    BehaviorExt, FrameProps, POPUP, button, frame, label, menu_item,
    overlay, popup, row,
};
use bevy_fynix::{ScopedExt, ViewExt, keyed, style};
use hamlet_realm::pick::{Orbit, Taken};

use crate::feel::FootLight;
use crate::icons::{Glyph, glyph};
use crate::looks::ICON;
use crate::platform::DisplayText;
use crate::theme::HamletTheme;
use crate::world_view::TimeOfDay;
use crate::{AnyView, Cx, View, window, with_cx};

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<OpenMenu>()
        .add_systems(Update, light_titles)
        // Before the toolkit closes a popup on Escape this frame.
        .add_systems(First, hold_escape);
}

/// A menu of the top bar. On a title, the menu it opens.
#[derive(Clone, Copy, PartialEq, Component)]
pub enum Menu {
    App,
    View,
}

/// The open menu and the rect of its title, in the window's logical
/// pixels.
#[derive(Resource, Default)]
pub struct OpenMenu(Option<(Menu, Rect)>);

impl OpenMenu {
    pub(crate) fn is_open(&self) -> bool {
        self.0.is_some()
    }

    /// `menu` open under a title at `title`.
    pub fn under(menu: Menu, title: Rect) -> Self {
        Self(Some((menu, title)))
    }
}

/// Tells the world whether a popup is open as the frame begins,
/// so that the Escape that closes it does not also let go of what
/// is selected. The toolkit closes the popup itself.
pub(crate) fn hold_escape(
    open: Query<&Scope>,
    taken: Option<ResMut<Taken>>,
) {
    let Some(mut taken) = taken else {
        return;
    };
    let over = open.iter().any(|scope| scope.0 == POPUP);
    if taken.over != over {
        taken.over = over;
    }
}

/// The rect of `title`, in the window's logical pixels.
fn rect_of(world: &World, title: Entity) -> Option<Rect> {
    let (node, at) = world
        .get::<ComputedNode>(title)
        .zip(world.get::<UiGlobalTransform>(title))?;
    let scale = node.inverse_scale_factor();
    let (_, _, centre) = at.to_scale_angle_translation();
    Some(Rect::from_center_size(centre * scale, node.size() * scale))
}

/// Closes the open menu. A press on another title opens that menu in
/// its place: the open menu covers the titles, so they never hear the
/// press. Escape only closes.
fn dismiss(world: &mut World) {
    let was = world.resource_mut::<OpenMenu>().0.take();
    let by_key = world
        .get_resource::<ButtonInput<MouseButton>>()
        .is_some_and(|buttons| !buttons.pressed(MouseButton::Left));
    if by_key {
        return;
    }
    let pointer =
        world.try_query::<&Window>().and_then(|mut windows| {
            windows.iter(world).find_map(Window::cursor_position)
        });
    let Some(pointer) = pointer else {
        return;
    };
    let pressed = world
        .try_query::<(Entity, &Menu)>()
        .map(|mut titles| {
            titles
                .iter(world)
                .map(|(title, menu)| (title, *menu))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
        .into_iter()
        .find(|(title, _)| {
            rect_of(world, *title)
                .is_some_and(|rect| rect.contains(pointer))
        });
    if let Some((title, menu)) = pressed
        && was.is_none_or(|(open, _)| open != menu)
    {
        open(world, menu, title);
    }
}

fn open(world: &mut World, menu: Menu, title: Entity) {
    let Some(rect) = rect_of(world, title) else {
        return;
    };
    world.resource_mut::<OpenMenu>().0 = Some((menu, rect));
}

/// A menu's title: lettering alone, that brightens under the pointer.
pub fn nav(cx: &mut Cx) {
    style::<HamletTheme>()
        .frame(|f, t| {
            f.fill(Color::NONE)
                .height(percent(100.0))
                .radius(0.0)
                .padding(UiRect::axes(
                    px(t.space.title_padding),
                    px(0.0),
                ))
        })
        .label(|l, t| {
            l.tone(Tone::Faint).size(t.text.heading).bold(true)
        })
        .hovered(|s| s.fill(|_| Color::NONE).tone(Tone::Body))
        .pressed(|s| s.fill(|_| Color::NONE).tone(Tone::Dim))
        .transition(Motion::Interact)
        .apply(cx);
}

/// Holds a title lit while its menu is open.
pub(crate) fn light_titles(
    open: Res<OpenMenu>,
    mut titles: Query<(&Menu, &mut FootLight)>,
) {
    for (menu, mut light) in &mut titles {
        let held = open.0.is_some_and(|(shown, _)| shown == *menu);
        if light.held != held {
            light.held = held;
        }
    }
}

fn title(name: &'static str, menu: Menu) -> impl View {
    button(label(name).wrap(false).tagged(DisplayText))
        .rules(nav)
        .tagged(menu)
        .tagged(FootLight::new(false))
        .on_activate_with(move |world, node| open(world, menu, node))
}

/// The menu titles, for the top bar.
pub fn titles() -> impl View {
    with_cx(|cx| {
        row((title("HAMLET", Menu::App), title("VIEW", Menu::View)))
            .gap(0.0)
            .height(px(cx.theme().space.bar))
            .align(AlignItems::Center)
    })
}

fn item(
    which: Glyph,
    name: &'static str,
    act: fn(&mut World),
) -> AnyView {
    with_cx(move |cx| {
        menu_item(
            row((
                glyph(which, ICON).tone(Tone::Faint),
                label(name).wrap(false),
            ))
            .gap(cx.theme().space.menu_gap)
            .align(AlignItems::Center),
        )
        .on_activate(move |world| {
            world.resource_mut::<OpenMenu>().0 = None;
            act(world);
        })
    })
    .boxed()
}

fn items(menu: Menu, found: bool) -> Vec<AnyView> {
    match menu {
        Menu::App => {
            let mut items = Vec::new();
            if found {
                items.push(item(
                    Glyph::Plus,
                    "Found a Hamlet...",
                    crate::world_menu::ask_found,
                ));
            }
            items.push(item(Glyph::Power, "Close", window::quit));
            items
        }
        Menu::View => vec![
            item(Glyph::Aim, "Reset Camera", |w| {
                *w.resource_mut::<Orbit>() = Orbit::default();
            }),
            item(Glyph::Clock, "Sky Follows the Clock", |w| {
                *w.resource_mut::<TimeOfDay>() = TimeOfDay::Clock;
            }),
            item(Glyph::Sun, "Sky at Day", |w| {
                *w.resource_mut::<TimeOfDay>() = TimeOfDay::Day;
            }),
            item(Glyph::Moon, "Sky at Night", |w| {
                *w.resource_mut::<TimeOfDay>() = TimeOfDay::Night;
            }),
        ],
    }
}

/// The open menu's list, or nothing while none is open.
pub fn open_menu() -> impl View {
    keyed(
        bevy_fynix::derived(|world: &World| {
            (
                world.resource::<OpenMenu>().0,
                hamlet_client::realm::Deeds::is_allowed(
                    world, "found",
                ),
            )
        })
        .when(crate::roll::either(
            crate::roll::moved::<OpenMenu>(),
            crate::roll::moved::<hamlet_client::realm::Deeds>(),
        )),
        |(open, found)| match open {
            Some((menu, anchor)) => {
                let (anchor, items) = (*anchor, items(*menu, *found));
                with_cx(move |cx| {
                    popup(anchor, items)
                        .on_dismiss(|world, _| dismiss(world))
                        .min_width(px(cx.theme().space.menu_width))
                })
                .boxed()
            }
            None => frame().boxed(),
        },
    )
    .within(overlay(()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_app_menu_closes_only_the_window_and_offers_founding_only_when_supported()
     {
        let mut app = crate::testing::panel_app();
        app.add_plugins(plugin).add_message::<bevy::app::AppExit>();
        app.insert_resource(OpenMenu::under(
            Menu::App,
            Rect::default(),
        ));
        bevy_fynix::mount::<HamletTheme>(
            app.world_mut(),
            open_menu(),
        );
        app.update();
        let shown = crate::testing::all_words(&mut app);
        assert!(shown.contains("Close"), "{shown}");
        assert!(!shown.contains("Run in Background"));
        assert!(!shown.contains("Quit Hamlet"));
        assert!(!shown.contains("Found a Hamlet..."));
        app.insert_resource(hamlet_client::realm::Deeds(vec![
            "found".into(),
        ]));
        app.update();
        assert!(
            crate::testing::words(&mut app)
                .iter()
                .any(|text| text == "Found a Hamlet...")
        );
        crate::testing::press(&mut app, "Close");
        assert!(app.world_mut().resource_mut::<bevy::ecs::message::Messages<bevy::app::AppExit>>().drain().any(|exit| exit == bevy::app::AppExit::Success));
    }
}
