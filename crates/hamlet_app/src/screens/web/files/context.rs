//! Add to chat: what the Files panel's page hands the window, a
//! file's lines or a box drawn on what a viewer shows, each with the
//! chief's note if they wrote one, put in the chat's field for the
//! chief to send.
//!
//! The page (the fork's `hamlet.js`) emits `{hamletContext}` with the
//! lines and the file's text from a file shown as text, or, for any
//! other (a PDF, an image, a sheet, slides), with the box drawn on
//! the page, which the window captures as it draws it.

use std::io::Cursor;

use bevy::app::App;
use bevy::ecs::entity::Entity;
use bevy::ecs::observer::On;
use bevy::ecs::system::Commands;
use bevy::ecs::world::World;
use bevy::image::Image;
use bevy::math::{Rect, UVec2, Vec2};
use bevy::render::render_resource::TextureFormat;
use bevy::render::view::screenshot::{
    Screenshot, ScreenshotCaptured,
};
use bevy::ui::ComputedNode;
use bevy_cef::prelude::Receive;
use image::{ImageFormat, RgbaImage};
use serde::Deserialize;

use super::super::page::Page;
use super::id;
use crate::convo::Convo;
use crate::dock::{HUB, show_tab};
use crate::menu::rect_of;
use crate::screens::chat;

pub(super) fn plugin(app: &mut App) {
    app.add_observer(handed);
}

/// What the page hands over.
#[derive(Deserialize, Debug)]
struct Handed {
    #[serde(rename = "hamletContext")]
    context: Ask,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum Ask {
    /// Lines `start` to `end`, from 1, of the file at `path` in the
    /// folder, whose words are `text`.
    Lines {
        path: String,
        start: usize,
        end: usize,
        #[serde(default)]
        text: String,
        #[serde(default)]
        note: String,
    },
    /// A box drawn on the page, in the page's pixels, over a viewer
    /// of the file at `path`, on a page `view_width` by
    /// `view_height`.
    Region {
        path: String,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        #[serde(rename = "viewWidth")]
        view_width: f32,
        #[serde(rename = "viewHeight")]
        view_height: f32,
        #[serde(default)]
        note: String,
    },
}

/// Adds what the Files panel's page handed over to the chat.
fn handed(
    emitted: On<Receive<serde_json::Value>>,
    mut commands: Commands,
) {
    let Ok(handed) =
        serde_json::from_value::<Handed>(emitted.payload.clone())
    else {
        return;
    };
    let webview = emitted.webview;
    commands.queue(move |world: &mut World| {
        let ours = world
            .get::<Page>(webview)
            .is_some_and(|page| page.0 == id());
        if !ours {
            return;
        }
        match handed.context {
            Ask::Lines {
                path,
                start,
                end,
                text,
                note,
            } => {
                if let Some(words) =
                    lines(&path, start, end, &text, &note)
                {
                    add(world, &words, Vec::new());
                }
            }
            Ask::Region {
                path,
                x,
                y,
                width,
                height,
                view_width,
                view_height,
                note,
            } => {
                let part = Rect::new(x, y, x + width, y + height);
                let view = Vec2::new(view_width, view_height);
                capture(world, webview, path, part, view, note);
            }
        }
    });
}

/// Shows the chat, and puts `words` and `pictures` in its field.
fn add(world: &mut World, words: &str, pictures: Vec<Vec<u8>>) {
    show_tab(world, HUB);
    if let Some(agent) =
        world.get_resource::<Convo>().map(|convo| convo.agent)
    {
        crate::convo::talk_to(world, agent);
    }
    chat::add_to_field(world, words, pictures);
}

/// The words lines `start` to `end` of `text`, the file at `path`,
/// are added as: where they are from, the lines fenced, and `note`.
fn lines(
    path: &str,
    start: usize,
    end: usize,
    text: &str,
    note: &str,
) -> Option<String> {
    let all = text.lines().collect::<Vec<_>>();
    if all.is_empty() {
        return None;
    }
    let start = start.clamp(1, all.len());
    let end = end.clamp(start, all.len());
    let which = match start == end {
        true => format!("line {start}"),
        false => format!("lines {start}–{end}"),
    };
    let code = all[start - 1..end].join("\n");
    Some(format!(
        "From `{path}`, {which}:\n```{}\n{code}\n```\n{}",
        language(path),
        with_note(note),
    ))
}

/// `note` on a line of its own, when there is one.
fn with_note(note: &str) -> String {
    match note.trim() {
        "" => String::new(),
        note => format!("{note}\n"),
    }
}

/// Captures the window as it draws the page, keeping `part` of the
/// page, in the page's pixels.
fn capture(
    world: &mut World,
    webview: Entity,
    path: String,
    part: Rect,
    view: Vec2,
    note: String,
) {
    let Some(page) = rect_of(world, webview) else {
        return;
    };
    let Some(region) = on_the_page(page, part, view) else {
        return;
    };
    // The page's rect is in the panels' pixels, under the window's
    // scale and the zoom.
    let Some(scale) = world
        .get::<ComputedNode>(webview)
        .map(|node| node.inverse_scale_factor().recip())
    else {
        return;
    };
    world.spawn(Screenshot::primary_window()).observe(
        move |captured: On<ScreenshotCaptured>,
              mut commands: Commands| {
            let image = captured.image.clone();
            let (path, note) = (path.clone(), note.clone());
            commands.queue(move |world: &mut World| {
                let Some(png) = cut(&image, region, scale)
                    .and_then(|(pixels, size)| png(pixels, size))
                else {
                    return;
                };
                let words =
                    format!("From `{path}`:\n{}", with_note(&note));
                add(world, &words, vec![png]);
            });
        },
    );
}

/// Where `part` of a page `view` big, in the page's own pixels, is in
/// `page`, the rect the page is drawn in: the page draws at its own
/// zoom, so its pixels are not the panels'.
fn on_the_page(page: Rect, part: Rect, view: Vec2) -> Option<Rect> {
    if view.x <= 0.0 || view.y <= 0.0 {
        return None;
    }
    let to = page.size() / view;
    let region = Rect::from_corners(
        page.min + part.min * to,
        page.min + part.max * to,
    )
    .intersect(page);
    (!region.is_empty()).then_some(region)
}

/// The RGBA pixels of `region`, in the panels' pixels, of a capture
/// of the window, `scale` window pixels to each, and their size.
fn cut(
    window: &Image,
    region: Rect,
    scale: f32,
) -> Option<(Vec<u8>, UVec2)> {
    let data = window.data.as_ref()?;
    let full = UVec2::new(window.width(), window.height());
    let bgra = matches!(
        window.texture_descriptor.format,
        TextureFormat::Bgra8Unorm | TextureFormat::Bgra8UnormSrgb
    );
    let at = |value: f32, most: u32| {
        ((value * scale).round().max(0.0) as u32).min(most)
    };
    let (left, top) =
        (at(region.min.x, full.x), at(region.min.y, full.y));
    let (right, bottom) =
        (at(region.max.x, full.x), at(region.max.y, full.y));
    if right <= left || bottom <= top {
        return None;
    }
    let row = full.x as usize * 4;
    if data.len() < row * full.y as usize {
        return None;
    }
    let mut pixels = Vec::with_capacity(
        ((right - left) * (bottom - top) * 4) as usize,
    );
    for y in top..bottom {
        let start = y as usize * row + left as usize * 4;
        let end = y as usize * row + right as usize * 4;
        for [b, g, r, _] in data[start..end].as_chunks::<4>().0 {
            match bgra {
                true => pixels.extend([*r, *g, *b, 255]),
                false => pixels.extend([*b, *g, *r, 255]),
            }
        }
    }
    Some((pixels, UVec2::new(right - left, bottom - top)))
}

/// The RGBA `pixels` of `size`, as a PNG.
fn png(pixels: Vec<u8>, size: UVec2) -> Option<Vec<u8>> {
    let picture = RgbaImage::from_raw(size.x, size.y, pixels)?;
    let mut bytes = Cursor::new(Vec::new());
    picture.write_to(&mut bytes, ImageFormat::Png).ok()?;
    Some(bytes.into_inner())
}

/// The name a fenced block of the file at `path` is marked with.
fn language(path: &str) -> &'static str {
    let extension =
        path.rsplit_once('.').map_or("", |(_, extension)| extension);
    match extension {
        "rs" => "rust",
        "ts" | "tsx" => "ts",
        "js" | "jsx" | "mjs" | "cjs" => "js",
        "py" => "python",
        "go" => "go",
        "md" => "markdown",
        "json" => "json",
        "toml" => "toml",
        "yaml" | "yml" => "yaml",
        "sh" | "bash" => "sh",
        "html" => "html",
        "css" => "css",
        "c" | "h" => "c",
        "cpp" | "hpp" | "cc" => "cpp",
        "csv" => "csv",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "one\ntwo\nthree\nfour";

    #[test]
    fn lines_are_fenced_with_where_they_are_from_and_the_note() {
        assert_eq!(
            lines("src/a.rs", 2, 3, FILE, " check this ").as_deref(),
            Some(
                "From `src/a.rs`, lines \
                 2–3:\n```rust\ntwo\nthree\n```\ncheck this\n"
            ),
        );
    }

    #[test]
    fn lines_go_without_a_note() {
        assert_eq!(
            lines("notes.txt", 4, 4, FILE, "").as_deref(),
            Some("From `notes.txt`, line 4:\n```\nfour\n```\n"),
        );
    }

    #[test]
    fn a_choice_past_the_file_is_kept_inside_it() {
        assert_eq!(
            lines("a.md", 3, 99, FILE, "").as_deref(),
            Some(
                "From `a.md`, lines \
                 3–4:\n```markdown\nthree\nfour\n```\n"
            ),
        );
        assert_eq!(lines("a.md", 1, 1, "", "x"), None);
    }

    #[test]
    fn the_page_hands_a_box_with_its_note() {
        let handed = serde_json::from_value::<Handed>(serde_json::json!({
            "hamletContext": {
                "kind": "region", "folder": "/f", "path": "docs/a.pdf",
                "x": 1.0, "y": 2.0, "width": 30.0, "height": 40.0,
                "viewWidth": 800.0, "viewHeight": 600.0,
                "note": "the chart",
            }
        }))
        .unwrap();
        assert_eq!(
            handed.context,
            Ask::Region {
                path: "docs/a.pdf".into(),
                x: 1.0,
                y: 2.0,
                width: 30.0,
                height: 40.0,
                view_width: 800.0,
                view_height: 600.0,
                note: "the chart".into(),
            }
        );
    }

    #[test]
    fn a_box_is_found_on_a_page_drawn_at_its_own_zoom() {
        let page = Rect::new(16.0, 68.0, 816.0, 568.0);
        let view = Vec2::new(1000.0, 625.0);
        assert_eq!(
            on_the_page(
                page,
                Rect::new(100.0, 100.0, 600.0, 350.0),
                view
            ),
            Some(Rect::new(96.0, 148.0, 496.0, 348.0)),
        );
        assert_eq!(
            on_the_page(
                page,
                Rect::new(900.0, 500.0, 1200.0, 700.0),
                view
            ),
            Some(Rect::new(736.0, 468.0, 816.0, 568.0)),
        );
        assert_eq!(
            on_the_page(
                page,
                Rect::new(0.0, 0.0, 10.0, 10.0),
                Vec2::ZERO
            ),
            None,
        );
    }
}
