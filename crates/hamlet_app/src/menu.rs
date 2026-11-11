//! The menus in the top bar.

use bevy::app::{App, First, Update};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::input::ButtonInput;
use bevy::input::keyboard::KeyCode;
use bevy::input::mouse::MouseButton;
use bevy::math::Rect;
use bevy::ui::{
    AlignItems, ComputedNode, UiGlobalTransform, UiRect, UiScale,
    percent, px,
};
use bevy::window::Window;
use bevy_fynix::shortcut::{
    Chord, CommandId, CommandSpec, GLOBAL, Mods, Scope,
    ShortcutAppExt,
};
use bevy_fynix::tokens::{Motion, Tone};
use bevy_fynix::views::{
    BehaviorExt, FrameProps, POPUP, button, frame, label, menu_item,
    overlay, popup, row,
};
use bevy_fynix::{ScopedExt, ViewExt, keyed, style};
use hamlet_realm::pick::Taken;
use hamlet_ui::styles::{ICON, hairline, transparent};
use hamlet_ui::theme::HamletTheme;
use hamlet_ui::widgets::effects::Underglow;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::widgets::platform::DisplayText;
use hamlet_ui::{AnyView, Cx, View, with_cx};

use crate::dock::show_tab;
use crate::updates::{self, Updates};
use crate::window;

/// The key that brings Settings forward.
const SETTINGS: Chord = Chord {
    key: KeyCode::Comma,
    mods: Mods::PRIMARY,
};

/// The key that closes the window.
const CLOSE: Chord = Chord {
    key: KeyCode::KeyQ,
    mods: Mods::PRIMARY,
};

pub fn plugin(app: &mut App) {
    let command = |id, label, run| CommandSpec {
        id: CommandId(id),
        label,
        scope: GLOBAL,
        run,
        enabled: |_| true,
        repeat: false,
    };
    app.init_resource::<OpenMenu>()
        .init_resource::<Updates>()
        .add_command(
            command("app.settings", "Settings", |world, _| {
                settings(world);
            }),
            &[SETTINGS],
        )
        .add_command(
            command("app.close", "Close Window", |world, _| {
                window::quit(world);
            }),
            &[CLOSE],
        )
        .add_systems(Update, light_titles)
        // Before the toolkit closes a popup on Escape this frame.
        .add_systems(First, hold_escape);
}

/// A menu of the top bar. On a title, the menu it opens.
#[derive(Clone, Copy, PartialEq, Component)]
pub enum Menu {
    App,
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
pub(crate) fn rect_of(world: &World, title: Entity) -> Option<Rect> {
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
    // In the panels' pixels, as the titles' rects are.
    let Some(pointer) = pointer.map(|at| {
        at / world.get_resource::<UiScale>().map_or(1.0, |s| s.0)
    }) else {
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
            // Clear of its hover fill, so that fades in rather than
            // passing through black.
            f.fill(transparent(t.palette.button_hover))
                .height(percent(100.0))
                .radius(t.space.round)
                .padding(UiRect::axes(
                    px(t.space.title_padding),
                    px(0.0),
                ))
        })
        .label(|l, t| {
            l.tone(Tone::Faint).size(t.text.heading).bold(true)
        })
        .hovered(|s| {
            s.fill(|t: &HamletTheme| t.palette.button_hover)
                .tone(Tone::Body)
        })
        .pressed(|s| {
            s.fill(|t: &HamletTheme| t.palette.selected)
                .tone(Tone::Dim)
        })
        .transition(Motion::Interact)
        .apply(cx);
}

/// Holds a title lit, washed with light, while its menu is open.
pub(crate) fn light_titles(
    open: Res<OpenMenu>,
    mut titles: Query<(&Menu, &mut Underglow)>,
) {
    for (menu, mut light) in &mut titles {
        let held = open.0.is_some_and(|(shown, _)| shown == *menu);
        if light.held != held {
            light.held = held;
        }
    }
}

/// A menu's title, with a chevron after its name that says it opens
/// a menu.
fn title(name: &'static str, menu: Menu) -> impl View {
    button(
        row((
            label(name).wrap(false).tagged(DisplayText),
            glyph(Glyph::Chevron, 10.0).rotation(180.0),
        ))
        .gap(6.0)
        .align(AlignItems::Center),
    )
    .rules(nav)
    .tagged(menu)
    .tagged(Underglow::new(false))
    .on_activate_with(move |world, node| open(world, menu, node))
}

/// The menu titles, for the top bar.
pub fn titles() -> impl View {
    with_cx(|cx| {
        row((title("Hamlet", Menu::App),))
            .gap(0.0)
            .height(px(cx.theme().space.bar))
            .align(AlignItems::Center)
    })
}

/// An item of a menu: its icon, its name and the keys that do the
/// same, if any.
fn item(
    which: Glyph,
    name: impl Into<String>,
    keys: Option<Chord>,
    act: fn(&mut World),
) -> AnyView {
    let name = name.into();
    with_cx(move |cx| {
        // A comma reads as its own mark, not its key's name.
        let keys = keys.map_or_else(String::new, |keys| {
            keys.to_string().replace("Comma", ",")
        });
        menu_item(
            row((
                glyph(which, ICON).tone(Tone::Faint),
                label(name.clone()).wrap(false),
                frame().grow(1.0).min_width(px(16.0)),
                label(keys).tone(Tone::Faint).wrap(false),
            ))
            .gap(cx.theme().space.menu_gap)
            .align(AlignItems::Center)
            .width(percent(100.0)),
        )
        .on_activate(move |world| {
            world.resource_mut::<OpenMenu>().0 = None;
            act(world);
        })
    })
    .boxed()
}

/// Brings the Settings panel forward.
fn settings(world: &mut World) {
    show_tab(world, "settings");
}

/// What the Hamlet menu offers for updates.
#[derive(Clone, PartialEq, Debug)]
enum Updating {
    /// Nothing: this copy does not look for updates.
    Off,
    /// A look for one.
    Check,
    /// A look the chief asked for is on its way.
    Checking,
    /// The release found, to take.
    Ready(hamlet_protocol::Version),
}

impl Updating {
    fn of(world: &World) -> Self {
        let updates = world.resource::<Updates>();
        match (&updates.offer, updates.checking) {
            _ if !updates.looking => Self::Off,
            (Some(offer), _) => Self::Ready(offer.version),
            (None, true) => Self::Checking,
            (None, false) => Self::Check,
        }
    }

    fn item(&self) -> Option<AnyView> {
        Some(match self {
            Self::Off => return None,
            Self::Check => item(
                Glyph::Restore,
                "Check for updates",
                None,
                updates::check,
            ),
            Self::Checking => item(
                Glyph::Restore,
                "Checking for updates...",
                None,
                |_| {},
            ),
            Self::Ready(version) => item(
                Glyph::Rise,
                format!("Update to {version}"),
                None,
                updates::update,
            ),
        })
    }
}

fn items(
    menu: Menu,
    found: bool,
    updating: &Updating,
) -> Vec<AnyView> {
    match menu {
        Menu::App => {
            let mut items = vec![item(
                Glyph::Sliders,
                "Settings",
                Some(SETTINGS),
                settings,
            )];
            items.extend(updating.item());
            if found {
                items.push(item(
                    Glyph::Hamlet,
                    "Found a Hamlet...",
                    None,
                    crate::world_menu::ask_found,
                ));
            }
            items.push(
                with_cx(|cx| {
                    hairline(cx.theme().palette.gold_6)
                        .margin(UiRect::vertical(px(4.0)))
                })
                .boxed(),
            );
            // The server and its agents carry on without the window.
            items.push(item(
                Glyph::Leave,
                "Close window",
                Some(CLOSE),
                window::quit,
            ));
            items
        }
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
                Updating::of(world),
            )
        })
        .when(crate::roll::either(
            crate::roll::either(
                crate::roll::moved::<OpenMenu>(),
                crate::roll::moved::<hamlet_client::realm::Deeds>(),
            ),
            crate::roll::moved::<Updates>(),
        )),
        |(open, found, updating)| match open {
            Some((menu, anchor)) => {
                let (anchor, items) =
                    (*anchor, items(*menu, *found, updating));
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
        assert!(shown.contains("Close window"), "{shown}");
        assert!(!shown.contains("New desk"));
        assert!(!shown.contains("Your hearth"));
        // A copy that does not look for updates offers no way to.
        assert!(!shown.contains("updates"));
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
        crate::testing::press(&mut app, "Close window");
        assert!(app.world_mut().resource_mut::<bevy::ecs::message::Messages<bevy::app::AppExit>>().drain().any(|exit| exit == bevy::app::AppExit::Success));
    }

    #[test]
    fn the_app_menu_checks_for_an_update_until_one_is_found() {
        let mut app = crate::testing::panel_app();
        app.add_plugins(plugin);
        app.world_mut().resource_mut::<Updates>().looking = true;
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
        assert!(shown.contains("Check for updates"), "{shown}");
        app.world_mut().resource_mut::<Updates>().checking = true;
        app.update();
        let shown = crate::testing::all_words(&mut app);
        assert!(shown.contains("Checking for updates..."), "{shown}");
        app.world_mut().resource_mut::<Updates>().offer =
            Some(updates::Offer {
                version: hamlet_protocol::Version::new(0, 2, 0),
                ready: updates::Ready::Page(
                    "https://example.com".into(),
                ),
            });
        app.update();
        let shown = crate::testing::all_words(&mut app);
        assert!(shown.contains("Update to 0.2.0"), "{shown}");
        assert!(!shown.contains("Check for updates"));
    }
}
