//! The items of the menus of the world that the window adds, and the
//! commands they run.

use bevy::ecs::world::World;
use hamlet_sdk::command::{Subject, kind};
use hamlet_sdk::plugin::Entry;

use super::acts::{self, queue};

/// An agent, and its house.
const RESIDENT: &[&str] = &[kind::AGENT, kind::HOUSE];
/// The chief's mascot, and its Hearth.
const CHIEF: &[&str] = &[kind::CHIEF, kind::HEARTH];

/// One item of a menu, for each of the kinds it is in. The window's
/// items use the sections 10 to 120, so that an item a plugin adds
/// with a higher section comes after them.
struct Item {
    kinds: &'static [&'static str],
    section: u32,
    label: &'static str,
    command: &'static str,
    when: Option<fn(&World, &Subject) -> bool>,
}

const fn item(
    kinds: &'static [&'static str],
    section: u32,
    label: &'static str,
    command: &'static str,
) -> Item {
    Item {
        kinds,
        section,
        label,
        command,
        when: None,
    }
}

const fn item_if(
    kinds: &'static [&'static str],
    section: u32,
    label: &'static str,
    command: &'static str,
    when: fn(&World, &Subject) -> bool,
) -> Item {
    Item {
        kinds,
        section,
        label,
        command,
        when: Some(when),
    }
}

/// The items, a menu's in the order it lists them.
const ITEMS: [Item; 24] = [
    item(RESIDENT, 10, "Talk to {name}", "talk"),
    item(RESIDENT, 20, "Prompt", "prompt"),
    item(RESIDENT, 30, "Focus", "focus"),
    item(RESIDENT, 40, "Follow", "follow"),
    item_if(RESIDENT, 50, "Stop {name}", "stop", acts::busy),
    item_if(
        RESIDENT,
        70,
        "Give a skill...",
        "give-skill",
        acts::can_learn,
    ),
    item_if(
        RESIDENT,
        100,
        "Undo the Last Change",
        "undo",
        acts::plain,
    ),
    item_if(
        RESIDENT,
        110,
        "Make {name} Mayor...",
        "make-mayor",
        acts::plain,
    ),
    item_if(RESIDENT, 120, "Remove {name}...", "remove", acts::plain),
    item(&[kind::MAYOR], 10, "Talk to the Mayor", "talk"),
    item(&[kind::MAYOR], 20, "Prompt", "prompt"),
    item(&[kind::MAYOR], 50, "Stop the Mayor", "stop"),
    item(&[kind::MAYOR], 100, "Undo the Last Change", "undo"),
    item(&[kind::RUIN], 10, "Restore {name}", "restore"),
    item(&[kind::HAMLET], 10, "Focus", "focus"),
    item(&[kind::HAMLET], 20, "Move {name}...", "move"),
    item_if(
        &[kind::HAMLET],
        30,
        "Close Down {name}...",
        "close-down",
        acts::closable,
    ),
    item(CHIEF, 10, "Change Your Look", "change-look"),
    item(CHIEF, 20, "Focus", "focus"),
    item(CHIEF, 30, "Go to the Hearth", "go-to-hearth"),
    item(CHIEF, 40, "Move the Hearth...", "move-hearth"),
    item(&[kind::LIBRARY], 10, "Focus", "focus"),
    item(&[kind::LIBRARY], 20, "Move the Library...", "move"),
    item(&[kind::GROUND], 10, "Found a Hamlet...", "found-here"),
];

/// Declares the commands the window's items run, and the items.
pub(crate) fn contribute(entry: &mut Entry) {
    entry.command_with("talk", "Talk to", |c, s| {
        queue(c, s, acts::talk)
    });
    entry.command_with("prompt", "Show the Prompt", |c, s| {
        queue(c, s, acts::prompt)
    });
    entry.command_with("focus", "Focus", |c, s| {
        queue(c, s, acts::focus)
    });
    entry.command_with("follow", "Follow", |c, s| {
        queue(c, s, acts::follow)
    });
    entry
        .command_with("stop", "Stop", |c, s| queue(c, s, acts::halt));
    entry.command_with("undo", "Undo the Last Change", |c, s| {
        queue(c, s, acts::undo)
    });
    entry.command_with("make-mayor", "Make Mayor", |c, s| {
        queue(c, s, acts::make_mayor)
    });
    entry.command_with("remove", "Remove", |c, s| {
        queue(c, s, acts::remove)
    });
    entry.command_with("give-skill", "Give a Skill", |c, s| {
        queue(c, s, acts::give_skill)
    });
    entry.command_with("restore", "Restore", |c, s| {
        queue(c, s, acts::bring_back)
    });
    entry.command_with("change-look", "Change Your Look", |c, s| {
        queue(c, s, acts::change_look)
    });
    entry.command_with("go-to-hearth", "Go to the Hearth", |c, s| {
        queue(c, s, acts::go_to_hearth)
    });
    entry.command_with("move-hearth", "Move the Hearth", |c, s| {
        queue(c, s, acts::move_hearth)
    });
    entry.command_with("move", "Move", |c, s| {
        queue(c, s, acts::carry)
    });
    entry.command_with("close-down", "Close Down", |c, s| {
        queue(c, s, acts::close_down)
    });
    entry.command_with(
        "found-here",
        "Found a Hamlet Here",
        |c, s| queue(c, s, acts::found),
    );
    for item in &ITEMS {
        for target in item.kinds {
            match item.when {
                Some(when) => entry.context_item_if(
                    target,
                    item.section,
                    item.label,
                    item.command,
                    when,
                ),
                None => entry.context_item(
                    target,
                    item.section,
                    item.label,
                    item.command,
                    None,
                ),
            }
        }
    }
}
