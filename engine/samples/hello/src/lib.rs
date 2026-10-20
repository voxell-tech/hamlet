//! A sample plugin: a panel with a button and a count of its presses,
//! and a command that sets the count back to nought. The count is
//! kept when the plugin is reloaded.

use hamlet_plugin::{Action, Entry, Plugin, Ui, WidgetEvent};

#[derive(Default)]
struct Hello {
    count: u32,
}

impl Plugin for Hello {
    fn declare(&self, entry: &mut Entry) {
        entry
            .panel("hello", "Hello")
            .command("reset", "Reset the count")
            .menu_entry("Hello", "reset")
            .shortcut("Primary+Shift+R", "reset");
    }

    fn draw(&self, _: &str, ui: &mut Ui) {
        let times = match self.count {
            1 => "once".to_string(),
            count => format!("{count} times"),
        };
        ui.heading("title", "Hello")
            .text("count", format!("Pressed {times}."))
            .button("press", "Press");
    }

    fn event(&mut self, event: WidgetEvent) {
        if event.widget.0 == "press" && event.action == Action::Click
        {
            self.count += 1;
        }
    }

    fn run(&mut self, name: &str) {
        if name == "reset" {
            self.count = 0;
        }
    }

    fn save(&self) -> Vec<u8> {
        self.count.to_le_bytes().to_vec()
    }

    fn restore(&mut self, state: &[u8]) {
        if let Ok(bytes) = state.try_into() {
            self.count = u32::from_le_bytes(bytes);
        }
    }
}

hamlet_plugin::export!(Hello);
