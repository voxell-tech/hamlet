//! The MEMORY of an agent or a scholar, shown as the folder it is:
//! the files at its root go into every prompt, and a folder is
//! described by its own `MEMORY.md`.

use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use bevy::ui::AlignItems;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, label, row, text_field,
};
use bevy_fynix::{ScopedExt, ViewExt, keyed, resource};
use hamlet_protocol::client as protocol;
use hamlet_ui::styles::{
    action, bad_action, bare, lettered, note, text_input,
};
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::widgets::platform::Wraps;
use hamlet_ui::{AnyView, View, with_cx};

use super::papers::{self, Papers};

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

fn file(entry: &Entry, open: bool) -> AnyView {
    let path = entry.path.clone();
    let name =
        entry.path.rsplit('/').next().unwrap_or("").to_string();
    let mark = if entry.learned { " (learned)" } else { "" };
    let head = lettered(format!("{name}{mark}"))
        .rules(hamlet_ui::styles::back_link)
        .tagged(Variant::Text)
        .on_activate({
            let path = path.clone();
            move |world| toggle(world, path.clone())
        });
    let mut parts = vec![head.boxed()];
    if !entry.description.is_empty() {
        parts.push(note(entry.description.clone()).boxed());
    }
    if open {
        parts.push(
            hamlet_ui::markdown::fixed(&entry.text, Tone::Body)
                .boxed(),
        );
        let (edit, remove) = (path.clone(), path);
        parts.push(
            row((
                action(Glyph::Rename, "EDIT", move |world| {
                    papers::edit_memory(world, edit.clone());
                }),
                bad_action(Glyph::Delete, "REMOVE", move |world| {
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
            text_field(String::new(), |world, text| {
                world.resource_mut::<Opened>().name = text;
            })
            .grow(1.0)
            .rules(text_input),
            action(Glyph::Plus, "ADD", |world| {
                let name = world.resource::<Opened>().name.clone();
                papers::add_memory(world, &name);
            }),
        ))
        .gap(gap)
        .align(AlignItems::Center)
    })
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
    column((shown, add(), note("Name a file to start it."), failed))
        .gap(8.0)
}
