//! A stall's counter, as the Inspector shows it while the stall is
//! selected in the world: its sign under a lamp, its keeper and what
//! they say to the chief, and their wares.

use std::collections::HashMap;

use bevy::app::{App, Update};
use bevy::color::Alpha;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::schedule::common_conditions::resource_changed;
use bevy::ecs::system::{Local, Res, ResMut};
use bevy::text::{Justify, TextLayout};
use bevy::ui::{
    AlignItems, BackgroundGradient, BorderRadius, ColorStop,
    JustifyContent, RadialGradient, RadialGradientShape, UiPosition,
    UiRect, percent, px,
};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, Label, column, frame, label, row, scroll,
};
use bevy_fynix::{ViewExt, keyed, resource};
use hamlet_world::bazaar::Stall;
use hamlet_world::pick::{Selected, Thing, select};

use crate::feel::FootLight;
use crate::icons::glyph;
use crate::looks::{Fills, action, back, buttons, hairline};
use crate::platform::DisplayText;
use crate::{AnyView, Cx, View, with_cx};

/// The side of the keeper's portrait.
const PORTRAIT: f32 = 52.0;

/// The corner of the keeper's speech, and the one by the portrait.
const ROUND: f32 = 10.0;
const TAIL: f32 = 3.0;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Visits>().add_systems(
        Update,
        visit.run_if(resource_changed::<Selected>),
    );
}

/// The times the chief has stepped up to each stall, so that its
/// keeper says something new each visit.
#[derive(Resource, Default)]
struct Visits(HashMap<Stall, usize>);

impl Visits {
    /// Which of its keeper's greetings `stall` gives now.
    fn greeting(&self, stall: Stall) -> usize {
        let visits = self.0.get(&stall).copied().unwrap_or(1);
        (visits.max(1) - 1) % stall.keeper().greetings.len()
    }
}

/// Counts a visit each time the chief comes to a stall from
/// elsewhere.
fn visit(
    selected: Res<Selected>,
    mut visits: ResMut<Visits>,
    mut at: Local<Option<Stall>>,
) {
    let now = match selected.0 {
        Some(Thing::Stall(stall)) => Some(stall),
        _ => None,
    };
    if now == *at {
        return;
    }
    if let Some(stall) = now {
        *visits.0.entry(stall).or_default() += 1;
    }
    *at = now;
}

/// The counter of `stall`.
pub(super) fn counter(stall: Stall) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        // Drawn without the bazaar's plugin in the gallery.
        cx.world.init_resource::<Visits>();
        let seam = cx.theme().palette.brass_6;
        let view = column((
            sign(cx, stall),
            keeper(cx, stall),
            speech(stall),
            buttons(vec![
                back("THE BAZAAR", |world| {
                    select(world, Some(Thing::Bazaar))
                })
                .boxed(),
                // Plain, for the wares below to have the one lamp.
                action(
                    super::glyph(stall),
                    "OPEN THE STALL",
                    move |world| super::open_stall(world, stall),
                ),
            ]),
            hairline(seam),
            super::wares(stall),
        ))
        .gap(12.0);
        // The keeper scrolls away with the wares, which in a narrow
        // panel would have little room under them.
        cx.build(
            column((scroll((view,)).fills_height(),))
                .fills_height()
                .gap(0.0),
        )
    })
}

/// The board over the stall: its name lit from below as by a candle,
/// hung on two chains under a lamp, and what it sells.
fn sign(cx: &Cx, stall: Stall) -> impl View + use<> {
    let palette = &cx.theme().palette;
    let link = || {
        frame()
            .width(px(5.0))
            .height(px(7.0))
            .border(1.0)
            .border_color(palette.brass_5)
            .radius(2.5)
    };
    let chain = || column((link(), link(), link())).gap(0.0).boxed();
    let lamp = BackgroundGradient::from(RadialGradient::new(
        UiPosition::TOP,
        RadialGradientShape::Ellipse(percent(75.0), percent(130.0)),
        vec![
            ColorStop::new(
                palette.lamp_high.with_alpha(0.5),
                percent(0.0),
            ),
            ColorStop::new(
                palette.lamp_low.with_alpha(0.8),
                percent(35.0),
            ),
            ColorStop::new(
                palette.lamp_low.with_alpha(0.0),
                percent(100.0),
            ),
        ],
    ));
    let name = row((label(stall.name().to_uppercase())
        .size(15.0)
        .bold(true)
        .wrap(false)
        .tone(Tone::Body)
        .tagged(DisplayText),))
    .padding(UiRect::axes(px(12.0), px(2.0)))
    .tagged(FootLight::new(true));
    column((
        row((chain(), chain()))
            .justify(JustifyContent::SpaceBetween)
            .padding(UiRect::axes(px(28.0), px(0.0))),
        column((
            row((
                glyph(super::glyph(stall), 14.0).tone(Tone::Faint),
                name,
                glyph(super::glyph(stall), 14.0).tone(Tone::Faint),
            ))
            .gap(4.0)
            .align(AlignItems::Center)
            .justify(JustifyContent::Center),
            centred(label(stall.sells()).size(12.0).tone(Tone::Dim)),
        ))
        .gap(6.0)
        .align(AlignItems::Center)
        .padding(UiRect::new(px(14.0), px(14.0), px(10.0), px(12.0)))
        .fill(palette.card)
        .border(1.0)
        .border_color(palette.brass_5)
        .radius(cx.theme().space.soft)
        .with(lamp),
    ))
    .gap(0.0)
}

/// The keeper behind the counter: their portrait, the stall's mark in
/// a brass ring, beside their name and what they are.
fn keeper(cx: &Cx, stall: Stall) -> impl View + use<> {
    let palette = &cx.theme().palette;
    let keeper = stall.keeper();
    let portrait =
        row((glyph(super::glyph(stall), 24.0).tone(Tone::Faint),))
            .width(px(PORTRAIT))
            .height(px(PORTRAIT))
            .shrink(0.0)
            .justify(JustifyContent::Center)
            .align(AlignItems::Center)
            .fill(palette.lamp_low)
            .border(1.5)
            .border_color(palette.brass_3)
            .radius(PORTRAIT / 2.0);
    row((
        portrait,
        column((
            label(keeper.name)
                .size(19.0)
                .bold(true)
                .tone(Tone::Body)
                .tagged(DisplayText),
            label(keeper.title).size(12.0).tone(Tone::Dim),
        ))
        .gap(2.0)
        .fills_width(),
    ))
    .gap(12.0)
    .align(AlignItems::Center)
}

/// What the keeper says to the chief, a new greeting each visit, in a
/// bubble that comes from their portrait.
fn speech(stall: Stall) -> AnyView {
    let said =
        resource::<Visits, _>(move |visits| visits.greeting(stall));
    keyed(said, move |at| {
        let words = stall.keeper().greetings[*at];
        let bubble = with_cx(move |cx| {
            let palette = &cx.theme().palette;
            column((label(format!("\"{words}\"")).size(13.0),))
                .padding(UiRect::axes(px(12.0), px(8.0)))
                .min_width(px(0.0))
                .fill(palette.card)
                .border(1.0)
                .border_color(palette.seam)
                .corners(BorderRadius {
                    top_left: px(TAIL),
                    ..BorderRadius::all(px(ROUND))
                })
                .boxed()
        });
        row((bubble,))
            .padding(UiRect::left(px(PORTRAIT / 2.0)))
            .boxed()
    })
    .within(column(()).gap(0.0))
    .boxed()
}

/// `words` with each of their lines centred.
fn centred(words: Label) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let node = cx.build(words);
        if let Some(mut layout) = cx.world.get_mut::<TextLayout>(node)
        {
            layout.justify = Justify::Center;
        }
        node
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The greeting of the Tinker's Stall once the chief has gone to
    /// `thing`.
    fn go(app: &mut App, thing: Option<Thing>) -> usize {
        app.world_mut().resource_mut::<Selected>().0 = thing;
        app.update();
        app.world().resource::<Visits>().greeting(Stall::Plugins)
    }

    #[test]
    fn a_keeper_greets_the_chief_anew_on_each_visit() {
        let mut app = App::new();
        app.init_resource::<Selected>().add_plugins(plugin);
        let tinker = Some(Thing::Stall(Stall::Plugins));
        assert_eq!(go(&mut app, tinker), 0);
        // Chosen again while there, it is the same visit.
        assert_eq!(go(&mut app, tinker), 0);
        go(&mut app, Some(Thing::Stall(Stall::Skills)));
        assert_eq!(go(&mut app, tinker), 1);
        go(&mut app, None);
        assert_eq!(go(&mut app, tinker), 2);
        go(&mut app, Some(Thing::Bazaar));
        assert_eq!(go(&mut app, tinker), 0);
    }
}
