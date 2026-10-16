//! Text fields that hold still: Bevy marks every editable text as
//! changed on every frame it lays one out, which re-measures the
//! field and so lays out every panel above it again, the whole chat
//! among them. A field's measure that comes out as it was is let pass
//! unnoticed.

use std::collections::HashMap;

use bevy::app::{App, PostUpdate};
use bevy::asset::Assets;
use bevy::ecs::change_detection::{
    DetectChanges, DetectChangesMut, Tick,
};
use bevy::ecs::entity::Entity;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Local, Query, Res};
use bevy::text::{EditableText, Font, LineHeight, TextFont};
use bevy::ui::widget::update_editable_text_content_size;
use bevy::ui::{ComputedUiRenderTargetInfo, ContentSize, UiSystems};

pub fn plugin(app: &mut App) {
    app.add_systems(
        PostUpdate,
        hold_still
            .after(update_editable_text_content_size)
            .before(UiSystems::Layout),
    );
}

/// All a field's measure is made from, as Bevy measures it.
#[derive(Clone, PartialEq, Debug)]
struct Sizing {
    width: Option<f32>,
    lines: Option<f32>,
    font: String,
    scale: f32,
    /// The fonts loaded: a field is measured once its font is.
    fonts: usize,
}

/// Puts back the time a field's measure last changed when it was
/// measured again from all the same.
fn hold_still(
    mut fields: Query<(
        Entity,
        &EditableText,
        &TextFont,
        &LineHeight,
        &ComputedUiRenderTargetInfo,
        &mut ContentSize,
    )>,
    fonts: Res<Assets<Font>>,
    mut kept: Local<HashMap<Entity, (Sizing, Tick)>>,
) {
    let mut seen = Vec::with_capacity(kept.len());
    for (field, text, font, line, target, mut size) in &mut fields {
        seen.push(field);
        if !size.is_changed() {
            continue;
        }
        let now = Sizing {
            width: text.visible_width,
            lines: text.visible_lines,
            font: format!("{font:?} {line:?}"),
            scale: target.scale_factor(),
            fonts: fonts.len(),
        };
        match kept.get(&field) {
            Some((was, at)) if *was == now => {
                size.set_last_changed(*at)
            }
            _ => {
                kept.insert(field, (now, size.last_changed()));
            }
        }
    }
    if kept.len() > seen.len() {
        kept.retain(|field, _| seen.contains(field));
    }
}

#[cfg(test)]
mod tests {
    use bevy::app::Update;
    use bevy::ecs::query::Changed;
    use bevy::ecs::resource::Resource;
    use bevy::ecs::system::ResMut;
    use bevy::ui::NodeMeasure;

    use super::*;

    #[derive(Resource, Default)]
    struct Seen(Vec<bool>);

    #[test]
    fn a_field_measured_again_the_same_is_not_changed() {
        let mut app = App::new();
        app.init_resource::<Seen>()
            .init_resource::<Assets<Font>>()
            .add_systems(
                Update,
                // As Bevy does on every frame.
                |mut fields: Query<(
                    &EditableText,
                    &mut ContentSize,
                )>| {
                    for (text, mut size) in &mut fields {
                        size.set(NodeMeasure::Fixed(
                            bevy::ui::measurement::FixedMeasure {
                                size: bevy::math::Vec2::new(
                                    text.visible_width.unwrap_or(1.0),
                                    10.0,
                                ),
                            },
                        ));
                    }
                },
            )
            .add_systems(PostUpdate, hold_still)
            .add_systems(
                PostUpdate,
                (|changed: Query<(), Changed<ContentSize>>,
                  mut seen: ResMut<Seen>| {
                    seen.0.push(!changed.is_empty());
                })
                .after(hold_still),
            );
        let field = app
            .world_mut()
            .spawn((
                EditableText {
                    visible_width: Some(12.0),
                    ..EditableText::default()
                },
                TextFont::default(),
                LineHeight::default(),
                ComputedUiRenderTargetInfo::default(),
                ContentSize::default(),
            ))
            .id();
        for _ in 0..3 {
            app.update();
        }
        app.world_mut()
            .get_mut::<EditableText>(field)
            .unwrap()
            .visible_width = Some(20.0);
        app.update();
        app.update();
        // Its first measure and the one after it was made wider are
        // changes; the measures in between are not.
        assert_eq!(
            app.world().resource::<Seen>().0,
            [true, false, false, true, false]
        );
    }
}
