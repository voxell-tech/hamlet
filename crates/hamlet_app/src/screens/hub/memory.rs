//! The MEMORY of an agent or a scholar, shown as the folder it is:
//! the files at its root go into every prompt, and a folder is
//! described by its own `MEMORY.md`. Its skills and character are
//! listed here too.

use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use bevy::ui::{AlignItems, UiRect, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, column, label, row,
    text_field,
};
use bevy_fynix::{ScopedExt, ViewExt, keyed, resource};
use hamlet_protocol::client as protocol;
use hamlet_ui::styles::{
    ICON, action, bad_action, bare, lettered, note, text_input,
};
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::widgets::platform::Wraps;
use hamlet_ui::{AnyView, View, with_cx};

use super::papers::{self, Papers};
use super::thread::dim;
use crate::overview::heading;

/// The memory file whose text is open, and the name typed for a new
/// one.
#[derive(Resource, Default, PartialEq)]
pub struct Opened {
    pub path: Option<String>,
    pub name: String,
}

/// A file of the memory, as the folder view lists it.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Entry {
    pub path: String,
    pub description: String,
    pub text: String,
    pub learned: bool,
}

/// A folder of the memory and what is in it. The root has no name.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Folder {
    pub name: String,
    /// What its `MEMORY.md` says it is for.
    pub description: String,
    pub files: Vec<Entry>,
}

/// The memory laid out as folders, the root first.
pub(super) fn folders(
    memory: &[protocol::MemoryFile],
    learned: &[String],
) -> Vec<Folder> {
    let mut folders: Vec<Folder> = vec![Folder {
        name: String::new(),
        description: String::new(),
        files: Vec::new(),
    }];
    let mut sorted = memory.to_vec();
    sorted.sort_by(|a, b| a.path.cmp(&b.path));
    for file in sorted {
        let (name, rest) = match file.path.split_once('/') {
            Some((name, rest)) => (name.to_string(), rest),
            None => (String::new(), file.path.as_str()),
        };
        let at = match folders
            .iter()
            .position(|folder| folder.name == name)
        {
            Some(at) => at,
            None => {
                folders.push(Folder {
                    name: name.clone(),
                    description: String::new(),
                    files: Vec::new(),
                });
                folders.len() - 1
            }
        };
        // The folder's own index describes it and is not listed
        // among its files.
        if rest == "MEMORY.md" && !name.is_empty() {
            folders[at].description = file.description.clone();
        }
        folders[at].files.push(Entry {
            learned: learned.contains(&file.path),
            path: file.path,
            description: file.description,
            text: file.text,
        });
    }
    folders
}

/// Opens the file `path`, or shuts it when it is open.
fn toggle(world: &mut World, path: String) {
    let mut opened = world.resource_mut::<Opened>();
    opened.path = match opened.path.as_deref() {
        Some(now) if now == path => None,
        _ => Some(path),
    };
}

/// How big `text` is, in the words a file browser uses.
fn size(text: &str) -> String {
    match text.len() {
        1 => "1 byte".to_string(),
        n if n < 1000 => format!("{n} bytes"),
        n => format!("{:.1} KB", n as f64 / 1000.0),
    }
}

/// A file as a row: its name, its size and the way to open it, then
/// its text and what changes it while it is open.
fn file(entry: &Entry, open: bool) -> AnyView {
    let path = entry.path.clone();
    let name =
        entry.path.rsplit('/').next().unwrap_or("").to_string();
    let mark = if entry.learned { " (learned)" } else { "" };
    let named = lettered(format!("{name}{mark}"))
        .padding(UiRect::all(px(0.0)))
        .rules(hamlet_ui::styles::back_link)
        .tagged(Variant::Text)
        .on_activate({
            let path = path.clone();
            move |world| toggle(world, path.clone())
        });
    let head = row((
        glyph(Glyph::Notes, ICON).tone(Tone::Faint),
        column((named, note(size(&entry.text))))
            .gap(2.0)
            .grow(1.0)
            .min_width(px(0.0))
            .align(AlignItems::FlexStart),
        action(
            Glyph::Into,
            if open { "Close" } else { "Open" },
            move |world| toggle(world, path.clone()),
        ),
    ))
    .gap(8.0)
    .align(AlignItems::Center);
    let mut parts = vec![head.boxed()];
    if !entry.description.is_empty() {
        parts.push(note(entry.description.clone()).boxed());
    }
    if open {
        parts.push(
            hamlet_ui::markdown::fixed(&entry.text, Tone::Body)
                .boxed(),
        );
        let (edit, remove) = (entry.path.clone(), entry.path.clone());
        parts.push(
            row((
                action(Glyph::Rename, "Edit", move |world| {
                    papers::edit_memory(world, edit.clone());
                }),
                bad_action(Glyph::Delete, "Remove", move |world| {
                    papers::remove_memory(world, remove.clone());
                }),
            ))
            .gap(8.0)
            .align(AlignItems::Center)
            .tagged(Wraps)
            .boxed(),
        );
    }
    column(parts).gap(4.0).boxed()
}

fn folder(folder: &Folder, open: Option<&str>) -> AnyView {
    let mut parts = Vec::new();
    if folder.name.is_empty() {
        parts
            .push(label("In every prompt").tone(Tone::Faint).boxed());
    } else {
        parts.push(
            label(format!("{}/", folder.name))
                .bold(true)
                .tone(Tone::Faint)
                .boxed(),
        );
        if !folder.description.is_empty() {
            parts.push(note(folder.description.clone()).boxed());
        }
    }
    for entry in &folder.files {
        parts.push(file(entry, open == Some(entry.path.as_str())));
    }
    column(parts).gap(6.0).boxed()
}

/// The add field and the button that opens the new file.
fn add() -> impl View {
    with_cx(|cx| {
        let gap = cx.theme().space.gap;
        row((
            column(
                (label("New file").tone(Tone::Faint).wrap(false),),
            )
            .tooltip(|| {
                note(
                    "A name like people.md, or a folder and name \
                     like orchard/rows.md",
                )
            }),
            text_field(String::new(), |world, text| {
                world.resource_mut::<Opened>().name = text;
            })
            .grow(1.0)
            .min_width(px(0.0))
            .rules(text_input),
            action(Glyph::Plus, "Add file", |world| {
                let name = world.resource::<Opened>().name.clone();
                papers::add_memory(world, &name);
            }),
        ))
        .gap(gap)
        .align(AlignItems::Center)
    })
}

/// The skills of an agent or a scholar, each with what it does.
pub(super) fn skills() -> impl View {
    keyed(resource::<Papers, _>(|p| p.skills.clone()), |skills| {
        if skills.is_empty() {
            return dim("None of its own").boxed();
        }
        column(
            skills
                .iter()
                .map(|(name, does)| {
                    crate::lists::item(
                        name.clone(),
                        does.clone(),
                        Tone::Body,
                    )
                })
                .collect::<Vec<_>>(),
        )
        .gap(4.0)
        .boxed()
    })
    .within(bare())
}

/// The character of an agent or a scholar under its heading, with the
/// way to edit it.
pub(super) fn character_section() -> impl View {
    column((
        row((
            heading(Glyph::Face, "Character"),
            column(()).grow(1.0),
            action(Glyph::Rename, "Edit", papers::edit_character)
                .tooltip(|| {
                    note(
                        "Opens it in your text editor. It is saved \
                         here when you save it there.",
                    )
                }),
        ))
        .gap(8.0)
        .align(AlignItems::Center),
        character(),
    ))
    .gap(6.0)
}

/// The character of an agent or a scholar.
fn character() -> impl View {
    keyed(resource::<Papers, _>(|p| p.character.clone()), |text| {
        if text.is_empty() {
            dim("Not written yet").boxed()
        } else {
            hamlet_ui::markdown::fixed(text, Tone::Body).boxed()
        }
    })
    .within(bare())
}

/// The memory, with the failure of the last save under it.
pub(super) fn view() -> impl View {
    let shown = keyed(
        resource::<Papers, _>(|p| {
            (p.memory.clone(), p.learned.clone())
        }),
        |(memory, learned)| {
            if memory.is_empty() {
                return note("No memory files yet.").boxed();
            }
            let folders = folders(memory, learned);
            keyed(
                resource::<Opened, _>(|o| o.path.clone()),
                move |open| {
                    column(
                        folders
                            .iter()
                            .filter(|f| {
                                !f.files.is_empty()
                                    || !f.name.is_empty()
                            })
                            .map(|f| folder(f, open.as_deref()))
                            .collect::<Vec<_>>(),
                    )
                    .gap(10.0)
                    .boxed()
                },
            )
            .within(bare())
            .boxed()
        },
    )
    .within(bare());
    let failed = keyed(
        resource::<Papers, _>(|p| p.failed.clone()),
        |failed| match failed {
            Some(why) => {
                label(why.clone()).tone(Tone::Critical).boxed()
            }
            None => column(()).boxed(),
        },
    )
    .within(bare());
    column((shown, add(), failed)).gap(8.0)
}
