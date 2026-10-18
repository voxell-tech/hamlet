//! What a plugin declares during `entry`.

use bevy_fynix::shortcut::Chord;
use hamlet_sdk::capability::{Capability, Verdict};
use hamlet_sdk::command::kind;
use hamlet_sdk::icons::Glyph;
use hamlet_sdk::inspector;
use hamlet_sdk::keys::chord;
use hamlet_sdk::menu::Menu;
use hamlet_sdk::tree::glyph;

use super::State;
use crate::interface::hamlet::plugin::host::Level;

/// A panel a plugin declared.
#[derive(Clone, PartialEq, Debug)]
pub struct Declared {
    /// The id the plugin knows the panel by.
    pub id: String,
    pub title: String,
    pub icon: Glyph,
}

/// A command a plugin declared.
#[derive(Clone, PartialEq, Debug)]
pub struct Offered {
    pub id: String,
    pub title: String,
    pub icon: Option<Glyph>,
}

/// A line of a menu a plugin declared.
#[derive(Clone, PartialEq, Debug)]
pub struct Placed {
    pub menu: Menu,
    pub section: u32,
    pub label: String,
    /// The plugin's own name for the command it runs.
    pub command: String,
}

/// An item of the menu of a kind of thing that a plugin declared.
#[derive(Clone, PartialEq, Debug)]
pub struct Targeted {
    /// The kind of thing whose menu it is in.
    pub target: String,
    pub section: u32,
    pub label: String,
    /// The plugin's own name for the command it runs.
    pub command: String,
}

/// A tab of the inspector's page of a kind of thing that a plugin
/// declared.
#[derive(Clone, PartialEq, Debug)]
pub struct Tabbed {
    pub target: String,
    pub title: String,
    /// The id the plugin knows the panel by.
    pub panel: String,
}

/// A door of the Settings panel a plugin declared.
#[derive(Clone, PartialEq, Debug)]
pub struct Paged {
    pub title: String,
    pub icon: Glyph,
    /// The id the plugin knows the panel by.
    pub panel: String,
}

/// A shortcut a plugin declared.
#[derive(Clone, PartialEq, Debug)]
pub struct Chorded {
    pub chord: Chord,
    /// The plugin's own name for the command it runs.
    pub command: String,
}

/// What a plugin adds besides panels.
#[derive(Clone, PartialEq, Default, Debug)]
pub struct Offers {
    pub commands: Vec<Offered>,
    pub menus: Vec<Placed>,
    pub contexts: Vec<Targeted>,
    pub tabs: Vec<Tabbed>,
    pub pages: Vec<Paged>,
    pub keys: Vec<Chorded>,
}

impl State {
    pub(super) fn offer(
        &mut self,
        id: String,
        title: String,
        icon: Option<String>,
    ) {
        let commands = &self.offered.commands;
        if commands.len() >= self.command_room
            || commands.iter().any(|offered| offered.id == id)
        {
            return;
        }
        let icon = icon.and_then(|name| {
            let named = glyph::named(&name);
            if named.is_none() {
                self.write(
                    Level::Warn,
                    format!("no icon named `{name}`"),
                );
            }
            named
        });
        let title = self.cut(title);
        self.offered.commands.push(Offered { id, title, icon });
    }

    pub(super) fn place(
        &mut self,
        menu: String,
        section: u32,
        label: String,
        command: String,
    ) {
        let Some(menu) = Menu::parse(&menu) else {
            self.write(
                Level::Warn,
                format!("no menu named `{menu}`"),
            );
            return;
        };
        if self.offered.menus.len() >= self.menu_room {
            return;
        }
        self.want_to_contribute(Capability::UiMenu);
        let label = self.cut(label);
        self.offered.menus.push(Placed {
            menu,
            section,
            label,
            command,
        });
    }

    pub(super) fn contextualise(
        &mut self,
        target: String,
        section: u32,
        label: String,
        command: String,
    ) {
        if !kind::ALL.contains(&target.as_str()) {
            self.write(
                Level::Warn,
                format!("no kind of thing named `{target}`"),
            );
            return;
        }
        if self.offered.contexts.len() >= self.context_room {
            return;
        }
        self.want_to_contribute(Capability::UiContext);
        let label = self.cut(label);
        self.offered.contexts.push(Targeted {
            target,
            section,
            label,
            command,
        });
    }

    pub(super) fn tab(
        &mut self,
        target: String,
        title: String,
        panel: String,
    ) {
        if !inspector::KINDS.contains(&target.as_str()) {
            self.write(
                Level::Warn,
                format!("the page of `{target}` has no tabs"),
            );
            return;
        }
        if self.offered.tabs.len() >= self.section_room {
            return;
        }
        self.want_to_contribute(Capability::UiInspector);
        let title = self.cut(title);
        self.offered.tabs.push(Tabbed {
            target,
            title,
            panel,
        });
    }

    pub(super) fn page(
        &mut self,
        title: String,
        icon: String,
        panel: String,
    ) {
        if self.offered.pages.len() >= self.page_room {
            return;
        }
        let named = glyph::named(&icon);
        if named.is_none() {
            self.write(
                Level::Warn,
                format!("no icon named `{icon}`"),
            );
        }
        self.want_to_contribute(Capability::UiSettings);
        let title = self.cut(title);
        self.offered.pages.push(Paged {
            title,
            icon: named.unwrap_or(glyph::UNKNOWN),
            panel,
        });
    }

    pub(super) fn bind(&mut self, chord: String, command: String) {
        let chord = match chord::parse(&chord) {
            Ok(chord) => chord,
            Err(why) => {
                self.write(Level::Warn, format!("bad chord: {why}"));
                return;
            }
        };
        if self.offered.keys.len() >= self.key_room {
            return;
        }
        self.want_to_contribute(Capability::UiKeys);
        self.offered.keys.push(Chorded { chord, command });
    }

    /// Notes that what the plugin declares shows only once it holds
    /// `capability`, so that the chief is asked where that is yet to
    /// be decided.
    fn want_to_contribute(&mut self, capability: Capability) {
        if self.granted.verdict(capability) == Some(Verdict::Ask) {
            self.want(capability, "");
        }
    }

    fn cut(&self, mut text: String) -> String {
        text.truncate(text.floor_char_boundary(self.bytes));
        text
    }

    pub(super) fn declare(
        &mut self,
        id: String,
        title: String,
        icon: String,
    ) {
        if self.declared.len() >= self.panel_room
            || self.declared.iter().any(|d| d.id == id)
        {
            return;
        }
        let named = glyph::named(&icon);
        if named.is_none() {
            self.write(
                Level::Warn,
                format!("no icon named `{icon}`"),
            );
        }
        self.declared.push(Declared {
            id,
            title,
            icon: named.unwrap_or(glyph::UNKNOWN),
        });
    }
}
