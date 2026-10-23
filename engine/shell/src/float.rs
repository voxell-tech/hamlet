//! Cards and menus floating over the window, as plugins open them: a
//! card beside the pointer, where the pointer was, or hung over a
//! point of a view, and a menu at the pointer, the menu a right-click
//! opens.

use bevy::camera::NormalizedRenderTarget;
use bevy::picking::backend::HitData;
use bevy::picking::events::{Pointer, Press};
use bevy::picking::pointer::{Location, PointerButton, PointerId};
use bevy::prelude::*;
use bevy::ui::{OverrideClip, UiSystems};
use bevy::ui_widgets::MenuPopup;
use bevy::window::{PrimaryWindow, WindowRef};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, ContextMenuExt, FrameProps, MENU_Z, column, label,
    menu_item, overlay, popup, row, submenu,
};
use bevy_fynix::{ViewExt, each, resource};

use crate::overlay::Shown as OverlayOpen;
use crate::{AnyView, Cx, EngineTheme};

/// The layer of a card beside the pointer: over the panels, under the
/// menus.
const POINTER_Z: i32 = MENU_Z - 50;

/// The layer of a card hung over a view: over the panels.
const HUNG_Z: i32 = 1;

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Floating>()
        .add_systems(Update, (fade, gather).chain())
        .add_systems(
            PostUpdate,
            (
                (place_beside, ink_beside).chain(),
                place_hung,
                open_menus,
                close_menus,
            )
                .before(UiSystems::Layout),
        );
}

/// A card floating over the window, shown at its [`CardPlace`].
#[derive(Component, Clone, Copy, Debug)]
#[require(CardPlace)]
pub struct FloatCard {
    pub look: CardLook,
    /// The widest it is drawn, in the interface's pixels.
    pub widest: f32,
    /// Builds what it holds, given the card's entity.
    pub content: fn(Entity) -> AnyView,
}

/// What a card is drawn on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CardLook {
    Plain,
    /// The surface in the frame of what floats.
    Card,
    /// The surface in the frame of what waits for the person.
    Tag,
    /// A menu's surface.
    Menu,
}

/// Where a [`FloatCard`] is shown.
#[derive(Component, Clone, Copy, PartialEq, Debug, Default)]
pub enum CardPlace {
    #[default]
    Hidden,
    /// Beside the pointer, following it out of its way.
    Pointer,
    /// At this point of the window, over everything, until a press
    /// elsewhere or Escape closes it.
    Here(Vec2),
    Hung(Hung),
}

/// A card hung over a view: the view's rect in the window, which cuts
/// it, and the point it hangs `gap` pixels over, none while the point
/// is not seen.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Hung {
    pub clip: Rect,
    pub at: Option<Vec2>,
    pub gap: f32,
}

/// A card placed [`CardPlace::Here`] was closed by a press elsewhere
/// or Escape.
#[derive(EntityEvent, Clone, Copy, Debug)]
pub struct CardClosed {
    pub entity: Entity,
}

/// A menu at a point of the window, opened as a right-click opens
/// one. The item chosen is told to `choose`, by its place, and the
/// menu goes.
#[derive(Component, Clone, Debug)]
pub struct FloatMenu {
    pub at: Vec2,
    pub items: Vec<MenuRow>,
    /// Given the world, the menu's entity and the item's place.
    pub choose: fn(&mut World, Entity, u32),
}

/// One item of a menu.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MenuRow {
    pub text: String,
    /// The item it is under, which opens those under it beside the
    /// menu, or none if empty.
    pub group: String,
    /// It refuses, stops or throws away.
    pub critical: bool,
}

/// The rows of a menu of `items`: each item, or a row opening those
/// of a group beside it, in the order they first come. `choose` is
/// given the place of the item chosen.
pub fn menu_rows(
    items: &[MenuRow],
    choose: impl Fn(&mut World, u32) + Clone + Send + Sync + 'static,
) -> Vec<AnyView> {
    let row = move |at: usize, item: &MenuRow| {
        let tone = if item.critical {
            Tone::Critical
        } else {
            Tone::Body
        };
        let at = u32::try_from(at).unwrap_or(u32::MAX);
        let choose = choose.clone();
        menu_item(label(item.text.clone()).tone(tone))
            .on_activate(move |world| choose(world, at))
            .boxed()
    };
    let mut rows = Vec::new();
    let mut groups = Vec::<&str>::new();
    for (at, item) in items.iter().enumerate() {
        if item.group.is_empty() {
            rows.push(row.clone()(at, item));
            continue;
        }
        if groups.contains(&item.group.as_str()) {
            continue;
        }
        groups.push(&item.group);
        let under = items
            .iter()
            .enumerate()
            .filter(|(_, one)| one.group == item.group)
            .map(|(at, one)| (at, one.clone()))
            .collect::<Vec<_>>();
        let row = row.clone();
        rows.push(
            submenu(label(item.group.clone()), move || {
                under
                    .iter()
                    .map(|(at, one)| row.clone()(*at, one))
                    .collect::<Vec<_>>()
            })
            .boxed(),
        );
    }
    rows
}

/// A [`FloatMenu`] closed with nothing chosen. Its entity goes after.
#[derive(EntityEvent, Clone, Copy, Debug)]
pub struct MenuClosed {
    pub entity: Entity,
}

/// The cards and menus shown, as the view builds them.
#[derive(Resource, Default, Clone, PartialEq)]
struct Floating {
    cards: Vec<(Entity, Mode)>,
    menus: Vec<Entity>,
}

/// How a shown card is built.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Mode {
    Pointer,
    Here(Vec2),
    Hung,
}

/// How far a card beside the pointer is shown, from 0 to 1. It fades
/// out after it is hidden.
#[derive(Component, Clone, Copy, Default, Debug)]
struct Fade(f32);

/// On the box of a card shown beside the pointer or hung: its card.
#[derive(Component, Clone, Copy, Debug)]
struct CardBox(Entity);

/// On what cuts a hung card to its view: its card.
#[derive(Component, Clone, Copy, Debug)]
struct HungClip(Entity);

/// On the root of a card's popup: its card.
#[derive(Component, Clone, Copy, Debug)]
struct PopupOf(Entity);

/// On the node a menu hangs from: its menu, and whether its popup
/// was seen open.
#[derive(Component, Clone, Copy, Debug)]
struct MenuAnchor {
    menu: Entity,
    opened: bool,
    seen: bool,
}

/// The cards and menus floating over the window.
pub fn view() -> AnyView {
    overlay((
        each(
            resource::<Floating, _>(|floating| {
                floating.cards.clone()
            }),
            |&(card, mode)| (card, mode),
            |&(card, mode)| card_view(card, mode),
        )
        // Over the whole window, so a card has the window's room to
        // be as wide as it may before its words wrap.
        .within(overlay(())),
        each(
            resource::<Floating, _>(|floating| {
                floating.menus.clone()
            }),
            |&menu| menu,
            |&menu| menu_view(menu),
        ),
    ))
    .boxed()
}

fn card_view(card: Entity, mode: Mode) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let Some(&FloatCard {
            look,
            widest,
            content,
        }) = cx.world.get::<FloatCard>(card)
        else {
            return cx.build(column(()));
        };
        let theme = cx.theme().clone();
        let body = column((content(card),)).gap(0.0);
        match mode {
            Mode::Here(at) => {
                let pad = theme.world.card_padding_x;
                let root = cx.build(
                    popup(Rect::from_corners(at, at), (body,))
                        .on_dismiss(dismiss)
                        .padding(UiRect::all(px(pad)))
                        .max_width(px(widest)),
                );
                cx.world.entity_mut(root).insert(PopupOf(card));
                root
            }
            Mode::Pointer => cx.build(
                dressed(body, look, &theme)
                    .max_width(px(widest))
                    .position(PositionType::Absolute)
                    .inset(corner())
                    .display(Display::None)
                    .z(Some(POINTER_Z))
                    .with((OverrideClip, CardBox(card))),
            ),
            Mode::Hung => cx.build(
                column((dressed(body, look, &theme)
                    .max_width(px(widest))
                    .position(PositionType::Absolute)
                    .inset(corner())
                    .with(CardBox(card)),))
                .gap(0.0)
                .position(PositionType::Absolute)
                .inset(corner())
                .overflow(Overflow::clip())
                .display(Display::None)
                .z(Some(HUNG_Z))
                .with((HungClip(card), Pickable::IGNORE)),
            ),
        }
    })
}

/// `body` on what `look` draws it on.
fn dressed(
    body: bevy_fynix::views::Stack<(AnyView,), ()>,
    look: CardLook,
    theme: &EngineTheme,
) -> bevy_fynix::views::Stack<(AnyView,), ()> {
    let (colors, space, world) =
        (&theme.colors, &theme.space, &theme.world);
    let framed = |body: bevy_fynix::views::Stack<(AnyView,), ()>,
                  pad: (f32, f32),
                  fill: Color,
                  edge: Color| {
        body.padding(UiRect::axes(px(pad.0), px(pad.1)))
            .fill(fill)
            .border(space.border)
            .border_color(edge)
            .radius(space.menu_radius)
    };
    match look {
        CardLook::Plain => body,
        CardLook::Card => framed(
            body,
            (world.card_padding_x, world.card_padding_y),
            *colors.surface,
            *colors.border_strong,
        ),
        CardLook::Tag => framed(
            body,
            (world.tag_padding_x, world.tag_padding_y),
            *colors.surface,
            *colors.active,
        ),
        CardLook::Menu => framed(
            body,
            (space.card_padding_x, space.card_padding_y),
            *colors.background,
            *colors.border,
        ),
    }
}

/// The top left corner, for a node placed by hand.
fn corner() -> UiRect {
    UiRect {
        left: px(0.0),
        top: px(0.0),
        right: Val::Auto,
        bottom: Val::Auto,
    }
}

/// Hides the card of a popup dismissed, and says it was closed.
fn dismiss(world: &mut World, root: Entity) {
    let Some(&PopupOf(card)) = world.get::<PopupOf>(root) else {
        return;
    };
    if let Ok(mut card) = world.get_entity_mut(card) {
        card.insert(CardPlace::Hidden);
    }
    world.trigger(CardClosed { entity: card });
}

/// A node at the menu's point that opens its rows as a right-click
/// on it would.
fn menu_view(menu: Entity) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let Some(FloatMenu { at, items, choose }) =
            cx.world.get::<FloatMenu>(menu).cloned()
        else {
            return cx.build(column(()));
        };
        let rows = move || {
            menu_rows(&items, move |world, at| {
                choose(world, menu, at);
                if let Ok(gone) = world.get_entity_mut(menu) {
                    gone.despawn();
                }
            })
        };
        let anchor = cx.build(
            row(())
                .position(PositionType::Absolute)
                .inset(UiRect {
                    left: px(at.x),
                    top: px(at.y),
                    right: Val::Auto,
                    bottom: Val::Auto,
                })
                .with(Pickable::IGNORE)
                .context_menu(rows),
        );
        cx.world.entity_mut(anchor).insert(MenuAnchor {
            menu,
            opened: false,
            seen: false,
        });
        anchor
    })
}

/// Fades each card beside the pointer in while it is shown there and
/// out after.
fn fade(
    mut cards: Query<(Entity, &CardPlace, Option<&mut Fade>)>,
    windows: Query<&Window, With<PrimaryWindow>>,
    theme: Res<EngineTheme>,
    time: Res<Time>,
    mut commands: Commands,
) {
    let pointer = windows
        .iter()
        .next()
        .and_then(Window::cursor_position)
        .is_some();
    let world = &theme.world;
    for (card, place, faded) in &mut cards {
        let wanted = *place == CardPlace::Pointer && pointer;
        match faded {
            Some(mut faded) => {
                let shown = if wanted {
                    faded.0 + time.delta_secs() / world.card_in
                } else {
                    faded.0 - time.delta_secs() / world.card_out
                };
                faded.0 = shown.clamp(0.0, 1.0);
                if faded.0 == 0.0 && !wanted {
                    commands.entity(card).remove::<Fade>();
                }
            }
            None if wanted => {
                commands.entity(card).insert(Fade(0.0));
            }
            None => {}
        }
    }
}

/// Lists the cards and menus to build, as they are shown.
fn gather(
    cards: Query<
        (Entity, &CardPlace, Option<&Fade>),
        With<FloatCard>,
    >,
    menus: Query<Entity, With<FloatMenu>>,
    mut floating: ResMut<Floating>,
) {
    let mut now = Floating {
        cards: cards
            .iter()
            .filter_map(|(card, place, faded)| {
                let mode = match place {
                    CardPlace::Pointer => Mode::Pointer,
                    CardPlace::Here(at) => Mode::Here(*at),
                    CardPlace::Hung(_) => Mode::Hung,
                    // It goes out with what it held.
                    CardPlace::Hidden if faded.is_some() => {
                        Mode::Pointer
                    }
                    CardPlace::Hidden => return None,
                };
                Some((card, mode))
            })
            .collect(),
        menus: menus.iter().collect(),
    };
    now.cards.sort_by_key(|(card, _)| *card);
    // One menu at a time, the last opened.
    now.menus.sort();
    if let Some(&last) = now.menus.last() {
        now.menus = vec![last];
    }
    floating.set_if_neq(now);
}

/// Keeps each card shown beside the pointer by it, out of its way and
/// inside the window, fading and growing the last little way to its
/// size. The pointer passes through it.
#[allow(clippy::type_complexity)]
fn place_beside(
    cards: Query<(&CardPlace, &Fade)>,
    mut boxes: Query<(
        Entity,
        &CardBox,
        &mut Node,
        &ComputedNode,
        Option<&mut UiTransform>,
        Has<Pickable>,
    )>,
    under: Query<&Children>,
    windows: Query<&Window, With<PrimaryWindow>>,
    theme: Res<EngineTheme>,
    mut commands: Commands,
) {
    let Some(window) = windows.iter().next() else {
        return;
    };
    let world = &theme.world;
    for (node_entity, card_box, mut node, computed, grown, passes) in
        &mut boxes
    {
        let Ok((place, faded)) = cards.get(card_box.0) else {
            continue;
        };
        if !passes {
            commands.entity(node_entity).try_insert(Pickable::IGNORE);
            for child in under.iter_descendants(node_entity) {
                commands.entity(child).try_insert(Pickable::IGNORE);
            }
        }
        let shown = faded.0;
        let display = if shown > 0.0 {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
        let eased = shown * shown * (3.0 - 2.0 * shown);
        let pose = UiTransform::from_scale(Vec2::splat(
            world.card_from + (1.0 - world.card_from) * eased,
        ));
        match grown {
            Some(mut grown) => {
                grown.set_if_neq(pose);
            }
            None => {
                commands.entity(node_entity).try_insert(pose);
            }
        }
        // It stays where it was while it goes.
        let Some(at) = window
            .cursor_position()
            .filter(|_| *place == CardPlace::Pointer)
        else {
            continue;
        };
        let size = computed.size() * computed.inverse_scale_factor();
        let room = Vec2::new(window.width(), window.height());
        let left = if at.x + world.card_right + size.x > room.x {
            at.x - world.card_back - size.x
        } else {
            at.x + world.card_right
        };
        let top = if at.y + world.card_below + size.y > room.y {
            at.y - world.card_back - size.y
        } else {
            at.y + world.card_below
        };
        let margin = world.card_margin;
        put(&mut node, Vec2::new(left.max(margin), top.max(margin)));
    }
}

/// Fades what a card beside the pointer is drawn on, and its words,
/// with the card.
fn ink_beside(
    cards: Query<&Fade>,
    mut boxes: Query<
        (Entity, &CardBox, &mut BackgroundColor, &mut BorderColor),
        Without<TextColor>,
    >,
    under: Query<&Children>,
    mut inks: Query<&mut TextColor>,
) {
    for (node, card_box, mut fill, mut edge) in &mut boxes {
        let Ok(faded) = cards.get(card_box.0) else {
            continue;
        };
        let alpha = faded.0 * faded.0 * (3.0 - 2.0 * faded.0);
        if fill.0.alpha() != alpha {
            fill.0.set_alpha(alpha);
            let mut line = edge.top;
            line.set_alpha(alpha);
            *edge = BorderColor::all(line);
        }
        for child in under.iter_descendants(node) {
            if let Ok(mut ink) = inks.get_mut(child)
                && ink.0.alpha() != alpha
            {
                ink.0.set_alpha(alpha);
            }
        }
    }
}

/// Keeps each hung card over its point, cut where its view ends, and
/// hidden while its point is not seen or an overlay covers the dock.
fn place_hung(
    cards: Query<&CardPlace>,
    mut clips: Query<(&HungClip, &mut Node), Without<CardBox>>,
    mut boxes: Query<
        (&CardBox, &mut Node, &ComputedNode),
        Without<HungClip>,
    >,
    overlay: Res<OverlayOpen>,
) {
    for (clip, mut node) in &mut clips {
        let hung = match cards.get(clip.0) {
            Ok(CardPlace::Hung(hung)) => *hung,
            _ => continue,
        };
        let seen = hung.at.is_some() && overlay.0.is_none();
        let display =
            if seen { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
        put(&mut node, hung.clip.min);
        let (width, height) =
            (px(hung.clip.width()), px(hung.clip.height()));
        if node.width != width || node.height != height {
            node.width = width;
            node.height = height;
        }
    }
    for (card_box, mut node, computed) in &mut boxes {
        let Ok(CardPlace::Hung(hung)) = cards.get(card_box.0) else {
            continue;
        };
        let Some(at) = hung.at else {
            continue;
        };
        let size = computed.size() * computed.inverse_scale_factor();
        let corner = at
            - hung.clip.min
            - Vec2::new(size.x * 0.5, size.y + hung.gap);
        put(&mut node, corner.round());
    }
}

/// Moves a node placed by hand to `corner`.
fn put(node: &mut Node, corner: Vec2) {
    let (left, top) = (px(corner.x), px(corner.y));
    if node.left != left || node.top != top {
        node.left = left;
        node.top = top;
    }
}

/// Opens the menu of each anchor just built, as a right-click there
/// would.
fn open_menus(
    mut anchors: Query<(Entity, &mut MenuAnchor)>,
    menus: Query<&FloatMenu>,
    windows: Query<Entity, With<PrimaryWindow>>,
    mut commands: Commands,
) {
    for (anchor, mut state) in &mut anchors {
        if state.opened {
            continue;
        }
        state.opened = true;
        let Ok(menu) = menus.get(state.menu) else {
            continue;
        };
        let target = match WindowRef::Primary
            .normalize(windows.iter().next())
        {
            Some(window) => NormalizedRenderTarget::Window(window),
            None => NormalizedRenderTarget::None {
                width: 1,
                height: 1,
            },
        };
        let location = Location {
            target,
            position: menu.at,
        };
        commands.trigger(Pointer::new_without_propagate(
            PointerId::Mouse,
            location,
            Press {
                button: PointerButton::Secondary,
                hit: HitData::new(
                    Entity::PLACEHOLDER,
                    0.0,
                    None,
                    None,
                ),
                count: 1,
            },
            anchor,
        ));
    }
}

/// Says each menu whose popup closed with nothing chosen was closed,
/// and takes it away.
fn close_menus(
    mut anchors: Query<(Entity, &mut MenuAnchor)>,
    popups: Query<Entity, With<MenuPopup>>,
    parents: Query<&ChildOf>,
    menus: Query<(), With<FloatMenu>>,
    mut commands: Commands,
) {
    for (anchor, mut state) in &mut anchors {
        if !state.opened || !menus.contains(state.menu) {
            continue;
        }
        let open = popups.iter().any(|popup| {
            parents.iter_ancestors(popup).any(|up| up == anchor)
        });
        if open {
            state.seen = true;
        } else if state.seen {
            commands.trigger(MenuClosed { entity: state.menu });
            commands.entity(state.menu).try_despawn();
        }
    }
}
