//! Add to chat from what a page shows: a picture of the window, cut
//! where a box was drawn, put in the chat's field with where it is
//! from and the chief's note, if any.
//!
//! The Files panel's page draws its own box (`files::context`). A Web
//! panel's is drawn here, on a sheet laid over its page by the press
//! beside its address, as any site can load in it: drag a box, write
//! a note in the field under it, and Enter adds it; Escape stops.

use std::io::Cursor;

use bevy::app::{App, Update};
use bevy::color::Color;
use bevy::ecs::component::Component;
use bevy::ecs::hierarchy::Children;
use bevy::ecs::observer::On;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::image::Image;
use bevy::input::ButtonInput;
use bevy::input::keyboard::KeyCode;
use bevy::input::mouse::MouseButton;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::math::{Rect, UVec2, Vec2};
use bevy::picking::Pickable;
use bevy::picking::events::PointerPress;
use bevy::picking::pointer::PointerButton;
use bevy::render::render_resource::TextureFormat;
use bevy::render::view::screenshot::{
    Screenshot, ScreenshotCaptured,
};
use bevy::text::EditableText;
use bevy::ui::{
    AlignItems, ComputedNode, JustifyContent, PositionType,
    UiGlobalTransform, UiRect, Val, ZIndex, percent, px,
};
use bevy::window::{PrimaryWindow, SystemCursorIcon, Window};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, button, column, frame, row,
    text_field,
};
use bevy_fynix::{
    EntityCursor, ScopedExt, ViewExt, derived, every_frame, keyed,
    resource, style,
};
use hamlet_ui::styles::{ICON, link_row, note};
use hamlet_ui::theme::HamletTheme;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::{AnyView, Cx, View, with_cx};
use image::{ImageFormat, RgbaImage};

use super::Addresses;
use super::panel::PanelId;
use crate::convo::Convo;
use crate::dock::{HUB, show_tab};
use crate::screens::chat;

/// The least side of a box, in the interface's pixels: a shorter
/// drag is a slip, and the sheet waits for another.
const SMALLEST: f32 = 6.0;

/// The note's field, in the interface's pixels.
const NOTE: Vec2 = Vec2::new(320.0, 34.0);

/// The space between the box, the sheet's edges and the note.
const SPACE: f32 = 8.0;

/// The frames the window draws without the sheet before it is
/// captured, so that the picture is of the page alone.
const SETTLE: u8 = 3;

/// The dark laid over the page outside the box, and over all of it
/// before there is one.
const SHADE: Color = Color::srgba(0.04, 0.024, 0.012, 0.35);
const SHEET: Color = Color::srgba(0.04, 0.024, 0.012, 0.28);

/// How far the shade reaches past the box: past any panel's edge, so
/// the sheet's clip cuts it there.
const REACH: f32 = 8000.0;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Cropping>()
        .init_resource::<Taking>()
        .add_observer(press)
        .add_systems(Update, (follow, stop, focus_note, take));
}

/// The Web panel a box is being drawn over, while one is.
#[derive(Resource, Default)]
struct Cropping(Option<Crop>);

#[derive(Clone, PartialEq, Debug)]
struct Crop {
    panel: PanelId,
    /// The box, once a press on the sheet starts one.
    drawn: Option<Drawn>,
    /// Whether the note's field waits for the focus.
    focus: bool,
}

/// A box drawn on the sheet.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Drawn {
    /// The sheet, in the window's pixels.
    area: Rect,
    /// The interface's pixels to one of the window's.
    ui: f32,
    /// Where the press was and where the pointer is, in the window's
    /// pixels, inside `area`.
    from: Vec2,
    to: Vec2,
    /// Whether the press still holds.
    held: bool,
}

impl Drawn {
    /// The box, in the window's pixels.
    fn rect(&self) -> Rect {
        Rect::from_corners(self.from, self.to)
    }

    /// The box, in the interface's pixels from the sheet's corner.
    fn on_sheet(&self) -> Rect {
        let rect = self.rect();
        Rect {
            min: (rect.min - self.area.min) * self.ui,
            max: (rect.max - self.area.min) * self.ui,
        }
    }

    /// Where the note's field goes: under the box, or over it where
    /// there is no room under, inside the sheet.
    fn note_at(&self) -> Vec2 {
        let sheet = self.area.size() * self.ui;
        let rect = self.on_sheet();
        let x = rect.min.x.min(sheet.x - NOTE.x - SPACE).max(SPACE);
        let under = rect.max.y + SPACE;
        let y = match under + NOTE.y <= sheet.y - SPACE {
            true => under,
            false => (rect.min.y - SPACE - NOTE.y).max(SPACE),
        };
        Vec2::new(x, y)
    }
}

/// Pictures waiting for the window to draw without the sheet: the
/// part of the window, the words to add with it, and the frames left.
#[derive(Resource, Default)]
struct Taking(Vec<(Rect, String, u8)>);

/// The sheet laid over a Web panel's page.
#[derive(Component)]
struct Sheet;

/// The field of the note on a box.
#[derive(Component)]
struct NoteField;

/// What a panel's sheet shows.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Shown {
    Off,
    /// No box yet: the page darkened, and what to do.
    Bare,
    /// A box held or drawn, and the note under it once drawn.
    Boxed {
        noting: bool,
    },
}

fn shown(cropping: &Cropping, panel: &PanelId) -> Shown {
    match &cropping.0 {
        Some(crop) if crop.panel == *panel => match crop.drawn {
            None => Shown::Bare,
            Some(drawn) => Shown::Boxed {
                noting: !drawn.held,
            },
        },
        _ => Shown::Off,
    }
}

/// The press beside a Web panel's address that lays the sheet over
/// its page, or takes it away.
pub(super) fn crop_button(panel: &PanelId) -> AnyView {
    let lit = panel.clone();
    let panel = panel.clone();
    keyed(
        resource::<Cropping, _>(move |cropping| {
            shown(cropping, &lit) != Shown::Off
        }),
        move |on| {
            let panel = panel.clone();
            let tone = match on {
                true => Tone::Accent,
                false => Tone::Dim,
            };
            button(glyph(Glyph::Crop, ICON).tone(tone))
                .padding(UiRect::all(px(4.0)))
                .rules(link_row)
                .on_activate_with(move |world, _| {
                    toggle(world, &panel);
                })
                .tooltip(|| note("Add part of the page to the chat"))
                .boxed()
        },
    )
    .boxed()
}

fn toggle(world: &mut World, panel: &PanelId) {
    let mut cropping = world.resource_mut::<Cropping>();
    cropping.0 = match &cropping.0 {
        Some(crop) if crop.panel == *panel => None,
        _ => Some(Crop {
            panel: panel.clone(),
            drawn: None,
            focus: false,
        }),
    };
}

/// The sheet over `panel`'s page while a box is drawn on it, and the
/// note's field. Above the page, and in no one's way when off.
pub(super) fn sheet(panel: &PanelId) -> impl View + use<> {
    let panel = panel.clone();
    keyed(
        resource::<Cropping, _>(move |cropping| {
            shown(cropping, &panel)
        }),
        |shown| match *shown {
            Shown::Off => frame().boxed(),
            Shown::Bare => cover(SHEET, hint().boxed()).boxed(),
            Shown::Boxed { noting } => {
                let sheet = cover(Color::NONE, shade().boxed());
                match noting {
                    true => column((sheet, note_field()))
                        .position(PositionType::Absolute)
                        .inset(UiRect::all(px(0.0)))
                        .tagged(Pickable::IGNORE)
                        .boxed(),
                    false => sheet.boxed(),
                }
            }
        },
    )
    .within(
        column(())
            .position(PositionType::Absolute)
            .inset(UiRect::all(px(0.0)))
            .tagged(Pickable::IGNORE)
            .tagged(ZIndex(1)),
    )
}

/// The sheet itself, filled with `fill`, holding `inside`.
fn cover(fill: Color, inside: AnyView) -> impl View {
    column((inside,))
        .position(PositionType::Absolute)
        .inset(UiRect::all(px(0.0)))
        .fill(fill)
        .overflow(bevy::ui::Overflow::clip())
        .tagged(Sheet)
        .tagged(EntityCursor(SystemCursorIcon::Crosshair))
}

/// What to do, at the top of the bare sheet.
fn hint() -> impl View {
    row((row((note(
        "Drag to draw a box around what to add. Esc to stop.",
    ),))
    .padding(UiRect::axes(px(10.0), px(4.0)))
    .fill(Color::srgba(0.08, 0.055, 0.035, 0.92))
    .radius(4.0)
    .tagged(Pickable::IGNORE),))
    .position(PositionType::Absolute)
    .inset(UiRect::new(px(0.0), px(0.0), px(12.0), Val::Auto))
    .justify(JustifyContent::Center)
    .tagged(Pickable::IGNORE)
}

/// The box, lined in the lamp's gold, with the page dark around it.
fn shade() -> impl View {
    let on_sheet = |cropping: &Cropping| {
        cropping
            .0
            .as_ref()
            .and_then(|crop| crop.drawn)
            .map_or(Rect::default(), |drawn| drawn.on_sheet())
    };
    with_cx(move |cx| {
        let lamp = cx.theme().palette.gold_4;
        column((frame()
            .width(percent(100.0))
            .height(percent(100.0))
            .border(2.0)
            .border_color(lamp)
            .radius(2.0)
            .tagged(Pickable::IGNORE),))
        .position(PositionType::Absolute)
        .inset(resource::<Cropping, _>(move |cropping| {
            let rect = on_sheet(cropping);
            UiRect::new(
                px(rect.min.x - REACH),
                Val::Auto,
                px(rect.min.y - REACH),
                Val::Auto,
            )
        }))
        .width(resource::<Cropping, _>(move |cropping| {
            px(on_sheet(cropping).width() + 2.0 * REACH)
        }))
        .height(resource::<Cropping, _>(move |cropping| {
            px(on_sheet(cropping).height() + 2.0 * REACH)
        }))
        .border(REACH)
        .border_color(SHADE)
        .tagged(Pickable::IGNORE)
    })
}

/// The note's field under the box, with the press that adds the box.
fn note_field() -> impl View {
    with_cx(|cx| {
        let theme = cx.theme();
        let lamp = theme.palette.gold_4;
        let fill = theme.palette.input;
        row((
            column((
                text_field("", |world, _| {
                    let entered = world
                        .resource::<ButtonInput<KeyCode>>()
                        .any_just_pressed([
                            KeyCode::Enter,
                            KeyCode::NumpadEnter,
                        ]);
                    // Focus leaving commits too, which adds nothing.
                    if entered {
                        add_drawn(world);
                    }
                })
                .grow(1.0)
                .min_width(px(0.0))
                .rules(bare_field)
                .tagged(NoteField),
                placeholder(),
            ))
            .grow(1.0)
            .min_width(px(0.0))
            .justify(JustifyContent::Center),
            button(glyph(Glyph::Send, ICON).tone(Tone::Body))
                .padding(UiRect::all(px(4.0)))
                .rules(link_row)
                .on_activate(add_drawn)
                .tooltip(|| note("Add to the chat")),
        ))
        .position(PositionType::Absolute)
        .inset(resource::<Cropping, _>(|cropping| {
            let at = cropping
                .0
                .as_ref()
                .and_then(|crop| crop.drawn)
                .map_or(Vec2::ZERO, |drawn| drawn.note_at());
            UiRect::new(px(at.x), Val::Auto, px(at.y), Val::Auto)
        }))
        .width(px(NOTE.x))
        .height(px(NOTE.y))
        .padding(UiRect::new(px(10.0), px(4.0), px(0.0), px(0.0)))
        .gap(4.0)
        .align(AlignItems::Center)
        .fill(fill)
        .border(1.0)
        .border_color(lamp)
        .radius(6.0)
    })
}

/// "Add a note", over the field while it is empty.
fn placeholder() -> impl View {
    keyed(
        derived(|world: &World| note_text(world).is_empty())
            .when(every_frame()),
        |empty| match empty {
            true => row((note("Add a note (optional)"),))
                .position(PositionType::Absolute)
                .inset(UiRect::all(px(0.0)))
                .align(AlignItems::Center)
                .tagged(Pickable::IGNORE)
                .boxed(),
            false => frame().boxed(),
        },
    )
}

/// The note's field without a frame of its own: the note's row is
/// its frame.
fn bare_field(cx: &mut Cx) {
    style::<HamletTheme>()
        .frame(|f, _| {
            f.fill(Color::NONE)
                .border(0.0)
                .padding(UiRect::all(px(0.0)))
        })
        .hovered(|s| s.frame(|f, _| f.fill(Color::NONE)))
        .apply(cx);
}

/// What is typed in the note's field.
fn note_text(world: &World) -> String {
    let Some(mut fields) =
        world.try_query_filtered::<&Children, With<NoteField>>()
    else {
        return String::new();
    };
    fields
        .iter(world)
        .flat_map(|children| children.iter().copied())
        .find_map(|kid| world.get::<EditableText>(kid))
        .map(|text| text.value().to_string())
        .unwrap_or_default()
}

/// Starts a box where the sheet is pressed.
fn press(
    press: On<PointerPress>,
    sheets: Query<(&ComputedNode, &UiGlobalTransform), With<Sheet>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut cropping: ResMut<Cropping>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let Ok((node, at)) = sheets.get(press.entity) else {
        return;
    };
    let Some(crop) = cropping.0.as_mut() else {
        return;
    };
    let scale = windows.single().map_or(1.0, Window::scale_factor);
    let area = Rect::from_center_size(at.translation, node.size());
    let from =
        (press.pointer.position * scale).clamp(area.min, area.max);
    crop.drawn = Some(Drawn {
        area,
        ui: node.inverse_scale_factor(),
        from,
        to: from,
        held: true,
    });
}

/// Keeps the box's corner on the pointer while the press holds,
/// however far it goes, and lets the box go at the release: to the
/// note, or, too small, to another press.
fn follow(
    mut cropping: ResMut<Cropping>,
    windows: Query<&Window, With<PrimaryWindow>>,
    buttons: Res<ButtonInput<MouseButton>>,
) {
    let Some(drawn) = cropping
        .0
        .as_ref()
        .and_then(|crop| crop.drawn)
        .filter(|drawn| drawn.held)
    else {
        return;
    };
    let to = windows
        .single()
        .ok()
        .and_then(|window| {
            Some(window.cursor_position()? * window.scale_factor())
        })
        .map_or(drawn.to, |at| {
            at.clamp(drawn.area.min, drawn.area.max)
        });
    let released = !buttons.pressed(MouseButton::Left);
    if to == drawn.to && !released {
        return;
    }
    let Some(crop) = cropping.0.as_mut() else {
        return;
    };
    let moved = Drawn {
        to,
        held: !released,
        ..drawn
    };
    let size = moved.on_sheet().size();
    crop.drawn = match released && size.min_element() < SMALLEST {
        true => None,
        false => Some(moved),
    };
    crop.focus = released && crop.drawn.is_some();
}

/// Takes the sheet away on Escape.
fn stop(
    keys: Res<ButtonInput<KeyCode>>,
    mut cropping: ResMut<Cropping>,
) {
    if cropping.0.is_some() && keys.just_pressed(KeyCode::Escape) {
        cropping.0 = None;
    }
}

/// Gives the note's field the focus once it is there.
fn focus_note(
    mut cropping: ResMut<Cropping>,
    fields: Query<&Children, With<NoteField>>,
    texts: Query<(), With<EditableText>>,
    mut focus: ResMut<InputFocus>,
) {
    if !cropping.0.as_ref().is_some_and(|crop| crop.focus) {
        return;
    }
    let Some(text) = fields
        .iter()
        .flat_map(|children| children.iter().copied())
        .find(|kid| texts.contains(*kid))
    else {
        return;
    };
    focus.set(text, FocusCause::Pressed);
    if let Some(crop) = cropping.0.as_mut() {
        crop.focus = false;
    }
}

/// Takes the sheet away, and adds the part of the page in its box,
/// with where it is from and the note, once the window draws the page
/// without it.
fn add_drawn(world: &mut World) {
    let note = note_text(world);
    let Some(crop) = world.resource_mut::<Cropping>().0.take() else {
        return;
    };
    let Some(drawn) = crop.drawn.filter(|drawn| !drawn.held) else {
        return;
    };
    let address = world
        .resource::<Addresses>()
        .0
        .get(&crop.panel.0)
        .cloned()
        .unwrap_or_default();
    let words = format!("From {address}:\n{}", with_note(&note));
    world.resource_mut::<Taking>().0.push((
        drawn.rect(),
        words,
        SETTLE,
    ));
}

/// Captures what waits, once its frames are drawn.
fn take(world: &mut World) {
    if world.resource::<Taking>().0.is_empty() {
        return;
    }
    let mut ready = Vec::new();
    world.resource_mut::<Taking>().0.retain_mut(
        |(region, words, left)| {
            *left = left.saturating_sub(1);
            if *left == 0 {
                ready.push((*region, std::mem::take(words)));
            }
            *left > 0
        },
    );
    for (region, words) in ready {
        capture(world, region, words);
    }
}

/// Shows the chat, and puts `words` and `pictures` in its field.
pub(super) fn add(
    world: &mut World,
    words: &str,
    pictures: Vec<Vec<u8>>,
) {
    show_tab(world, HUB);
    if let Some(agent) =
        world.get_resource::<Convo>().map(|convo| convo.agent)
    {
        crate::convo::talk_to(world, agent);
    }
    chat::add_to_field(world, words, pictures);
}

/// `note` on a line of its own, when there is one.
pub(super) fn with_note(note: &str) -> String {
    match note.trim() {
        "" => String::new(),
        note => format!("{note}\n"),
    }
}

/// Captures the window as it is next drawn, and adds `region` of it,
/// in the window's pixels, with `words`.
pub(super) fn capture(
    world: &mut World,
    region: Rect,
    words: String,
) {
    world.spawn(Screenshot::primary_window()).observe(
        move |captured: On<ScreenshotCaptured>,
              mut commands: Commands| {
            let image = captured.image.clone();
            let words = words.clone();
            commands.queue(move |world: &mut World| {
                let Some(png) = cut(&image, region)
                    .and_then(|(pixels, size)| png(pixels, size))
                else {
                    return;
                };
                add(world, &words, vec![png]);
            });
        },
    );
}

/// The RGBA pixels of `region`, in the window's pixels, of a capture
/// of the window, and their size.
fn cut(window: &Image, region: Rect) -> Option<(Vec<u8>, UVec2)> {
    let data = window.data.as_ref()?;
    let full = UVec2::new(window.width(), window.height());
    let bgra = matches!(
        window.texture_descriptor.format,
        TextureFormat::Bgra8Unorm | TextureFormat::Bgra8UnormSrgb
    );
    let at = |value: f32, most: u32| {
        (value.round().max(0.0) as u32).min(most)
    };
    let (left, top) =
        (at(region.min.x, full.x), at(region.min.y, full.y));
    let (right, bottom) =
        (at(region.max.x, full.x), at(region.max.y, full.y));
    if right <= left || bottom <= top {
        return None;
    }
    let row = full.x as usize * 4;
    if data.len() < row * full.y as usize {
        return None;
    }
    let mut pixels = Vec::with_capacity(
        ((right - left) * (bottom - top) * 4) as usize,
    );
    for y in top..bottom {
        let start = y as usize * row + left as usize * 4;
        let end = y as usize * row + right as usize * 4;
        for [b, g, r, _] in data[start..end].as_chunks::<4>().0 {
            match bgra {
                true => pixels.extend([*r, *g, *b, 255]),
                false => pixels.extend([*b, *g, *r, 255]),
            }
        }
    }
    Some((pixels, UVec2::new(right - left, bottom - top)))
}

/// The RGBA `pixels` of `size`, as a PNG.
fn png(pixels: Vec<u8>, size: UVec2) -> Option<Vec<u8>> {
    let picture = RgbaImage::from_raw(size.x, size.y, pixels)?;
    let mut bytes = Cursor::new(Vec::new());
    picture.write_to(&mut bytes, ImageFormat::Png).ok()?;
    Some(bytes.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drawn(from: Vec2, to: Vec2) -> Drawn {
        // A sheet at (100, 50), 2000 by 1200 of the window's pixels,
        // on a window drawn at twice the interface's pixels.
        Drawn {
            area: Rect::new(100.0, 50.0, 2100.0, 1250.0),
            ui: 0.5,
            from,
            to,
            held: false,
        }
    }

    #[test]
    fn a_box_dragged_up_and_left_is_the_same_box() {
        let down =
            drawn(Vec2::new(300.0, 250.0), Vec2::new(500.0, 450.0));
        let up =
            drawn(Vec2::new(500.0, 450.0), Vec2::new(300.0, 250.0));
        assert_eq!(down.rect(), up.rect());
        assert_eq!(
            up.on_sheet(),
            Rect::new(100.0, 100.0, 200.0, 200.0)
        );
    }

    #[test]
    fn the_note_goes_under_the_box_or_over_it_at_the_bottom() {
        let high =
            drawn(Vec2::new(300.0, 250.0), Vec2::new(500.0, 450.0));
        assert_eq!(high.note_at(), Vec2::new(100.0, 208.0));
        // A box to the sheet's bottom right: the note over it,
        // inside.
        let low = drawn(
            Vec2::new(1900.0, 1000.0),
            Vec2::new(2100.0, 1250.0),
        );
        assert_eq!(low.note_at(), Vec2::new(672.0, 433.0));
    }
}
