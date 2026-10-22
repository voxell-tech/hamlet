//! A sample plugin: a panel with a button and a count of its presses,
//! and a command that sets the count back to nought. The count is
//! kept when the plugin is reloaded.

use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::serde::{Deserialize, Serialize};
use hamlet_engine_bevy::ui::{
    UiExt, button, column, heading, label, resource,
};

#[derive(Default)]
struct Hello;

impl Plugin for Hello {
    fn build(&self, app: &mut App) {
        app.init_saved_resource::<Count>()
            .add_panel("hello", "Hello", hello)
            .add_command("reset", "Reset the count", reset)
            .add_menu_entry("Hello", "reset")
            .add_shortcut("Primary+Shift+R", "reset");
    }
}

hamlet_engine_bevy::export!(Hello);

/// The presses counted.
#[derive(Resource, Default, Serialize, Deserialize)]
#[serde(crate = "hamlet_engine_bevy::serde")]
struct Count(u32);

fn hello() -> AnyView {
    column((
        heading("Hello"),
        label(resource::<Count, _>(|count| {
            let times = match count.0 {
                1 => "once".to_string(),
                count => format!("{count} times"),
            };
            format!("Pressed {times}.")
        })),
        button("Press").on_click(|world| {
            world.resource_mut::<Count>().0 += 1;
        }),
    ))
    .boxed()
}

fn reset(mut count: ResMut<Count>) {
    count.0 = 0;
}
