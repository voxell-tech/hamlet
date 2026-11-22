//! The memory, skills and character of an agent or a profile.

use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;
use bevy::ui::{AlignItems, UiRect, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, column, frame, label, row,
    text_field,
};
use bevy_fynix::{
    Binding, ScopedExt, Signal, ViewExt, component, keyed,
};
use hamlet_protocol::client as protocol;
use hamlet_ui::styles::{
    ICON, Looks, action, bad_action, bare, buttons, good_action,
    lettered, note, text_input,
};
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::hint::hinted;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::widgets::platform::Wraps;
use hamlet_ui::{AnyView, View, with_cx};

use super::details::{section, section_line};
use super::papers::{self, Papers};
use super::thread::dim;

/// The memory file open in a Hub, and the name typed for a new one.
#[derive(Component, Default, PartialEq)]
pub struct Opened {
    pub path: Option<String>,
    pub name: String,
    pub adding: bool,
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
        // A folder's own index describes it and is not listed.
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

/// Opens the file `path` in `hub`, or shuts it when it is open.
fn toggle(world: &mut World, hub: Entity, path: String) {
    let Some(mut opened) = world.get_mut::<Opened>(hub) else {
        return;
    };
    opened.path = match opened.path.as_deref() {
        Some(now) if now == path => None,
        _ => Some(path),
    };
}

/// Changes what `hub` has open with `change`.
fn opened(
    world: &mut World,
    hub: Entity,
    change: impl FnOnce(&mut Opened),
) {
    if let Some(mut opened) = world.get_mut::<Opened>(hub) {
        change(&mut opened);
    }
}

/// What `read` gives of what `hub` has open.
fn opened_in<T: Default + Send + Sync + 'static>(
    hub: Entity,
    read: impl Fn(&Opened) -> T + Send + Sync + 'static,
) -> Signal<T> {
    component::<Opened, _>(hub, move |o| {
        o.map(&read).unwrap_or_default()
    })
}

/// What `read` gives of the papers `hub` shows.
fn papers_in<T: Default + Send + Sync + 'static>(
    hub: Entity,
    read: impl Fn(&Papers) -> T + Send + Sync + 'static,
) -> Signal<T> {
    component::<Papers, _>(hub, move |p| {
        p.map(&read).unwrap_or_default()
    })
}

/// How big `text` is, in the words a file browser uses.
fn size(text: &str) -> String {
    match text.len() {
        1 => "1 byte".to_string(),
        n if n < 1000 => format!("{n} bytes"),
        n => format!("{:.1} KB", n as f64 / 1000.0),
    }
}

/// What makes the menu at the end of a file's row, from its Hub and
/// path.
pub(super) type FileMenu = Option<fn(Entity, &str) -> AnyView>;

/// A file's row in `hub`, and its text while it is open.
fn file(
    hub: Entity,
    entry: &Entry,
    open: bool,
    menu: FileMenu,
) -> AnyView {
    let path = entry.path.clone();
    let name =
        entry.path.rsplit('/').next().unwrap_or("").to_string();
    let mark = if entry.learned { " (learned)" } else { "" };
    let named = lettered(format!("{name}{mark}"))
        .padding(UiRect::all(px(0.0)))
        .variant(Variant::Text)
        .on_activate({
            let path = path.clone();
            move |world| toggle(world, hub, path.clone())
        });
    let mut head = vec![
        glyph(Glyph::Notes, ICON).tone(Tone::Faint).boxed(),
        named.boxed(),
        note(size(&entry.text)).boxed(),
        frame().grow(1.0).boxed(),
        action(Glyph::Into, if open { "Close" } else { "Open" }, {
            let path = path.clone();
            move |world| toggle(world, hub, path.clone())
        }),
    ];
    head.extend(menu.map(|menu| menu(hub, &path)));
    let head = row(head).gap(8.0).align(AlignItems::Center);
    let mut parts = vec![head.boxed()];
    if !entry.description.is_empty() {
        parts.push(note(entry.description.clone()).boxed());
    }
    // A file in every prompt shows what it says before it is opened.
    if !open && !entry.path.contains('/') {
        let lines = gist(&entry.text);
        if !lines.is_empty() {
            parts.push(quote(label(lines).tone(Tone::Dim)).boxed());
        }
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
                    papers::edit_memory(world, hub, edit.clone());
                }),
                bad_action(Glyph::Delete, "Remove", move |world| {
                    papers::remove_memory(world, hub, remove.clone());
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

/// `content` set off as a quote, with a rule down its left.
fn quote(content: impl View + 'static) -> impl View {
    with_cx(move |cx| {
        row((
            frame().width(px(2.0)).fill(cx.theme().palette.mark),
            column((content,)).grow(1.0).min_width(px(0.0)),
        ))
        .gap(8.0)
    })
}

/// Lines a closed file shows of its text.
const GIST: usize = 3;

/// The first [`GIST`] lines of prose in the Markdown `text`.
fn gist(text: &str) -> String {
    let mut fenced = false;
    let mut lines = Vec::new();
    for line in text.lines().map(str::trim) {
        if line.starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced
            || line.is_empty()
            || line.starts_with(['#', '|'])
            || line.chars().all(|c| "-*_=".contains(c))
        {
            continue;
        }
        let line = line
            .trim_start_matches("- [x] ")
            .trim_start_matches("- [ ] ")
            .trim_start_matches(['-', '*', '>', ' ']);
        lines.push(line.replace("**", "").replace('`', ""));
        if lines.len() == GIST {
            break;
        }
    }
    lines.join("\n")
}

fn folder(
    hub: Entity,
    folder: &Folder,
    open: Option<&str>,
    menu: FileMenu,
) -> AnyView {
    let mut parts = Vec::new();
    if !folder.name.is_empty() {
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
        parts.push(file(
            hub,
            entry,
            open == Some(entry.path.as_str()),
            menu,
        ));
    }
    column(parts).gap(6.0).boxed()
}

/// The button that adds a file in `hub`, or the field its name is
/// typed in.
fn add(
    hub: Entity,
    learn: impl Fn() -> Option<AnyView> + Send + Sync + 'static,
) -> impl View {
    keyed(opened_in(hub, |o| o.adding), move |adding| {
        if !*adding {
            let mut parts =
                vec![action(Glyph::Plus, "Add File", move |world| {
                    opened(world, hub, |o| o.adding = true);
                })];
            parts.extend(learn());
            return buttons(parts).boxed();
        }
        with_cx(move |cx| {
            let gap = cx.theme().space.gap;
            // The name typed before the sheet was built again.
            let typed = cx
                .world
                .get::<Opened>(hub)
                .map(|o| o.name.clone())
                .unwrap_or_default();
            row((
                hinted(
                    text_field(Binding::new(
                        typed,
                        move |world, text| {
                            opened(world, hub, |o| o.name = text);
                        },
                    ))
                    .grow(1.0)
                    .min_width(px(0.0))
                    .rules(text_input),
                    "people.md, or orchard/rows.md",
                ),
                good_action(Glyph::Plus, "Add", move |world| {
                    let mut name = String::new();
                    opened(world, hub, |o| {
                        if !o.name.trim().is_empty() {
                            name = std::mem::take(&mut o.name);
                            o.adding = false;
                        }
                    });
                    if !name.is_empty() {
                        papers::add_memory(world, hub, &name);
                    }
                }),
                action(Glyph::Cross, "Cancel", move |world| {
                    opened(world, hub, |o| {
                        o.adding = false;
                        o.name.clear();
                    });
                }),
            ))
            .gap(gap)
            .align(AlignItems::Center)
        })
        .boxed()
    })
    .within(bare())
}

/// The skills of the agent or the profile `hub` shows.
pub(super) fn skills_section(hub: Entity) -> AnyView {
    keyed(papers_in(hub, |p| p.skills.clone()), |skills| {
        if skills.is_empty() {
            return section_line(
                Glyph::Book,
                "Skills",
                dim("None of its own").boxed(),
            );
        }
        let rows = skills
            .iter()
            .map(|(name, does)| {
                crate::lists::item(
                    name.clone(),
                    does.clone(),
                    Tone::Body,
                )
            })
            .collect::<Vec<_>>();
        section(Glyph::Book, "Skills", None, column(rows).gap(4.0))
    })
    .within(bare())
    .boxed()
}

/// The character of the agent or the profile `hub` shows, with the
/// way to edit it.
pub(super) fn character_section(hub: Entity) -> AnyView {
    section(
        Glyph::Face,
        "Character",
        Some(
            action(Glyph::Rename, "Edit", move |world| {
                papers::edit_character(world, hub);
            })
            .tooltip(|| {
                note(
                    "Opens it in your text editor. It is saved here \
                     when you save it there.",
                )
            })
            .boxed(),
        ),
        character(hub),
    )
}

/// The character of the agent or the profile `hub` shows.
fn character(hub: Entity) -> impl View {
    keyed(papers_in(hub, |p| p.character.clone()), |text| {
        if text.is_empty() {
            dim("Not written yet").boxed()
        } else {
            hamlet_ui::markdown::fixed(text, Tone::Body).boxed()
        }
    })
    .within(bare())
}

/// The files of the memory `hub` shows, the way to add one, and the
/// last save's failure.
pub(super) fn view(
    hub: Entity,
    learn: impl Fn() -> Option<AnyView> + Send + Sync + 'static,
    menu: FileMenu,
) -> impl View {
    let shown = keyed(
        papers_in(hub, |p| (p.memory.clone(), p.learned.clone())),
        move |(memory, learned)| {
            if memory.is_empty() {
                return note("No memory files yet.").boxed();
            }
            let folders = folders(memory, learned);
            keyed(opened_in(hub, |o| o.path.clone()), move |open| {
                column(
                    folders
                        .iter()
                        .filter(|f| {
                            !f.files.is_empty() || !f.name.is_empty()
                        })
                        .map(|f| {
                            folder(hub, f, open.as_deref(), menu)
                        })
                        .collect::<Vec<_>>(),
                )
                .gap(8.0)
                .boxed()
            })
            .within(bare())
            .boxed()
        },
    )
    .within(bare());
    let failed =
        keyed(papers_in(hub, |p| p.failed.clone()), |failed| {
            match failed {
                Some(why) => {
                    label(why.clone()).tone(Tone::Critical).boxed()
                }
                None => column(()).boxed(),
            }
        })
        .within(bare());
    column((shown, add(hub, learn), failed)).gap(8.0)
}
