//! A sample plugin: a panel with a button and a count of its presses,
//! and a command that sets the count back to nought. The count is
//! kept when the plugin is reloaded.

use hamlet_engine_sdk::ui::{
    AnyView, UiExt, ViewExt, button, column, heading, label, resource,
};
use hamlet_engine_sdk::{Entry, Plugin, World};

#[derive(Default)]
struct Hello;

/// The presses counted.
#[derive(Default)]
struct Count(u32);

impl Plugin for Hello {
    fn declare(&self, entry: &mut Entry) {
        entry
            .panel("hello", "Hello")
            .command("reset", "Reset the count")
            .menu_entry("Hello", "reset")
            .shortcut("Primary+Shift+R", "reset");
    }

    fn mount(&mut self, _: &str, world: &mut World) -> AnyView {
        world.init_resource::<Count>();
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

    fn run(&mut self, name: &str, world: &mut World) {
        if name == "reset" {
            world.insert(Count(0));
        }
    }

    fn save(&self, world: &World) -> Vec<u8> {
        let count = world.get::<Count>().map_or(0, |count| count.0);
        count.to_le_bytes().to_vec()
    }

    fn restore(&mut self, state: &[u8], world: &mut World) {
        if let Ok(bytes) = state.try_into() {
            world.insert(Count(u32::from_le_bytes(bytes)));
        }
    }
}

hamlet_engine_sdk::export!(Hello);
