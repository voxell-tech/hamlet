//! What a plugin declares it adds.

use hamlet_engine_api::{Contribution, Layout};

/// What a plugin adds, as [`Plugin::declare`](crate::Plugin::declare)
/// declares it. A name is the plugin's own. A command is named by its
/// name, for one of the plugin's own, or by its full id for
/// another's.
#[derive(Default)]
pub struct Entry(Vec<Contribution>);

impl Entry {
    /// A panel for the dock, titled `title`, its view built by
    /// [`Plugin::mount`](crate::Plugin::mount) with its name.
    pub fn panel(&mut self, name: &str, title: &str) -> &mut Self {
        self.add(Contribution::Panel {
            name: name.into(),
            title: title.into(),
            icon: None,
        })
    }

    /// A panel as [`Entry::panel`] adds it, its tab showing the icon
    /// `icon`, as `Glyph::name` has it.
    pub fn panel_with_icon(
        &mut self,
        name: &str,
        title: &str,
        icon: &str,
    ) -> &mut Self {
        self.add(Contribution::Panel {
            name: name.into(),
            title: title.into(),
            icon: Some(icon.into()),
        })
    }

    /// A command a menu calls `label`, run by
    /// [`Plugin::run`](crate::Plugin::run) with its name.
    pub fn command(&mut self, name: &str, label: &str) -> &mut Self {
        self.add(Contribution::Command {
            name: name.into(),
            label: label.into(),
        })
    }

    /// The command `command` in the bar's menu titled `menu`.
    pub fn menu_entry(
        &mut self,
        menu: &str,
        command: &str,
    ) -> &mut Self {
        self.add(Contribution::MenuEntry {
            menu: menu.into(),
            command: command.into(),
        })
    }

    /// The keys `keys` bound to the command `command`, written as
    /// `Primary+Shift+H`: Primary is Command on macOS and Control
    /// elsewhere.
    pub fn shortcut(
        &mut self,
        keys: &str,
        command: &str,
    ) -> &mut Self {
        self.add(Contribution::Shortcut {
            keys: keys.into(),
            command: command.into(),
        })
    }

    /// A page of settings titled `title`, built as a panel is. Its
    /// link row in Settings shows the icon `icon`, as `Glyph::name`
    /// has it, and what it holds, `about`.
    pub fn settings_page(
        &mut self,
        name: &str,
        title: &str,
        about: &str,
        icon: &str,
    ) -> &mut Self {
        self.add(Contribution::SettingsPage {
            name: name.into(),
            title: title.into(),
            about: about.into(),
            icon: icon.into(),
        })
    }

    /// What Settings shows under the link rows to its pages, built as
    /// a panel is.
    pub fn settings_footer(&mut self, name: &str) -> &mut Self {
        self.add(Contribution::SettingsFooter { name: name.into() })
    }

    /// A workspace the person can take, titled `title`.
    pub fn workspace(
        &mut self,
        name: &str,
        title: &str,
        layout: Layout,
    ) -> &mut Self {
        self.add(Contribution::Workspace {
            name: name.into(),
            title: title.into(),
            layout,
        })
    }

    /// An item of the status line, after those that came before it,
    /// hidden until [`ui::set_status`](crate::ui::set_status) gives
    /// it words.
    pub fn status_item(&mut self, name: &str) -> &mut Self {
        self.add(Contribution::StatusItem {
            name: name.into(),
            grow: false,
        })
    }

    /// An item of the status line that takes the room left on it,
    /// words or none, and clips its words.
    pub fn growing_status_item(&mut self, name: &str) -> &mut Self {
        self.add(Contribution::StatusItem {
            name: name.into(),
            grow: true,
        })
    }

    /// A card over the dock, on a backdrop that takes every press,
    /// shown by [`ui::show_overlay`](crate::ui::show_overlay). Its
    /// view is built by [`Plugin::mount`](crate::Plugin::mount) with
    /// its name.
    pub fn overlay(&mut self, name: &str) -> &mut Self {
        self.add(Contribution::Overlay { name: name.into() })
    }

    pub(crate) fn finish(self) -> Vec<Contribution> {
        self.0
    }

    fn add(&mut self, contribution: Contribution) -> &mut Self {
        self.0.push(contribution);
        self
    }
}
