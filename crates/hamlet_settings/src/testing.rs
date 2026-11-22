//! Helpers the tests of the pages share.

use bevy::app::App;
use hamlet_core::desktop::awake::KeepAwake;
use hamlet_core::settings::SettingsPane;
use hamlet_core::testing::settle;
use hamlet_core::ui::theme::HamletTheme;

/// The Settings panel open on `pane`, over the shared panels.
pub(crate) fn settings_page(pane: Option<&'static str>) -> App {
    let mut app = hamlet_core::testing::panel_app();
    app.add_plugins((
        crate::plugin,
        hamlet_core::ui::widgets::secret::plugin,
    ))
    .init_resource::<KeepAwake>()
    .insert_resource(SettingsPane(pane));
    hamlet_core::fynix::mount::<HamletTheme>(
        app.world_mut(),
        crate::screen(),
    );
    settle(&mut app);
    app
}
