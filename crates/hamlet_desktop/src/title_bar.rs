//! Whether the system moves the window by a drag in its title bar. A
//! Mac moves it from anywhere in the bar, a control there included,
//! so the app turns that off while the pointer is on one.

use bevy_app::App;
use bevy_ecs::resource::Resource;

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<TitleBarDrag>();
    #[cfg(target_os = "macos")]
    {
        use bevy_app::Update;
        use bevy_ecs::schedule::IntoScheduleConfigs;
        use bevy_ecs::schedule::common_conditions::resource_changed;

        app.add_systems(
            Update,
            apply.run_if(resource_changed::<TitleBarDrag>),
        );
    }
}

/// Whether a drag in the title bar moves the window.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug)]
pub struct TitleBarDrag(pub bool);

impl Default for TitleBarDrag {
    fn default() -> Self {
        Self(true)
    }
}

#[cfg(target_os = "macos")]
fn apply(
    drag: bevy_ecs::system::Res<TitleBarDrag>,
    _main: bevy_ecs::system::NonSendMarker,
) {
    use objc2_app_kit::NSApplication;
    use objc2_foundation::MainThreadMarker;

    let Some(thread) = MainThreadMarker::new() else {
        return;
    };
    for window in
        NSApplication::sharedApplication(thread).windows().iter()
    {
        window.setMovable(drag.0);
    }
}
