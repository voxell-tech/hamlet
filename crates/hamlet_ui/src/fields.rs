//! Text inputs that are laid out again only when they change.

use std::collections::HashMap;

use bevy::app::{App, PostUpdate, Startup};
use bevy::ecs::entity::Entity;
use bevy::ecs::query::{Added, Changed, Or, With};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::common_conditions::resource_changed;
use bevy::ecs::schedule::{
    IntoScheduleConfigs, ScheduleCleanupPolicy, SystemCondition,
};
use bevy::ecs::system::{Local, Query, Res};
use bevy::ecs::world::World;
use bevy::input_focus::InputFocus;
use bevy::math::Rect;
use bevy::text::{
    EditableText, EditableTextGeneration, EditableTextSystems,
    LineHeight, RemSize, TextFont, TextLayoutInfo,
};
use bevy::ui::widget::{
    scroll_editable_text, update_editable_text_content_size,
    update_editable_text_layout, update_editable_text_styles,
};
use bevy::ui::{ComputedUiRenderTargetInfo, UiSystems};

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Resized>()
        .add_systems(Startup, (steady, follow));
}

/// Marked changed when a system sets the size a field asks for.
/// `EditableText` cannot say so, because Bevy reads it mutably each
/// frame and it is always changed.
#[derive(Resource, Default)]
pub struct Resized;

/// Bevy reads every `EditableText` mutably each frame, so its size
/// system sees all of them changed and sets their sizes again, and
/// the layout of everything above them is done again. Here that
/// system runs only when something it reads has changed.
fn steady(world: &mut World) {
    world.schedule_scope(PostUpdate, |world, schedule| {
        let removed = schedule.remove_systems_in_set(
            update_editable_text_content_size,
            world,
            ScheduleCleanupPolicy::RemoveSystemsOnly,
        );
        // Without Bevy's own panels the system is not there.
        if removed.is_ok_and(|removed| removed > 0) {
            schedule.add_systems(
                update_editable_text_content_size
                    .run_if(
                        sized
                            .or_eager(resource_changed::<RemSize>)
                            .or_eager(resource_changed::<Resized>),
                    )
                    .in_set(UiSystems::Content)
                    .before(update_editable_text_styles)
                    .before(EditableTextSystems),
            );
        }
    });
}

/// Bevy scrolls every input to its caret each frame, because it reads
/// them mutably and they are always changed, so the wheel could not
/// take one anywhere else. Here that system runs only when a caret
/// has moved, text has changed or focus has changed.
fn follow(world: &mut World) {
    world.schedule_scope(PostUpdate, |world, schedule| {
        let removed = schedule.remove_systems_in_set(
            scroll_editable_text,
            world,
            ScheduleCleanupPolicy::RemoveSystemsOnly,
        );
        if removed.is_ok_and(|removed| removed > 0) {
            schedule.add_systems(
                scroll_editable_text
                    .run_if(caret_moved)
                    .in_set(UiSystems::PostLayout)
                    .after(update_editable_text_layout),
            );
        }
    });
}

/// True when the caret of an input, its text or the focus is not as
/// it was the last time this was asked. The caret blinking is not.
fn caret_moved(
    inputs: Query<
        (Entity, &TextLayoutInfo, &EditableTextGeneration),
        With<EditableText>,
    >,
    focus: Option<Res<InputFocus>>,
    mut seen: Local<
        HashMap<Entity, (Option<Rect>, EditableTextGeneration)>,
    >,
    mut focused: Local<Option<Entity>>,
) -> bool {
    let now = focus.and_then(|focus| focus.get());
    let mut moved = *focused != now;
    *focused = now;
    seen.retain(|input, _| inputs.contains(*input));
    for (input, info, generation) in &inputs {
        let state = (info.cursor.map(|(_, rect)| rect), *generation);
        moved |= seen.insert(input, state) != Some(state);
    }
    moved
}

fn sized(
    fields: Query<
        (),
        Or<(
            Added<EditableText>,
            Changed<TextFont>,
            Changed<LineHeight>,
            Changed<ComputedUiRenderTargetInfo>,
        )>,
    >,
) -> bool {
    !fields.is_empty()
}
