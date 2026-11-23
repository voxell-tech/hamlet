//! The tag of an agent's city profile, which opens its page.

use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;
use bevy::ui::{
    AlignItems, JustifyContent, PositionType, UiRect, Val, px,
};
use hamlet_core::client::realm::{
    Learned, Profile, city_profile, shown_of,
};
use hamlet_core::fynix::tokens::{MotionTokens, SurfaceTokens, Tone};
use hamlet_core::fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, button, frame, label, row,
};
use hamlet_core::fynix::{ScopedExt, ViewExt, style};
use hamlet_core::ui::styles::{note, small_rules};
use hamlet_core::ui::theme::HamletTheme;
use hamlet_core::ui::widgets::ellipsis::cut;
use hamlet_core::ui::widgets::icons::{Glyph, glyph};
use hamlet_core::ui::{AnyView, Cx, View, with_cx};

/// The most letters of a profile's name a tag shows.
const NAME_SHOWN: usize = 16;

/// The side of the tag's icon, and of its mark.
const TAG_ICON: f32 = 10.0;
const MARK: f32 = 5.0;

/// The side of a badge, and of its icon.
const BADGE: f32 = 10.0;
const BADGE_ICON: f32 = 7.0;

/// The profile of the city an agent is of, as its tag shows it.
#[derive(Clone, PartialEq, Debug)]
pub(crate) struct ProfileTag {
    pub profile: Entity,
    pub name: String,
    /// Whether the agent goes by its profile's name.
    pub same_name: bool,
    /// Whether the agent learned what no dream has brought home yet.
    pub learned: bool,
}

impl ProfileTag {
    /// The tag of `agent`, while it is of a profile of the city.
    pub(crate) fn of(world: &World, agent: Entity) -> Option<Self> {
        let profile = city_profile(world, agent)?;
        let name = world.get::<Profile>(profile)?.0.name.clone();
        Some(Self {
            profile,
            same_name: shown_of(world, agent) == name,
            learned: world.get::<Learned>(agent).is_some(),
            name,
        })
    }

    /// What the tag says under the pointer.
    pub(crate) fn tip(&self) -> String {
        let of = format!("Of {}, a profile of the city", self.name);
        if self.learned {
            format!("{of}\n{}", learned_words(&self.name))
        } else {
            of
        }
    }
}

/// What the mark of an agent that learned what `profile` lacks says.
pub(crate) fn learned_words(profile: &str) -> String {
    format!(
        "Learned what {profile} does not know yet; a dream brings \
         it home"
    )
}

/// The mark of what an agent learned that its profile lacks.
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

/// `tag` as a capsule.
pub(crate) fn tag(tag: &ProfileTag) -> AnyView {
    let mut parts = vec![glyph(Glyph::Profile, TAG_ICON).boxed()];
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
    let profile = tag.profile;
    let tip = tag.tip();
    button(row(parts).gap(4.0).align(AlignItems::Center))
        .shrink(0.0)
        .rules(tag_style)
        .tooltip(move || note(tip.clone()))
        .on_activate(move |world| {
            crate::hub::open_profile(world, profile);
        })
        .boxed()
}

/// `tag` as a badge on the top-left corner of the agent's icon.
pub(crate) fn badge(tag: &ProfileTag) -> AnyView {
    let profile = tag.profile;
    let tip = tag.tip();
    let learned = tag.learned;
    button(glyph(Glyph::Profile, BADGE_ICON))
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
        .rules(move |cx: &mut Cx| badge_style(cx, learned))
        .tooltip(move || note(tip.clone()))
        .on_activate(move |world| {
            crate::hub::open_profile(world, profile);
        })
        .boxed()
}

fn badge_style(cx: &mut Cx, learned: bool) {
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

fn tag_style(cx: &mut Cx) {
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
