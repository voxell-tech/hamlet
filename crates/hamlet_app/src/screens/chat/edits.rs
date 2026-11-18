//! The files a turn changed, as a card that opens to their tree.

use bevy::ecs::entity::Entity;
use bevy::ui::{
    AlignItems, JustifyContent, Overflow, UiRect, percent, px,
};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, button, column, frame,
    label, row,
};
use bevy_fynix::{ScopedExt, ViewExt, keyed, resource};
use hamlet_client::chats::{Entry, Transcript};
use hamlet_protocol::client::Change;
use hamlet_ui::kinds::card;
use hamlet_ui::styles::{note, small_rules, text_button};
use hamlet_ui::widgets::ellipsis::{Keep, one_line};
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::{AnyView, Cx, View, with_cx};

use super::change::{counted, diff};
use super::entry::{Fold, OpenTools, toggle};

/// One file a turn changed, with each edit of it.
struct File {
    path: String,
    /// Its folder below the folder every file shares, and its name.
    folder: String,
    name: String,
    edits: Vec<(String, Change)>,
    added: u64,
    removed: u64,
}

fn parts(path: &str) -> Vec<&str> {
    path.split(['/', '\\'])
        .filter(|part| !part.is_empty())
        .collect()
}

/// The files `edits` changed, by folder.
fn files(transcript: &Transcript, edits: &[String]) -> Vec<File> {
    let mut files = Vec::<File>::new();
    for id in edits {
        let Some(Entry::Tool {
            change: Some(change),
            ..
        }) = transcript.item(id).map(|item| &item.entry)
        else {
            continue;
        };
        let file =
            match files.iter().position(|f| f.path == change.path) {
                Some(at) => &mut files[at],
                None => {
                    files.push(File {
                        path: change.path.clone(),
                        folder: String::new(),
                        name: String::new(),
                        edits: Vec::new(),
                        added: 0,
                        removed: 0,
                    });
                    files.last_mut().expect("just pushed")
                }
            };
        file.added += change.added;
        file.removed += change.removed;
        file.edits.push((id.clone(), change.clone()));
    }
    // The folder every file is in is said once, over the tree.
    let folders = files
        .iter()
        .map(|file| {
            let parts = parts(&file.path);
            parts[..parts.len().saturating_sub(1)]
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let shared = folders.first().map_or(0, |first| {
        (0..first.len())
            .take_while(|at| {
                folders.iter().all(|f| f.get(*at) == first.get(*at))
            })
            .count()
    });
    // Its last part names the files' own folder when they share one.
    let shared = shared.saturating_sub(1);
    for (file, folder) in files.iter_mut().zip(&folders) {
        let parts = parts(&file.path);
        file.name = parts.last().copied().unwrap_or("").to_string();
        file.folder = folder[shared.min(folder.len())..].join("/");
    }
    files.sort_by(|a, b| {
        (&a.folder, &a.name).cmp(&(&b.folder, &b.name))
    });
    files
}

/// The lines added and taken out of `files`, as one change.
fn total(files: &[File]) -> Change {
    let (added, removed) =
        files.iter().fold((0, 0), |(a, r), file| {
            (a + file.added, r + file.removed)
        });
    Change {
        added,
        count: format!("{added} lines added, {removed} removed"),
        created: false,
        lines: Vec::new(),
        path: String::new(),
        removed,
        total: 0,
    }
}

/// A chevron that points down while the block `key` is open.
fn chevron(key: Fold) -> impl View {
    glyph(Glyph::Chevron, 10.0).rotation(resource::<OpenTools, _>(
        move |open| {
            if open.0.contains(&key) { 180.0 } else { 90.0 }
        },
    ))
}

/// "2 changed files +13 -5", which opens to the tree of the files, in
/// `hub`.
pub(super) fn view(
    hub: Entity,
    chat: Entity,
    first: String,
    edits: Vec<String>,
) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let Some(transcript) = cx.world.get::<Transcript>(chat)
        else {
            return cx.build(frame());
        };
        let files = files(transcript, &edits);
        let key = (hub, chat, format!("{first} files"));
        let pressed = key.clone();
        let count = match files.len() {
            1 => "1 changed file".to_string(),
            n => format!("{n} changed files"),
        };
        let head = button(
            row((
                chevron(key.clone()),
                label(count).wrap(false).rules(small_rules),
                counted(&total(&files)),
            ))
            .gap(cx.theme().space.gap / 2.0)
            .align(AlignItems::Center),
        )
        .justify(JustifyContent::FlexStart)
        .rules(text_button)
        .on_activate(move |world| toggle(world, pressed.clone()));
        let tree = std::sync::Arc::new(
            files
                .into_iter()
                .map(|file| (file, first.clone()))
                .collect::<Vec<_>>(),
        );
        let body = keyed(
            resource::<OpenTools, _>(move |open| {
                open.0.contains(&key)
            }),
            move |open| match open {
                true => folders(hub, chat, tree.clone()).boxed(),
                false => frame().boxed(),
            },
        )
        .within(column(()).width(percent(100.0)).min_width(px(0.0)));
        cx.build(
            card((head, body))
                .width(percent(100.0))
                .min_width(px(0.0)),
        )
    })
}

/// Each folder of `tree` by its files.
fn folders(
    hub: Entity,
    chat: Entity,
    tree: std::sync::Arc<Vec<(File, String)>>,
) -> impl View {
    with_cx(move |cx| {
        let gap = cx.theme().space.gap;
        let mut rows = Vec::new();
        let mut folder = None::<&str>;
        for (file, first) in tree.iter() {
            if folder != Some(file.folder.as_str()) {
                folder = Some(file.folder.as_str());
                if !file.folder.is_empty() {
                    rows.push(
                        row((
                            glyph(Glyph::Folder, 12.0)
                                .tone(Tone::Dim),
                            column((one_line(
                                label(file.folder.clone())
                                    .tone(Tone::Dim),
                            )
                            .keep(Keep::Ends)
                            .rules(small_rules),))
                            .grow(1.0)
                            .min_width(px(0.0))
                            .overflow(Overflow::clip_x()),
                        ))
                        .gap(gap / 2.0)
                        .align(AlignItems::Center)
                        .padding(UiRect::vertical(px(2.0)))
                        .boxed(),
                    );
                }
            }
            rows.push(file_row(hub, chat, first, file).boxed());
        }
        column(rows).width(percent(100.0)).min_width(px(0.0))
    })
}

/// A file of the tree, which opens to the lines its edits changed.
fn file_row(
    hub: Entity,
    chat: Entity,
    first: &str,
    file: &File,
) -> impl View {
    let key = (hub, chat, format!("{first} {}", file.path));
    let pressed = key.clone();
    let whole = file.path.clone();
    let summed = Change {
        added: file.added,
        count: format!(
            "{} lines added, {} removed",
            file.added, file.removed
        ),
        created: false,
        lines: Vec::new(),
        path: file.path.clone(),
        removed: file.removed,
        total: 0,
    };
    let head = button(
        row((
            chevron(key.clone()),
            glyph(Glyph::Notes, 12.0).tone(Tone::Dim),
            label(file.name.clone()).wrap(false).rules(small_rules),
            counted(&summed),
        ))
        .gap(6.0)
        .align(AlignItems::Center),
    )
    .justify(JustifyContent::FlexStart)
    .rules(text_button)
    .tooltip(move || note(whole.clone()))
    .on_activate(move |world| toggle(world, pressed.clone()));
    let edits = file.edits.clone();
    let body = keyed(
        resource::<OpenTools, _>(move |open| open.0.contains(&key)),
        move |open| match open {
            true => column(
                edits
                    .iter()
                    .map(|(id, change)| {
                        diff(hub, chat, id.clone(), change.clone())
                    })
                    .collect::<Vec<_>>(),
            )
            .gap(4.0)
            .width(percent(100.0))
            .min_width(px(0.0))
            .boxed(),
            false => frame().boxed(),
        },
    )
    .within(column(()).width(percent(100.0)).min_width(px(0.0)));
    with_cx(move |cx| {
        let gap = cx.theme().space.gap;
        column((head, body))
            .gap(gap / 4.0)
            .width(percent(100.0))
            .min_width(px(0.0))
            .padding(UiRect::left(px(gap)))
    })
}
