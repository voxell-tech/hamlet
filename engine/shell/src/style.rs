//! The styles of the shell's widgets, built from the theme: buttons
//! of each kind, fields, toggles, choices, headings, notes, cards,
//! lists, link rows, dividers and scroll areas, and how a control
//! glows and gives under the hand.

use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui_widgets::{Activate, Button as ButtonBehavior};
use bevy_fynix::tokens::{Motion, Tone};
use bevy_fynix::views::{
    BehaviorExt, FrameProps, button, checkbox, column, frame, label,
    row, scroll, text_field,
};
use bevy_fynix::{
    Focused, Hovered, Pressed, Prop, ScopedExt, Style, ViewExt, style,
};

use crate::fonts::DisplayText;
use crate::icons::{Glyph, glyph};
use crate::theme::EngineTheme;
use crate::{AnyView, Cx, View};

pub(crate) fn plugin(app: &mut App) {
    app.add_observer(answer).add_systems(
        Update,
        (
            (arm, animate, shine, bottom_light).chain(),
            shade,
            wrap,
            clip_inside,
            glow_margin,
            show_placeholders,
            pick_whole_buttons,
        ),
    );
}

/// The kind of a button, which sets its style and its glow.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ButtonStyle {
    /// An ordinary action: a plate in a frame, lettered in the
    /// accent.
    Plain,
    /// The one action a panel is for: lit low at rest, and more
    /// under the pointer.
    Primary,
    /// What refuses, stops or throws away.
    Critical,
    /// Lettering with no plate, dim until the pointer is on it, as a
    /// way back.
    Quiet,
    /// A row of a list that leads somewhere: no plate at rest.
    Row,
    /// A menu's title: lettering alone, lit from below.
    Title,
    /// A tab: lettering alone, bright on the one shown.
    Tab,
}

/// How a control glows, by what it is for.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Glow {
    /// No glow, only the give under a press.
    #[default]
    None,
    Plain,
    /// About its lettering alone, which has no plate.
    Words,
    Primary,
    Critical,
    /// A field, lit while it is typed in.
    Field,
}

/// On the tab that is shown.
#[derive(Component, Clone, Copy, Debug)]
pub struct Shown;

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
pub struct Shade;

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
    heat: f32,
    press: f32,
    pulse: f32,
    shake: f32,
    glow: f32,
    reach: f32,
}

type Rules = Style<EngineTheme>;

impl ButtonStyle {
    /// Its glow.
    pub fn glow(self) -> Glow {
        match self {
            Self::Plain | Self::Row => Glow::Plain,
            Self::Primary => Glow::Primary,
            Self::Critical => Glow::Critical,
            Self::Quiet => Glow::Words,
            Self::Title | Self::Tab => Glow::None,
        }
    }

    /// Its rules, for [`ScopedExt::rules`].
    pub fn rules(
        self,
    ) -> impl FnOnce(&mut Cx) + Send + Sync + 'static {
        let rules = match self {
            Self::Plain => plated()
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
            Self::Primary => plated()
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
            Self::Critical => plated()
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
            Self::Quiet => lettered()
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
            Self::Row => lettered()
                .frame(|f, t| {
                    f.fill(Color::NONE)
                        .radius(t.space.round)
                        .justify(JustifyContent::FlexStart)
                        .padding(UiRect::axes(
                            px(t.space.card_across),
                            px(t.space.card_down),
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
            Self::Tab => lettered()
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
                .when::<Shown>(|s| s.tone(Tone::Body)),
        };
        rules.transition(Motion::Interact).bundle()
    }
}

/// Lettering of a button: small, bold.
fn lettered() -> Rules {
    style::<EngineTheme>()
        .label(|l, t| l.size(t.text.small).bold(true))
        .icon(|i, t| i.size(t.space.icon))
}

/// Lettering on a plate in a frame, a row high.
fn plated() -> Rules {
    lettered().frame(|f, t| {
        f.border(t.space.border)
            .radius(t.space.round)
            .min_height(px(t.space.row))
            .padding(UiRect::axes(
                px(t.space.button_padding),
                px(0.0),
            ))
    })
}

/// A button of the style `kind` showing `content`.
pub fn styled<C: View>(kind: ButtonStyle, content: C) -> impl View {
    button(content).rules(kind.rules()).tagged(kind.glow())
}

/// A button of the style `kind` lettered `text` in the display face.
pub fn action(
    kind: ButtonStyle,
    text: impl Into<Prop<String>>,
) -> impl View {
    styled(kind, label(text).wrap(false).tagged(DisplayText))
}

/// A button of the style `kind` with the icon `which` before its
/// lettering, `text` in the display face.
pub fn marked(
    kind: ButtonStyle,
    which: Glyph,
    text: impl Into<Prop<String>>,
) -> impl View {
    button(headed(which, label(text).wrap(false).tagged(DisplayText)))
        .rules(
            style::<EngineTheme>()
                .frame(|f, t| {
                    // Less on the icon's side, which has air of its
                    // own.
                    f.padding(UiRect::new(
                        px(t.space.mark_start),
                        px(t.space.mark_end),
                        px(0.0),
                        px(0.0),
                    ))
                })
                .bundle(),
        )
        .rules(kind.rules())
        .tagged(kind.glow())
}

/// The icon `which` before `words`, set a little high to sit on the
/// middle of the display face's capitals.
pub fn headed<V: View>(which: Glyph, words: V) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let theme = cx.theme();
        let (size, gap, lift) = (
            theme.space.icon,
            theme.space.icon_gap,
            theme.space.icon_lift,
        );
        cx.build(
            row((
                row((glyph(which, size),))
                    .gap(0.0)
                    .padding(UiRect::bottom(px(lift))),
                words,
            ))
            .gap(gap)
            .align(AlignItems::Center),
        )
    })
}

/// A menu's title, `text`, lit from below while `held`.
pub fn title_button(
    text: impl Into<Prop<String>>,
    held: bool,
) -> AnyView {
    styled(
        ButtonStyle::Title,
        column((label(text).wrap(false).tagged(DisplayText),))
            .gap(0.0)
            .height(percent(100.0))
            .justify(JustifyContent::Center),
    )
    .tagged(BottomLight::new(held))
    .boxed()
}

/// A tab showing `content`, lit and underlined across its width
/// while `shown`.
pub fn tab_button<C: View>(content: C, shown: bool) -> AnyView {
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
        let tab = styled(
            ButtonStyle::Tab,
            column((
                column((content,))
                    .grow(1.0)
                    .padding(UiRect::axes(px(pad), px(0.0)))
                    .justify(JustifyContent::Center),
                underline,
            ))
            .gap(0.0)
            .align(AlignItems::Stretch),
        )
        .tagged(BottomLight::new(shown));
        if shown {
            cx.build(tab.tagged(Shown))
        } else {
            cx.build(tab)
        }
    })
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
                .radius(t.space.round)
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

/// A toggle showing `on`, as large as a button is tall, handing the
/// state asked for to `change`.
pub fn toggle(
    on: impl Into<Prop<bool>>,
    change: impl Fn(&mut World, bool) + Send + Sync + 'static,
) -> AnyView {
    let on = on.into();
    AnyView::new(move |cx: &mut Cx| {
        let side = cx.theme().space.row;
        cx.build(
            checkbox(on)
                .width(px(side))
                .height(px(side))
                .on_change(change),
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
        let (across, down, border, line, round, fill, gap) = (
            theme.space.card_across,
            theme.space.card_down,
            theme.space.border,
            *theme.colors.border,
            theme.space.round,
            *theme.colors.surface,
            theme.space.gap / 2.0,
        );
        cx.build(
            column(content)
                .gap(gap)
                .padding(UiRect::axes(px(across), px(down)))
                .border(border)
                .border_color(line)
                .radius(round)
                .fill(fill),
        )
    })
}

/// A line across, between two parts.
pub fn divider() -> AnyView {
    AnyView::new(|cx: &mut Cx| {
        let theme = cx.theme();
        let (line, colour) =
            (theme.space.divider, *theme.colors.border);
        cx.build(frame().height(px(line)).fill(colour))
    })
}

/// `rows` as one list, each padded, a divider between each and the
/// next.
pub fn list(rows: Vec<AnyView>) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let pad = cx.theme().space.row_padding;
        let mut parts = Vec::new();
        for one in rows {
            if !parts.is_empty() {
                parts.push(divider());
            }
            parts.push(
                column((one,))
                    .gap(0.0)
                    .padding(UiRect::vertical(px(pad)))
                    .boxed(),
            );
        }
        cx.build(column(parts).gap(0.0))
    })
}

/// `children` in a column that scrolls, taking the height left.
pub fn scroll_area<C>(children: C) -> AnyView
where
    C: bevy_fynix::ViewSeq<bevy_fynix::Bevy, EngineTheme> + 'static,
{
    AnyView::new(move |cx: &mut Cx| {
        let gap = cx.theme().space.gap;
        cx.build(
            scroll(children)
                .gap(gap)
                .grow(1.0)
                .min_height(px(0.0))
                .tagged(GlowMargin),
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
        let mut lines = vec![headed(
            which,
            label(title).bold(true).wrap(false).tagged(DisplayText),
        )];
        if let Some(holds) = holds {
            lines.push(
                label(holds)
                    .size(small)
                    .bold(false)
                    .tone(Tone::Dim)
                    .boxed(),
            );
        }
        cx.build(styled(
            ButtonStyle::Row,
            row((
                column(lines)
                    .gap(gap / 2.0)
                    .grow(1.0)
                    .min_width(px(0.0)),
                glyph(Glyph::ChevronRight, icon).tone(Tone::Dim),
            ))
            .gap(section)
            .grow(1.0)
            .align(AlignItems::Center),
        ))
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
                marked(ButtonStyle::Quiet, Glyph::Back, front)
                    .on_activate(back),
                headed(
                    which,
                    label(name)
                        .bold(true)
                        .wrap(false)
                        .tone(Tone::Faint)
                        .tagged(DisplayText),
                ),
            ))
            .gap(section)
            .align(AlignItems::Center),
        )
    })
}

/// One answer of several to pick from, each lettered `labels`, the
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
/// and the one picked a lit plate in a frame.
fn choice_rules(
    picked: bool,
) -> impl FnOnce(&mut Cx) + Send + Sync + 'static {
    let base = lettered().frame(|f, t| {
        f.border(t.space.border)
            .radius(t.space.round)
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
fn waver(now: f32, seed: f32) -> f32 {
    0.5 * (now * 5.3 + seed).sin()
        + 0.3 * (now * 8.9 + seed * 1.7).sin()
        + 0.2 * (now * 2.1 + seed * 0.6).sin()
}

/// Gives every button and field a state under the hand.
fn arm(
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
fn answer(
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
        let heat =
            ease(state.heat, f32::from(over), set.hover_rate, dt);
        let press =
            ease(state.press, f32::from(held), set.press_rate, dt);
        let pulse = ease(state.pulse, 0.0, set.fade_rate, dt);
        let shake = ease(state.shake, 0.0, set.fade_rate, dt);
        let still = heat == state.heat
            && press == state.press
            && pulse == state.pulse
            && shake == state.shake;
        // Its glow at rest and under the pointer or the caret, and
        // the glow its work adds.
        let (rest, near, done) = match glow {
            Glow::None => (0.0, 0.0, 0.0),
            Glow::Field => (0.0, set.field, 0.0),
            Glow::Plain | Glow::Words => {
                (0.0, set.plain, set.done_gain * pulse)
            }
            Glow::Primary => (set.primary_rest, set.primary, pulse),
            Glow::Critical => {
                (0.0, set.critical, set.done_gain * shake)
            }
        };
        let steady = rest + (near - rest) * heat;
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
                px(set.press_sink * press),
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
        let flicker = 1.0 + set.flicker * waver(now, seed);
        *state = GlowState {
            heat,
            press,
            pulse,
            shake,
            glow: (lit * flicker).clamp(0.0, 1.0),
            reach: set.reach + set.pulse_reach * pulse.max(shake),
        };
    }
}

/// Draws each control's glow as shadows of its colours just below it,
/// or below its lettering where it has no plate, and lights the
/// inside of the primary action and a field being typed in from the
/// bottom.
fn shine(
    theme: Res<EngineTheme>,
    controls: Query<(Entity, &GlowState, &Glow, Option<&Children>)>,
    shadows: Query<&BoxShadow>,
    gradients: Query<&BackgroundGradient>,
    mut commands: Commands,
) {
    let (set, colors) = (&theme.glow, &theme.colors);
    for (control, state, glow, within) in &controls {
        if matches!(glow, Glow::Primary | Glow::Field) {
            let inner = set.inner * state.glow;
            let now = gradients.get(control).ok();
            if inner <= 0.0 {
                if now.is_some() {
                    commands
                        .entity(control)
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
                    commands.entity(control).try_insert(want);
                }
            }
        }
        let target = match glow {
            Glow::None => continue,
            Glow::Words => {
                let Some(first) =
                    within.and_then(|within| within.first())
                else {
                    continue;
                };
                *first
            }
            _ => control,
        };
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
            Glow::Words => state.reach * set.text_reach,
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

/// Gives each new [`Shade`] surface its shading toward its bottom.
fn shade(
    theme: Res<EngineTheme>,
    surfaces: Query<Entity, Added<Shade>>,
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

/// Lets the pointer through everything inside a button, so the button
/// itself is what a press and a release land on. A click counts only
/// when both land on the same node, and without this a press on the
/// label and a release on the icon or the padding would not count.
fn pick_whole_buttons(
    nodes: Query<Entity, (Added<Node>, Without<ButtonBehavior>)>,
    parents: Query<&ChildOf>,
    buttons: Query<(), With<ButtonBehavior>>,
    mut commands: Commands,
) {
    for node in &nodes {
        if parents
            .iter_ancestors(node)
            .any(|above| buttons.contains(above))
        {
            commands.entity(node).try_insert(Pickable::IGNORE);
        }
    }
}

/// Gives each [`GlowMargin`] area room for the farthest glow, again
/// whenever the theme changes.
fn glow_margin(
    theme: Res<EngineTheme>,
    mut nodes: Query<(Ref<GlowMargin>, &mut Node)>,
) {
    let set = &theme.glow;
    let room = (set.reach + set.pulse_reach) * (1.0 + set.drop);
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
