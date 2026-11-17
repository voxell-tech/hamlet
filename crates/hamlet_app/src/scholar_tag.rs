//! The tag beside an agent of a scholar of the city: the scholar it
//! is of, marked while the agent holds what it learned that no dream
//! has brought home yet. Pressed, it opens the scholar's page.

use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;
use bevy::ui::{
    AlignItems, JustifyContent, PositionType, UiRect, Val, px,
};
use bevy_fynix::tokens::{MotionTokens, SurfaceTokens, Tone};
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, button, frame, label, row,
};
use bevy_fynix::{ScopedExt, ViewExt, style};
use hamlet_client::realm::{
    Learned, Scholar, city_scholar, shown_of,
};
use hamlet_ui::styles::{note, small_rules};
use hamlet_ui::theme::HamletTheme;
use hamlet_ui::widgets::ellipsis::cut;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::{AnyView, Cx, View, with_cx};

/// The most letters of a scholar's name a tag shows.
const NAME_SHOWN: usize = 16;

/// The side of the tag's icon, and of its mark.
const TAG_ICON: f32 = 10.0;
const MARK: f32 = 5.0;

/// The side of a badge, and of its icon.
const BADGE: f32 = 10.0;
const BADGE_ICON: f32 = 7.0;

/// The scholar of the city an agent is of, as its tag shows it.
#[derive(Clone, PartialEq, Debug)]
pub(crate) struct ScholarTag {
    pub scholar: Entity,
    pub name: String,
    /// Whether the agent goes by its scholar's name, which the tag
    /// then leaves out.
    pub same_name: bool,
    /// Whether the agent holds what it learned that no dream has
    /// brought home to the scholar yet.
    pub learned: bool,
}

impl ScholarTag {
    /// The tag of `agent`, while it is of a scholar of the city.
    pub(crate) fn of(world: &World, agent: Entity) -> Option<Self> {
        let scholar = city_scholar(world, agent)?;
        let name = world.get::<Scholar>(scholar)?.0.name.clone();
        Some(Self {
            scholar,
            same_name: shown_of(world, agent) == name,
            learned: world.get::<Learned>(agent).is_some(),
            name,
        })
    }

    /// What the tag says under the pointer.
    pub(crate) fn tip(&self) -> String {
        let of = format!("Of {}, a scholar of the city", self.name);
        if self.learned {
            format!("{of}\n{}", learned_words(&self.name))
        } else {
            of
        }
    }
}

/// What the mark of a hire that learned what `scholar` lacks says.
pub(crate) fn learned_words(scholar: &str) -> String {
    format!(
        "Learned what {scholar} does not know yet; a dream brings \
         it home"
    )
}

/// The mark of what a hire learned that its scholar lacks: a small
/// dot in the accent.
pub(crate) fn learned_mark() -> impl View {
    with_cx(|cx| {
        frame()
            .width(px(MARK))
            .height(px(MARK))
            .shrink(0.0)
            .radius(MARK / 2.0)
            .fill(cx.theme().accent())
    })
}

/// `tag` as a quiet capsule: the scholar's icon, its name where the
/// agent goes by another, and the mark of what the agent learned.
pub(crate) fn tag(tag: &ScholarTag) -> AnyView {
    let mut parts = vec![glyph(Glyph::Scholar, TAG_ICON).boxed()];
    if !tag.same_name {
        parts.push(
            label(cut(&tag.name, NAME_SHOWN))
                .wrap(false)
                .rules(small_rules)
                .boxed(),
        );
    }
    if tag.learned {
        parts.push(learned_mark().boxed());
    }
    let scholar = tag.scholar;
    let tip = tag.tip();
    button(row(parts).gap(4.0).align(AlignItems::Center))
        .shrink(0.0)
        .rules(tag_look)
        .tooltip(move || note(tip.clone()))
        .on_activate(move |world| {
            crate::hub::open_scholar(world, scholar);
        })
        .boxed()
}

/// `tag` as a little disc with the scholar's icon, laid on the
/// top-left corner of the agent's icon. Its icon is in the accent
/// while the agent holds what it learned.
pub(crate) fn badge(tag: &ScholarTag) -> AnyView {
    let scholar = tag.scholar;
    let tip = tag.tip();
    let learned = tag.learned;
    button(glyph(Glyph::Scholar, BADGE_ICON))
        .position(PositionType::Absolute)
        .inset(UiRect {
            left: px(-BADGE / 2.0),
            top: px(-BADGE / 2.0),
            right: Val::Auto,
            bottom: Val::Auto,
        })
        .width(px(BADGE))
        .height(px(BADGE))
        .justify(JustifyContent::Center)
        .align(AlignItems::Center)
        .rules(move |cx: &mut Cx| badge_look(cx, learned))
        .tooltip(move || note(tip.clone()))
        .on_activate(move |world| {
            crate::hub::open_scholar(world, scholar);
        })
        .boxed()
}

/// A badge is a disc of the panel's background, ringed by the hover's
/// fill, its icon dim, and in the accent under the pointer.
fn badge_look(cx: &mut Cx, learned: bool) {
    style::<HamletTheme>()
        .frame(|f, t| {
            f.fill(t.palette.surface)
                .border(1.0)
                .border_color(t.palette.button_hover)
                .radius(BADGE / 2.0)
                .padding(UiRect::all(px(0.0)))
        })
        .tone(if learned { Tone::Accent } else { Tone::Dim })
        .hovered(|s| s.tone(Tone::Accent))
        .pressed(|s| s.tone(Tone::Faint))
        .transition(HamletTheme::interact)
        .apply(cx);
}

/// A tag is set in a capsule of the hover's fill, its words dim, and
/// in the accent under the pointer.
fn tag_look(cx: &mut Cx) {
    style::<HamletTheme>()
        .frame(|f, t| {
            f.fill(t.palette.button_hover)
                .radius(t.text.small + 4.0)
                .padding(UiRect::axes(px(6.0), px(1.0)))
        })
        .tone(Tone::Dim)
        .hovered(|s| s.tone(Tone::Accent))
        .pressed(|s| s.tone(Tone::Faint))
        .transition(HamletTheme::interact)
        .apply(cx);
}
