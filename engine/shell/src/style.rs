//! The styles of the shell's widgets, built from the theme: buttons
//! of each kind, tabs, fields, switches, choices, headings, notes,
//! cards, lists, link rows, breadcrumbs, dividers, scroll areas and a
//! panel's content, and how a control glows and gives under the hand.

use std::fmt::Debug;

use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui::{Checked, InteractionDisabled};
use bevy::ui_widgets::{
    Activate, ActivateOnPress, Button as ButtonBehavior,
};
use bevy::window::SystemCursorIcon;
use bevy_fynix::tokens::{Motion, Tone};
use bevy_fynix::transition::blend_f32;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, button, column, dropdown as choose,
    frame, label, row, scroll, text_field,
};
use bevy_fynix::{
    Bevy, EntityCursor, Focused, Hovered, Pressed, Prop, ScopedExt,
    Style, ViewExt, component, style,
};
use fynix::{Patch, element};

use crate::fonts::DisplayText;
use crate::icons::{Glyph, glyph};
use crate::theme::EngineTheme;
use crate::{AnyView, Cx, View};

pub(crate) fn plugin(app: &mut App) {
    app.add_observer(pulse)
        .add_observer(note_press)
        .add_observer(release_inside)
        .add_observer(forget_press::<DragEnd>)
        .add_observer(forget_press::<Cancel>)
        .add_systems(
            Update,
            (
                (add_glow_state, animate, apply_glow, bottom_light)
                    .chain(),
                gradient,
                wrap,
                clip_inside,
                glow_margin,
                show_placeholders,
            ),
        );
}

/// The kind of a button, which sets its style and its glow.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ButtonStyle {
    /// An ordinary action: a background in a frame, its text in the
    /// accent.
    Plain,
    /// The one action a panel is for: lit low at rest, and more
    /// under the pointer.
    Primary,
    /// What refuses, stops or throws away.
    Critical,
    /// Text with no background, dim until the pointer is on it, as a
    /// way back.
    Quiet,
    /// A row of a list that leads somewhere: no background at rest.
    Row,
    /// A menu's title: text alone, lit from below.
    Title,
    /// A tab: text alone, bright on the one shown.
    Tab,
}

/// How a control glows, by what it is for.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Glow {
    /// No glow, only the give under a press.
    #[default]
    None,
    Plain,
    /// About its text alone, which has no background.
    Text,
    Primary,
    Critical,
    /// A field, lit while it is typed in.
    Field,
    /// About a switch's track, which is lit inside under the pointer.
    Switch,
    /// Lit inside only, from its bottom, as a row of a list is.
    Inner,
}

/// On the tab that is shown.
#[derive(Component, Clone, Copy, Debug)]
pub struct Active;

/// A title or a tab lit from below under the pointer, at a press,
/// and for as long as it is `held`: a gradient from the active colour
/// at its bottom to nothing at its top.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct BottomLight {
    pub held: bool,
    level: f32,
}

impl BottomLight {
    pub fn new(held: bool) -> Self {
        Self { held, level: 0.0 }
    }
}

/// A surface whose background shades toward its bottom.
#[derive(Component, Clone, Copy, Debug)]
pub struct BottomGradient;

/// Words shown in a field while it is empty.
#[derive(Component, Clone, Copy, Debug)]
pub struct Placeholder;

/// A row whose things go on to a line below when it is too narrow
/// for them.
#[derive(Component, Clone, Copy, Debug)]
pub struct Wraps;

/// A surface that cuts what it holds at the outer edge of its
/// padding.
#[derive(Component, Clone, Copy, Debug)]
pub struct ClipInside;

/// A scroll area that cuts what it holds as far past its edges as a
/// glow reaches, so the glow of a control at its edge shows whole.
#[derive(Component, Clone, Copy, Debug)]
pub struct GlowMargin;

/// A control's state under the hand, each 0 to 1, with its glow and
/// how far the glow reaches.
#[derive(Component, Clone, Copy, Default, Debug)]
pub struct GlowState {
    hover: f32,
    press: f32,
    pulse: f32,
    shake: f32,
    glow: f32,
    reach: f32,
}

/// On the root node of a [`switch`].
#[derive(Component, Clone, Copy, Debug)]
pub struct Switch;

/// A node moved across from where it is laid out, by `by` logical
/// pixels.
#[element(backend = Bevy)]
pub struct Slide {
    #[elem(patch = PatchSlide, blend = blend_f32)]
    pub by: Prop<f32>,
}

/// Writes how far a [`Slide`] is moved to its node.
pub struct PatchSlide;

impl Patch<Bevy, f32> for PatchSlide {
    fn patch(world: &mut World, node: Entity, by: &f32) {
        if let Some(mut laid) = world.get_mut::<Node>(node) {
            laid.left = px(*by);
        }
    }
}

/// On a button under a primary press: the node inside it the press
/// landed on.
#[derive(Component, Clone, Copy, Debug)]
struct PressedOn(Entity);

type Rules = Style<EngineTheme>;

impl ButtonStyle {
    /// Its glow.
    pub fn glow(self) -> Glow {
        match self {
            Self::Plain => Glow::Plain,
            Self::Row => Glow::Inner,
            Self::Primary => Glow::Primary,
            Self::Critical => Glow::Critical,
            Self::Quiet => Glow::Text,
            Self::Title | Self::Tab => Glow::None,
        }
    }

    /// Its rules, for [`ScopedExt::rules`].
    pub fn rules(
        self,
    ) -> impl FnOnce(&mut Cx) + Send + Sync + 'static {
        let rules = match self {
            Self::Plain => framed_button_rules()
                .frame(|f, t| {
                    f.fill(*t.colors.fill)
                        .border_color(*t.colors.border_strong)
                })
                .label(|l, _| l.tone(Tone::Faint))
                .icon(|i, _| i.tone(Tone::Faint))
                .hovered(|s| {
                    s.frame(|f, t| {
                        f.fill(*t.colors.accent_hover)
                            .border_color(*t.colors.accent_soft)
                    })
                    .tone(Tone::Body)
                })
                .pressed(|s| {
                    s.frame(|f, t| {
                        f.fill(*t.colors.surface_sunken)
                            .border_color(*t.colors.border_strong)
                    })
                    .tone(Tone::Dim)
                }),
            Self::Primary => framed_button_rules()
                .frame(|f, t| {
                    f.fill(*t.colors.active_low)
                        .border_color(*t.colors.accent)
                })
                .tone(Tone::Body)
                .hovered(|s| {
                    s.frame(|f, t| {
                        f.fill(*t.colors.active_low)
                            .border_color(*t.colors.active)
                    })
                })
                .pressed(|s| {
                    s.frame(|f, t| {
                        f.fill(*t.colors.active_low)
                            .border_color(*t.colors.border_strong)
                    })
                    .tone(Tone::Faint)
                }),
            Self::Critical => framed_button_rules()
                .frame(|f, t| {
                    f.fill(*t.colors.critical_surface)
                        .border_color(*t.colors.critical_muted)
                })
                .tone(Tone::Critical)
                .hovered(|s| {
                    s.frame(|f, t| {
                        f.fill(*t.colors.critical_hover)
                            .border_color(*t.colors.critical)
                    })
                    .tone(Tone::Body)
                })
                .pressed(|s| {
                    s.frame(|f, t| {
                        f.fill(*t.colors.surface_sunken)
                            .border_color(*t.colors.critical)
                    })
                    .tone(Tone::Critical)
                }),
            Self::Quiet => text_button_rules()
                .frame(|f, t| {
                    f.fill(Color::NONE)
                        .min_height(px(t.space.row))
                        .padding(UiRect::axes(
                            px(t.space.button_padding),
                            px(0.0),
                        ))
                })
                .tone(Tone::Dim)
                .hovered(|s| {
                    s.fill(|_| Color::NONE).tone(Tone::Accent)
                })
                .pressed(|s| {
                    s.fill(|_| Color::NONE).tone(Tone::Faint)
                }),
            Self::Row => text_button_rules()
                .frame(|f, t| {
                    f.fill(Color::NONE)
                        .radius(0.0)
                        .justify(JustifyContent::FlexStart)
                        .padding(UiRect::axes(
                            px(t.space.card_padding_x),
                            px(t.space.card_padding_y),
                        ))
                })
                .tone(Tone::Faint)
                .hovered(|s| {
                    s.fill(|t| *t.colors.fill).tone(Tone::Body)
                })
                .pressed(|s| {
                    s.fill(|t| *t.colors.surface_sunken)
                        .tone(Tone::Dim)
                }),
            Self::Title => style::<EngineTheme>()
                .frame(|f, t| {
                    f.fill(Color::NONE)
                        .height(percent(100.0))
                        .shrink(0.0)
                        .radius(0.0)
                        .overflow(Overflow::clip())
                        .padding(UiRect::axes(
                            px(t.space.title_padding),
                            px(0.0),
                        ))
                })
                .label(|l, t| {
                    l.size(t.text.heading)
                        .bold(true)
                        .tone(Tone::Faint)
                })
                .hovered(|s| s.fill(|_| Color::NONE).tone(Tone::Body))
                .pressed(|s| s.fill(|_| Color::NONE).tone(Tone::Dim)),
            Self::Tab => text_button_rules()
                .frame(|f, t| {
                    f.fill(Color::NONE)
                        .radius(0.0)
                        .overflow(Overflow::clip())
                        .padding(UiRect::all(px(0.0)))
                        .min_height(px(t.space.tab_height))
                })
                .tone(Tone::Dim)
                .hovered(|s| s.fill(|_| Color::NONE).tone(Tone::Body))
                .pressed(|s| {
                    s.fill(|_| Color::NONE).tone(Tone::Faint)
                })
                .when::<Active>(|s| s.tone(Tone::Body)),
        };
        rules.transition(Motion::Interact).bundle()
    }
}

/// The text of a button: small, bold.
fn text_button_rules() -> Rules {
    style::<EngineTheme>()
        .label(|l, t| l.size(t.text.small).bold(true))
        .icon(|i, t| i.size(t.space.icon))
}

/// Text on a background in a frame, a row high.
fn framed_button_rules() -> Rules {
    text_button_rules().frame(|f, t| {
        f.border(t.space.border)
            .radius(t.space.radius)
            .min_height(px(t.space.row))
            .padding(UiRect::axes(
                px(t.space.button_padding),
                px(0.0),
            ))
    })
}

/// The shell's styles on any view.
pub trait StyleExt: View + Sized {
    /// A button of the style `kind` showing this view.
    fn styled(self, kind: ButtonStyle) -> impl View {
        button(self).rules(kind.rules()).tagged(kind.glow())
    }

    /// The icon `which` before this view.
    fn with_icon(self, which: Glyph) -> AnyView {
        AnyView::new(move |cx: &mut Cx| {
            let size = cx.theme().space.icon;
            cx.build(icon_row((raised(glyph(which, size)), self)))
        })
    }

    /// A tab showing this view, lit and underlined across its width
    /// while `shown`.
    fn in_tab(self, shown: bool) -> AnyView {
        AnyView::new(move |cx: &mut Cx| {
            let theme = cx.theme();
            let (line, under, pad) = (
                theme.space.underline,
                *theme.colors.accent_soft,
                theme.space.gap / 2.0,
            );
            let underline = frame()
                .height(px(line))
                .width(percent(100.0))
                .fill(if shown { under } else { Color::NONE });
            let tab = column((
                column((self,))
                    .grow(1.0)
                    .padding(UiRect::axes(px(pad), px(0.0)))
                    .justify(JustifyContent::Center),
                underline,
            ))
            .gap(0.0)
            .align(AlignItems::Stretch)
            .styled(ButtonStyle::Tab)
            .tagged(BottomLight::new(shown));
            if shown {
                cx.build(tab.tagged(Active))
            } else {
                cx.build(tab)
            }
        })
    }

    /// A panel's content: this view, inside the panel's padding on a
    /// surface that shades toward its bottom, cut at the padding.
    fn in_panel(self) -> AnyView {
        AnyView::new(move |cx: &mut Cx| {
            let theme = cx.theme();
            let (fill, inset) =
                (*theme.colors.background, theme.space.panel);
            cx.build(
                column((self,))
                    .grow(1.0)
                    .min_width(px(0.0))
                    .min_height(px(0.0))
                    .padding(UiRect::all(px(inset)))
                    .gap(0.0)
                    .fill(fill)
                    .overflow(Overflow::clip())
                    .tagged(BottomGradient)
                    .tagged(ClipInside),
            )
        })
    }
}

impl<V: View> StyleExt for V {}

/// A button of the style `kind` showing `text` in the display face.
pub fn action(
    kind: ButtonStyle,
    text: impl Into<Prop<String>>,
) -> impl View {
    label(text).wrap(false).tagged(DisplayText).styled(kind)
}

/// A button of the style `kind` with the icon `which` before its
/// text, `text` in the display face.
pub fn icon_button(
    kind: ButtonStyle,
    which: Glyph,
    text: impl Into<Prop<String>>,
) -> impl View {
    content_button(
        kind,
        label(text).wrap(false).tagged(DisplayText).with_icon(which),
    )
}

/// `children` in a row as an icon and its text are set out.
pub fn icon_row<C>(children: C) -> AnyView
where
    C: bevy_fynix::ViewSeq<bevy_fynix::Bevy, EngineTheme> + 'static,
{
    AnyView::new(move |cx: &mut Cx| {
        let gap = cx.theme().space.icon_gap;
        cx.build(row(children).gap(gap).align(AlignItems::Center))
    })
}

/// `icon` set a little high to sit on the middle of the display
/// face's capitals.
pub fn raised(icon: impl View) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let offset = cx.theme().space.icon_offset;
        cx.build(
            row((icon,)).gap(0.0).padding(UiRect::bottom(px(offset))),
        )
    })
}

/// A button of the style `kind` showing `content`, padded as one with
/// an icon before its text is.
pub fn content_button(
    kind: ButtonStyle,
    content: impl View,
) -> impl View {
    button(content)
        .rules(
            style::<EngineTheme>()
                .frame(|f, t| {
                    // Less on the icon's side, which has air of its
                    // own.
                    f.padding(UiRect::new(
                        px(t.space.pad_start),
                        px(t.space.pad_end),
                        px(0.0),
                        px(0.0),
                    ))
                })
                .bundle(),
        )
        .rules(kind.rules())
        .tagged(kind.glow())
}

/// A menu's title, `text`, lit from below while `held`.
pub fn title_button(
    text: impl Into<Prop<String>>,
    held: bool,
) -> AnyView {
    column((label(text).wrap(false).tagged(DisplayText),))
        .gap(0.0)
        .height(percent(100.0))
        .justify(JustifyContent::Center)
        .styled(ButtonStyle::Title)
        .tagged(BottomLight::new(held))
        .boxed()
}

/// A text field showing `value`, handing what was typed to `commit`,
/// with `placeholder` in it while it is empty.
pub fn field(
    value: impl Into<Prop<String>>,
    placeholder: String,
    commit: impl Fn(&mut World, String) + Send + Sync + 'static,
) -> AnyView {
    let field = text_field(value, commit).rules(field_rules());
    if placeholder.is_empty() {
        return field.boxed();
    }
    AnyView::new(move |cx: &mut Cx| {
        let inset = cx.theme().space.gap + cx.theme().space.border;
        let words =
            row((label(placeholder).tone(Tone::Dim).wrap(false),))
                .position(PositionType::Absolute)
                .inset(UiRect {
                    left: px(inset),
                    right: Val::Auto,
                    top: px(0.0),
                    bottom: px(0.0),
                })
                .align(AlignItems::Center)
                .tagged(Placeholder)
                .tagged(Pickable::IGNORE);
        cx.build(column((field, words)).gap(0.0))
    })
    .boxed()
}

/// The rules of a field: a floor sunk into the panel, framed in a
/// border that brightens under the pointer and while it is typed in.
pub fn field_rules() -> impl FnOnce(&mut Cx) + Send + Sync + 'static {
    style::<EngineTheme>()
        .frame(|f, t| {
            f.fill(*t.colors.surface_sunken)
                .border(t.space.border)
                .border_color(*t.colors.border)
                .radius(t.space.radius)
        })
        .hovered(|s| {
            s.frame(|f, t| {
                f.fill(*t.colors.surface_sunken)
                    .border_color(*t.colors.border_strong)
            })
        })
        .when::<Focused>(|s| {
            s.frame(|f, t| {
                f.fill(*t.colors.surface_sunken)
                    .border_color(*t.colors.accent)
            })
        })
        .transition(Motion::Interact)
        .bundle()
}

/// A switch showing `on`: a track with its knob at the start while
/// off and at the end while on, handing the state asked for to
/// `change` when it is pressed. It never flips itself, so a bound
/// `on` follows what `change` writes.
pub fn switch(
    on: impl Into<Prop<bool>>,
    change: impl Fn(&mut World, bool) + Send + Sync + 'static,
) -> AnyView {
    switch_of(on.into(), None, change)
}

/// A [`switch`] with `text` beside it, pressed as one with it.
pub fn labelled_switch(
    text: impl Into<Prop<String>>,
    on: impl Into<Prop<bool>>,
    change: impl Fn(&mut World, bool) + Send + Sync + 'static,
) -> AnyView {
    switch_of(on.into(), Some(text.into()), change)
}

fn switch_of(
    on: Prop<bool>,
    text: Option<Prop<String>>,
    change: impl Fn(&mut World, bool) + Send + Sync + 'static,
) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let theme = cx.theme();
        let (space, colors) = (&theme.space, &theme.colors);
        let (width, height, inset, border, gap) = (
            space.switch_width,
            space.switch_height,
            space.switch_inset,
            space.border,
            space.gap,
        );
        let knob = height - 2.0 * (border + inset);
        let travel = width - height;
        let tracks = (*colors.accent, *colors.fill);
        let frames = (*colors.accent_soft, *colors.border_strong);
        let knobs = (*colors.text, *colors.text_muted);
        cx.scope(|cx| {
            style::<EngineTheme>()
                .tone(Tone::Faint)
                .hovered(|s| s.tone(Tone::Body))
                .pressed(|s| s.tone(Tone::Dim))
                .transition(Motion::Interact)
                .apply(cx);
            let root =
                cx.build(row(()).gap(gap).align(AlignItems::Center));
            cx.world.entity_mut(root).insert((
                ButtonBehavior,
                EntityCursor(SystemCursorIcon::Pointer),
                Switch,
                Glow::Switch,
            ));
            cx.effect(root, on, |world, node, &on| {
                let Ok(mut entity) = world.get_entity_mut(node)
                else {
                    return;
                };
                if !on {
                    entity.remove::<Checked>();
                } else if !entity.contains::<Checked>() {
                    entity.insert(Checked);
                }
            });
            let by_state = move |(on, off): (Color, Color)| {
                component::<Checked, _>(root, move |checked| {
                    if checked.is_some() { on } else { off }
                })
            };
            cx.under(root, |cx| {
                let track = cx.build(
                    frame()
                        .width(px(width))
                        .height(px(height))
                        .shrink(0.0)
                        .radius(height / 2.0)
                        .border(border)
                        .border_color(by_state(frames))
                        .fill(by_state(tracks))
                        .padding(UiRect::all(px(inset)))
                        .align(AlignItems::Center),
                );
                cx.under(track, |cx| {
                    let by = component::<Checked, _>(
                        root,
                        move |checked| {
                            if checked.is_some() {
                                travel
                            } else {
                                0.0
                            }
                        },
                    );
                    let slide = cx.build(Slide { by: by.into() });
                    cx.under(slide, |cx| {
                        cx.build(
                            frame()
                                .width(px(knob))
                                .height(px(knob))
                                .radius(knob / 2.0)
                                .fill(by_state(knobs)),
                        )
                    });
                });
                if let Some(text) = text {
                    cx.build(label(text));
                }
            });
            root
        })
    })
    .on_activate_with(move |world, node| {
        let on = world.get::<Checked>(node).is_some();
        change(world, !on);
    })
    .boxed()
}

/// One of `options`, the one at `selected`, which opens a list of
/// them to pick another from, as wide as it is given. `pick` is
/// handed the place of the one picked.
pub fn dropdown(
    options: Vec<String>,
    selected: impl Into<Prop<usize>>,
    pick: impl Fn(&mut World, usize) + Send + Sync + 'static,
) -> AnyView {
    let selected = selected.into();
    AnyView::new(move |cx: &mut Cx| {
        let size = cx.theme().space.icon;
        cx.build(
            choose(
                options,
                selected,
                Glyph::ChevronUp.image(size),
                pick,
            )
            .width(percent(100.0))
            .max_width(percent(100.0)),
        )
    })
}

/// The name at the head of a page.
pub fn heading(text: impl Into<Prop<String>>) -> AnyView {
    let text = text.into();
    AnyView::new(move |cx: &mut Cx| {
        let size = cx.theme().text.title;
        cx.build(
            label(text)
                .size(size)
                .bold(true)
                .wrap(false)
                .tagged(DisplayText),
        )
    })
}

/// The name of a part of a panel: small, in the display face.
pub fn section(text: impl Into<Prop<String>>) -> AnyView {
    let text = text.into();
    AnyView::new(move |cx: &mut Cx| {
        let size = cx.theme().text.small;
        cx.build(
            label(text)
                .size(size)
                .bold(true)
                .tone(Tone::Faint)
                .tagged(DisplayText),
        )
    })
}

/// Small words that stand back.
pub fn note(text: impl Into<Prop<String>>) -> AnyView {
    let text = text.into();
    AnyView::new(move |cx: &mut Cx| {
        let size = cx.theme().text.small;
        cx.build(label(text).size(size).tone(Tone::Dim))
    })
}

/// `content` in a card: a shade above the panel, in a frame.
pub fn card<C>(content: C) -> AnyView
where
    C: bevy_fynix::ViewSeq<bevy_fynix::Bevy, EngineTheme> + 'static,
{
    AnyView::new(move |cx: &mut Cx| {
        let theme = cx.theme();
        let (padding_x, padding_y, border, line, radius, fill, gap) = (
            theme.space.card_padding_x,
            theme.space.card_padding_y,
            theme.space.border,
            *theme.colors.border,
            theme.space.radius,
            *theme.colors.surface,
            theme.space.gap / 2.0,
        );
        cx.build(
            column(content)
                .gap(gap)
                .padding(UiRect::axes(px(padding_x), px(padding_y)))
                .border(border)
                .border_color(line)
                .radius(radius)
                .fill(fill),
        )
    })
}

/// A line across, between two parts.
pub fn divider() -> AnyView {
    line(true.into())
}

/// A divider, taking no room while not `shown`.
fn line(shown: Prop<bool>) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let theme = cx.theme();
        let (line, colour) =
            (theme.space.divider, *theme.colors.border);
        let display = shown.map(|shown| {
            if shown { Display::Flex } else { Display::None }
        });
        cx.build(
            frame().height(px(line)).fill(colour).display(display),
        )
    })
}

/// `rows` as one list, each padded, a divider between each and the
/// next.
pub fn list(rows: Vec<AnyView>) -> AnyView {
    let rows = rows
        .into_iter()
        .enumerate()
        .map(|(at, one)| list_row(one, at > 0))
        .collect::<Vec<_>>();
    column(rows).gap(0.0).boxed()
}

/// A row of a [`list`]: `content` padded, under a divider while
/// `divided`.
pub fn list_row(
    content: impl View,
    divided: impl Into<Prop<bool>>,
) -> AnyView {
    let divided = divided.into();
    AnyView::new(move |cx: &mut Cx| {
        let pad = cx.theme().space.row_padding;
        cx.build(
            column((
                line(divided),
                column((content,))
                    .gap(0.0)
                    .padding(UiRect::vertical(px(pad))),
            ))
            .gap(0.0),
        )
    })
}

/// `children` in a column that scrolls, taking the height left. Glows
/// reach past its sides, but what scrolls is cut at its top and
/// bottom, so it never shows over what is above or below it.
pub fn scroll_area<C>(children: C) -> AnyView
where
    C: bevy_fynix::ViewSeq<bevy_fynix::Bevy, EngineTheme> + 'static,
{
    AnyView::new(move |cx: &mut Cx| {
        let gap = cx.theme().space.gap;
        cx.build(
            column((scroll(children)
                .gap(gap)
                .grow(1.0)
                .min_height(px(0.0))
                .tagged(GlowMargin),))
            .gap(0.0)
            .grow(1.0)
            .min_height(px(0.0))
            .overflow(Overflow::clip_y()),
        )
    })
}

/// The line over a card that waits for the person: small capitals
/// in `tone`.
pub fn caption(text: impl Into<Prop<String>>, tone: Tone) -> AnyView {
    let text = text.into();
    AnyView::new(move |cx: &mut Cx| {
        let size = cx.theme().text.caption;
        cx.build(
            label(text)
                .size(size)
                .bold(true)
                .tone(tone)
                .tagged(DisplayText),
        )
    })
}

/// `buttons` in a row that wraps, so a narrow panel never cuts one
/// off.
pub fn buttons(buttons: Vec<AnyView>) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let gap = cx.theme().space.gap;
        cx.build(
            row(buttons)
                .gap(gap)
                .align(AlignItems::Center)
                .tagged(Wraps),
        )
    })
}

/// The way into a part of something, as a row of a list: its `title`
/// under `which`, what it `holds` if anything, and a mark that it
/// opens.
pub fn link_row(
    which: Glyph,
    title: impl Into<Prop<String>>,
    holds: Option<String>,
) -> AnyView {
    let title = title.into();
    AnyView::new(move |cx: &mut Cx| {
        let theme = cx.theme();
        let (gap, icon, small, section) = (
            theme.space.gap,
            theme.space.icon,
            theme.text.small,
            theme.space.section,
        );
        let mut lines = vec![
            label(title)
                .bold(true)
                .wrap(false)
                .tagged(DisplayText)
                .with_icon(which),
        ];
        if let Some(holds) = holds {
            lines.push(
                label(holds)
                    .size(small)
                    .bold(false)
                    .tone(Tone::Dim)
                    .boxed(),
            );
        }
        cx.build(
            row((
                column(lines)
                    .gap(gap / 2.0)
                    .grow(1.0)
                    .min_width(px(0.0)),
                glyph(Glyph::ChevronRight, icon).tone(Tone::Dim),
            ))
            .gap(section)
            .grow(1.0)
            .align(AlignItems::Center)
            .styled(ButtonStyle::Row),
        )
    })
}

/// `rows` as one list, a divider between each and the next.
pub fn link_rows(rows: Vec<AnyView>) -> AnyView {
    let mut parts = Vec::new();
    for row in rows {
        if !parts.is_empty() {
            parts.push(divider());
        }
        parts.push(row);
    }
    column(parts).gap(0.0).boxed()
}

/// The head of a page reached through a link row: the way back to
/// `front`, which `back` takes, and the page's `name` under `which`.
pub fn page_head(
    front: impl Into<Prop<String>>,
    back: impl Fn(&mut World) + Send + Sync + 'static,
    which: Glyph,
    name: impl Into<Prop<String>>,
) -> AnyView {
    let (front, name) = (front.into(), name.into());
    AnyView::new(move |cx: &mut Cx| {
        let section = cx.theme().space.section;
        cx.build(
            row((
                icon_button(ButtonStyle::Quiet, Glyph::Back, front)
                    .on_activate(back),
                label(name)
                    .bold(true)
                    .wrap(false)
                    .tone(Tone::Faint)
                    .tagged(DisplayText)
                    .with_icon(which),
            ))
            .gap(section)
            .align(AlignItems::Center),
        )
    })
}

/// Where a page is within its panel: a quiet button for each of
/// `trail`, the pages above it from the top, each followed by a dim
/// `/`, and then `current`, the page shown. A press hands the place
/// in `trail` of its page to `go`.
pub fn breadcrumb<F>(
    trail: Vec<String>,
    current: impl Into<Prop<String>>,
    go: F,
) -> AnyView
where
    F: Fn(&mut World, usize) + Clone + Send + Sync + 'static,
{
    let current = current.into();
    AnyView::new(move |cx: &mut Cx| {
        let theme = cx.theme();
        let (gap, size) = (theme.space.gap, theme.text.small);
        let mut parts = Vec::new();
        for (at, text) in trail.into_iter().enumerate() {
            let go = go.clone();
            parts.push(
                button(label(text).wrap(false).tagged(DisplayText))
                    .rules(
                        // Bare, so the trail starts at the panel's
                        // edge and the parts sit evenly apart.
                        style::<EngineTheme>()
                            .frame(|f, _| {
                                f.padding(UiRect::all(px(0.0)))
                            })
                            .bundle(),
                    )
                    .rules(ButtonStyle::Quiet.rules())
                    .tagged(ButtonStyle::Quiet.glow())
                    .on_activate(move |world| go(world, at))
                    .boxed(),
            );
            parts.push(
                label("/")
                    .size(size)
                    .tone(Tone::Dim)
                    .wrap(false)
                    .tagged(DisplayText)
                    .boxed(),
            );
        }
        parts.push(
            label(current)
                .size(size)
                .bold(true)
                .tone(Tone::Faint)
                .wrap(false)
                .tagged(DisplayText)
                .boxed(),
        );
        cx.build(
            row(parts)
                .gap(gap)
                .align(AlignItems::Center)
                .tagged(Wraps),
        )
    })
}

/// One answer of several to pick from, each showing `labels`, the
/// one at `picked` lit. A press hands the place of its answer to
/// `pick`.
pub fn choices<F>(
    labels: Vec<String>,
    picked: usize,
    pick: F,
) -> AnyView
where
    F: Fn(&mut World, usize) + Clone + Send + Sync + 'static,
{
    let options = labels
        .into_iter()
        .enumerate()
        .map(|(at, text)| {
            let pick = pick.clone();
            button(label(text).wrap(false))
                .rules(choice_rules(at == picked))
                .on_activate(move |world| pick(world, at))
                .boxed()
        })
        .collect::<Vec<_>>();
    AnyView::new(move |cx: &mut Cx| {
        let gap = cx.theme().space.gap / 2.0;
        cx.build(
            row(options)
                .gap(gap)
                .align(AlignItems::Center)
                .tagged(Wraps),
        )
    })
}

/// The rules of an answer to pick: bare until the pointer is on it,
/// and the one picked a lit background in a frame.
fn choice_rules(
    picked: bool,
) -> impl FnOnce(&mut Cx) + Send + Sync + 'static {
    let base = text_button_rules().frame(|f, t| {
        f.border(t.space.border)
            .radius(t.space.radius)
            .min_height(px(t.space.row))
            .padding(UiRect::axes(px(t.space.gap), px(0.0)))
    });
    let rules = if picked {
        base.frame(|f, t| {
            f.fill(*t.colors.active_low)
                .border_color(*t.colors.accent)
        })
        .tone(Tone::Body)
        .hovered(|s| {
            s.frame(|f, t| {
                f.fill(*t.colors.active_low)
                    .border_color(*t.colors.active)
            })
        })
    } else {
        base.frame(|f, _| {
            f.fill(Color::NONE).border_color(Color::NONE)
        })
        .tone(Tone::Faint)
        .hovered(|s| {
            s.frame(|f, t| {
                f.fill(*t.colors.hover).border_color(Color::NONE)
            })
            .tone(Tone::Body)
        })
        .pressed(|s| {
            s.frame(|f, t| {
                f.fill(*t.colors.surface_sunken)
                    .border_color(Color::NONE)
            })
        })
    };
    rules.transition(Motion::Interact).bundle()
}

/// Moves `value` toward `to` at `rate` per second.
fn ease(value: f32, to: f32, rate: f32, dt: f32) -> f32 {
    let next = to + (value - to) * (-rate * dt).exp();
    if (next - to).abs() < 0.002 { to } else { next }
}

/// The unsteadiness of a glow at `now`, -1 to 1, its own for each
/// `seed`.
fn flicker_offset(now: f32, seed: f32) -> f32 {
    0.5 * (now * 5.3 + seed).sin()
        + 0.3 * (now * 8.9 + seed * 1.7).sin()
        + 0.2 * (now * 2.1 + seed * 0.6).sin()
}

/// Gives every button and field a state under the hand.
fn add_glow_state(
    mut commands: Commands,
    buttons: Query<
        Entity,
        (With<ButtonBehavior>, Without<GlowState>),
    >,
    fields: Query<&ChildOf, Added<EditableText>>,
) {
    for field in &fields {
        commands
            .entity(field.parent())
            .try_insert((GlowState::default(), Glow::Field));
    }
    for button in &buttons {
        commands.entity(button).try_insert((
            GlowState::default(),
            UiTransform::IDENTITY,
        ));
    }
}

/// A button that has done its work pulses, or shakes if what it did
/// was to refuse.
fn pulse(
    activated: On<Activate>,
    mut controls: Query<(&mut GlowState, Option<&Glow>)>,
) {
    let Ok((mut state, glow)) = controls.get_mut(activated.entity)
    else {
        return;
    };
    match glow.copied().unwrap_or_default() {
        Glow::Field => {}
        Glow::Critical => state.shake = 1.0,
        _ => state.pulse = 1.0,
    }
}

type Handled<'a> = (
    Entity,
    &'a mut GlowState,
    Option<&'a mut UiTransform>,
    Has<Focused>,
    Has<Hovered>,
    Has<Pressed>,
    Option<&'a Glow>,
);

/// Eases each control toward the hand: its glow, its give under a
/// press, and the pulse or shake of work done.
fn animate(
    time: Res<Time>,
    theme: Res<EngineTheme>,
    mut controls: Query<Handled>,
) {
    let set = &theme.glow;
    let dt = time.delta_secs();
    let now = time.elapsed_secs();
    for (id, mut state, transform, focused, hovered, held, glow) in
        &mut controls
    {
        let glow = glow.copied().unwrap_or_default();
        let over = if glow == Glow::Field {
            focused
        } else {
            hovered
        };
        let hover =
            ease(state.hover, f32::from(over), set.hover_rate, dt);
        let press =
            ease(state.press, f32::from(held), set.press_rate, dt);
        let pulse = ease(state.pulse, 0.0, set.fade_rate, dt);
        let shake = ease(state.shake, 0.0, set.fade_rate, dt);
        let still = hover == state.hover
            && press == state.press
            && pulse == state.pulse
            && shake == state.shake;
        // Its glow at rest and under the pointer or the caret, and
        // the glow its work adds.
        let (rest, near, done) = match glow {
            Glow::None => (0.0, 0.0, 0.0),
            Glow::Field => (0.0, set.field, 0.0),
            Glow::Plain | Glow::Text | Glow::Switch | Glow::Inner => {
                (0.0, set.plain, set.done_gain * pulse)
            }
            Glow::Primary => (set.primary_rest, set.primary, pulse),
            Glow::Critical => {
                (0.0, set.critical, set.done_gain * shake)
            }
        };
        let steady = rest + (near - rest) * hover;
        if still && steady == 0.0 && state.glow == 0.0 {
            continue;
        }
        // A pulse swells and falls back as it dies away.
        let swell = 4.0 * pulse * (1.0 - pulse);
        let scale =
            1.0 - set.press_shrink * press + set.pulse_grow * swell;
        let slide = shake * set.shake * (now * set.shake_rate).sin();
        let want = UiTransform {
            translation: Val2::new(
                px(slide),
                px(set.press_offset * press),
            ),
            scale: Vec2::splat(scale),
            ..UiTransform::IDENTITY
        };
        if let Some(mut transform) = transform
            && *transform != want
        {
            *transform = want;
        }
        // The hand covers the glow, and the work done turns it up.
        let covered = steady * (1.0 - set.press_dim * press);
        let lit = covered + (1.0 - covered) * done;
        let seed = id.index_u32() as f32 * 1.618;
        let flicker = 1.0 + set.flicker * flicker_offset(now, seed);
        *state = GlowState {
            hover,
            press,
            pulse,
            shake,
            glow: (lit * flicker).clamp(0.0, 1.0),
            reach: set.blur + set.pulse_blur * pulse.max(shake),
        };
    }
}

/// Draws each control's glow as shadows of its colours just below it,
/// or below its text or its track where it has no background, and
/// lights the inside of the primary action, a field being typed in
/// and a switch's track from the bottom.
fn apply_glow(
    theme: Res<EngineTheme>,
    controls: Query<(Entity, &GlowState, &Glow, Option<&Children>)>,
    shadows: Query<&BoxShadow>,
    gradients: Query<&BackgroundGradient>,
    mut commands: Commands,
) {
    let (set, colors) = (&theme.glow, &theme.colors);
    for (control, state, glow, within) in &controls {
        let target = match glow {
            Glow::None => continue,
            Glow::Text | Glow::Switch => {
                let Some(first) =
                    within.and_then(|within| within.first())
                else {
                    continue;
                };
                *first
            }
            _ => control,
        };
        if matches!(
            glow,
            Glow::Primary | Glow::Field | Glow::Switch | Glow::Inner
        ) {
            let inner = set.inner * state.glow;
            let now = gradients.get(target).ok();
            if inner <= 0.0 {
                if now.is_some() {
                    commands
                        .entity(target)
                        .try_remove::<BackgroundGradient>();
                }
            } else {
                let color = colors.active.with_alpha(inner);
                let want = BackgroundGradient::from(
                    LinearGradient::to_top(vec![
                        ColorStop::new(color, percent(0.0)),
                        ColorStop::new(
                            color.with_alpha(0.0),
                            percent(100.0),
                        ),
                    ]),
                );
                if now != Some(&want) {
                    commands.entity(target).try_insert(want);
                }
            }
        }
        if *glow == Glow::Inner {
            continue;
        }
        let (hue, core) = match glow {
            Glow::Critical => {
                (*colors.critical_glow, *colors.critical_strong)
            }
            _ => (*colors.active, *colors.active_strong),
        };
        let thrown = match glow {
            Glow::Critical | Glow::Field => {
                set.spread * set.spread_muted
            }
            _ => set.spread,
        };
        let reach = match glow {
            Glow::Text => state.reach * set.text_reach,
            _ => state.reach,
        };
        let strength = state.glow * thrown;
        let now = shadows.get(target).ok();
        if strength <= 0.0 {
            if now.is_some() {
                commands.entity(target).try_remove::<BoxShadow>();
            }
            continue;
        }
        let want = BoxShadow(vec![
            ShadowStyle {
                color: hue.with_alpha(strength),
                x_offset: px(0.0),
                y_offset: px(reach * set.drop),
                spread_radius: px(0.0),
                blur_radius: px(reach),
            },
            ShadowStyle {
                color: core.with_alpha(strength * set.core),
                x_offset: px(0.0),
                y_offset: px(reach * set.core * set.drop),
                spread_radius: px(0.0),
                blur_radius: px(reach * set.core),
            },
        ]);
        if now != Some(&want) {
            commands.entity(target).try_insert(want);
        }
    }
}

/// Eases each bottom light toward the pointer and its hold, flares it
/// at a press, and draws it as a gradient from the active colour at
/// the bottom to nothing at the top.
fn bottom_light(
    time: Res<Time>,
    theme: Res<EngineTheme>,
    mut lights: Query<(
        Entity,
        &mut BottomLight,
        Has<Hovered>,
        Option<&GlowState>,
        Has<BackgroundGradient>,
    )>,
    mut commands: Commands,
) {
    let set = &theme.glow;
    let dt = time.delta_secs();
    for (node, mut light, hovered, state, lit) in &mut lights {
        let want = f32::from(hovered || light.held);
        let level = ease(light.level, want, set.hover_rate, dt);
        let flare = state.map_or(0.0, |state| state.pulse);
        if level == light.level
            && flare == 0.0
            && lit == (level > 0.0)
        {
            continue;
        }
        light.level = level;
        let strength = set.bottom * level + set.bottom_press * flare;
        if strength <= 0.0 {
            commands.entity(node).try_remove::<BackgroundGradient>();
            continue;
        }
        let color = theme.colors.active.with_alpha(strength);
        commands.entity(node).try_insert(BackgroundGradient::from(
            LinearGradient::to_top(vec![
                ColorStop::new(color, percent(0.0)),
                ColorStop::new(color.with_alpha(0.0), percent(100.0)),
            ]),
        ));
    }
}

/// Gives each new [`BottomGradient`] surface its shading toward its
/// bottom.
fn gradient(
    theme: Res<EngineTheme>,
    surfaces: Query<Entity, Added<BottomGradient>>,
    mut commands: Commands,
) {
    for surface in &surfaces {
        let stops = vec![
            ColorStop::new(
                *theme.colors.background,
                percent(theme.space.bottom_start * 100.0),
            ),
            ColorStop::new(
                *theme.colors.background_bottom,
                percent(100.0),
            ),
        ];
        commands.entity(surface).try_insert(
            BackgroundGradient::from(LinearGradient::to_bottom(
                stops,
            )),
        );
    }
}

/// Lets each new [`Wraps`] row wrap, its lines as far apart as the
/// things in one.
fn wrap(mut rows: Query<&mut Node, Added<Wraps>>) {
    for mut row in &mut rows {
        row.flex_wrap = FlexWrap::Wrap;
        row.row_gap = row.column_gap;
    }
}

/// Cuts what each new [`ClipInside`] surface holds at the outer edge
/// of its padding, so the glow of a control at the edge of its
/// content has the padding to spread into.
fn clip_inside(mut nodes: Query<&mut Node, Added<ClipInside>>) {
    for mut node in &mut nodes {
        node.overflow_clip_margin = OverflowClipMargin::padding_box();
    }
}

/// Notes the node inside a button a primary press landed on.
fn note_press(
    press: On<Pointer<Press>>,
    buttons: Query<
        (),
        (With<ButtonBehavior>, Without<ActivateOnPress>),
    >,
    mut commands: Commands,
) {
    let button = press.event_target();
    if press.button == PointerButton::Primary
        && buttons.contains(button)
    {
        commands
            .entity(button)
            .try_insert(PressedOn(press.original_event_target()));
    }
}

/// Activates a button when a press and its release land inside it on
/// different nodes, as on the label and then the icon, which Bevy
/// does not count as a click.
fn release_inside(
    release: On<Pointer<Release>>,
    buttons: Query<(&PressedOn, Has<InteractionDisabled>)>,
    mut commands: Commands,
) {
    let button = release.event_target();
    if release.button != PointerButton::Primary {
        return;
    }
    let Ok((pressed, disabled)) = buttons.get(button) else {
        return;
    };
    commands.entity(button).try_remove::<PressedOn>();
    if pressed.0 != release.original_event_target() && !disabled {
        commands.trigger(Activate { entity: button });
    }
}

/// Forgets a press on a button that ends without a release inside it.
fn forget_press<E: Debug + Clone + Reflect>(
    event: On<Pointer<E>>,
    pressed: Query<(), With<PressedOn>>,
    mut commands: Commands,
) {
    let button = event.event_target();
    if pressed.contains(button) {
        commands.entity(button).try_remove::<PressedOn>();
    }
}

/// Gives each [`GlowMargin`] area room for the farthest glow, again
/// whenever the theme changes.
fn glow_margin(
    theme: Res<EngineTheme>,
    mut nodes: Query<(Ref<GlowMargin>, &mut Node)>,
) {
    let set = &theme.glow;
    let room = (set.blur + set.pulse_blur) * (1.0 + set.drop);
    for (margin, mut node) in &mut nodes {
        if theme.is_changed() || margin.is_added() {
            node.overflow_clip_margin =
                OverflowClipMargin::content_box().with_margin(room);
        }
    }
}

/// Shows each placeholder while its field is empty.
fn show_placeholders(
    mut placeholders: Query<
        (&ChildOf, &mut Visibility),
        With<Placeholder>,
    >,
    children: Query<&Children>,
    texts: Query<&EditableText>,
) {
    for (parent, mut shown) in &mut placeholders {
        let empty = children
            .iter_descendants(parent.parent())
            .find_map(|node| texts.get(node).ok())
            .is_none_or(|text| text.value().chars().next().is_none());
        let want = if empty {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        shown.set_if_neq(want);
    }
}

#[cfg(test)]
mod tests {
    use bevy::camera::NormalizedRenderTarget;
    use bevy::picking::backend::HitData;
    use bevy::picking::pointer::{Location, PointerId};
    use bevy::time::TimePlugin;

    use super::*;

    #[derive(Resource, Default)]
    struct Activated(usize);

    /// Sends what picking sends for `event` on `on`, bubbling up.
    fn send<E: Debug + Clone + Reflect>(
        world: &mut World,
        on: Entity,
        event: E,
    ) {
        let location = Location {
            target: NormalizedRenderTarget::None {
                width: 1,
                height: 1,
            },
            position: Vec2::ZERO,
        };
        world.trigger(Pointer::new(
            PointerId::Mouse,
            location,
            event,
            on,
        ));
        world.flush();
    }

    fn hit() -> HitData {
        HitData::new(Entity::PLACEHOLDER, 0.0, None, None)
    }

    fn press(world: &mut World, on: Entity) {
        let (button, hit) = (PointerButton::Primary, hit());
        send(
            world,
            on,
            Press {
                button,
                hit,
                count: 1,
            },
        );
    }

    fn release(world: &mut World, on: Entity) {
        let (button, hit) = (PointerButton::Primary, hit());
        send(world, on, Release { button, hit });
    }

    #[test]
    fn a_press_and_a_release_inside_a_button_activate_it() {
        let mut world = World::new();
        // Pointer events bubble only where windows are known.
        world.register_component::<Window>();
        world.init_resource::<Activated>();
        world.add_observer(note_press);
        world.add_observer(release_inside);
        world.add_observer(forget_press::<DragEnd>);
        world.add_observer(
            |_: On<Activate>, mut activated: ResMut<Activated>| {
                activated.0 += 1;
            },
        );
        let button = world.spawn(ButtonBehavior).id();
        let label = world.spawn(ChildOf(button)).id();
        let icon = world.spawn(ChildOf(button)).id();
        let outside = world.spawn_empty().id();
        let activated =
            |world: &World| world.resource::<Activated>().0;

        press(&mut world, label);
        release(&mut world, icon);
        assert_eq!(activated(&world), 1);

        // On one node, it is a click, left to the button itself.
        press(&mut world, label);
        release(&mut world, label);
        assert_eq!(activated(&world), 1);

        // Let go outside, it is forgotten.
        press(&mut world, label);
        release(&mut world, outside);
        let end = DragEnd {
            button: PointerButton::Primary,
            distance: Vec2::ONE,
        };
        send(&mut world, label, end);
        release(&mut world, icon);
        assert_eq!(activated(&world), 1);
    }

    #[derive(Resource)]
    struct Lit(bool);

    #[test]
    fn a_switch_asks_for_the_state_it_is_not_in() {
        let mut app = App::new();
        app.add_plugins((
            TaskPoolPlugin::default(),
            TimePlugin,
            crate::views_plugin,
        ))
        .insert_resource(Lit(false));
        let root = bevy_fynix::mount::<EngineTheme>(
            app.world_mut(),
            switch(
                bevy_fynix::resource::<Lit, _>(|lit| lit.0),
                |world, on| world.resource_mut::<Lit>().0 = on,
            ),
        );
        for want in [true, false] {
            app.world_mut().trigger(Activate { entity: root });
            app.update();
            assert_eq!(app.world().resource::<Lit>().0, want);
            assert_eq!(
                app.world().get::<Checked>(root).is_some(),
                want
            );
        }
    }
}
