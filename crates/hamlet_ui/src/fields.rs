//! Text inputs that are laid out again only when they change.

use bevy::app::{App, PostUpdate, Startup};
use bevy::ecs::query::{Added, Changed, Or};
use bevy::ecs::schedule::common_conditions::resource_changed;
use bevy::ecs::schedule::{
    IntoScheduleConfigs, ScheduleCleanupPolicy, SystemCondition,
};
use bevy::ecs::system::Query;
use bevy::ecs::world::World;
use bevy::text::{
    EditableText, EditableTextSystems, LineHeight, RemSize, TextFont,
};
use bevy::ui::widget::{
    update_editable_text_content_size, update_editable_text_styles,
};
use bevy::ui::{ComputedUiRenderTargetInfo, UiSystems};

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(Startup, steady);
}

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
                        sized.or_eager(resource_changed::<RemSize>),
                    )
                    .in_set(UiSystems::Content)
                    .before(update_editable_text_styles)
                    .before(EditableTextSystems),
            );
        }
    });
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
