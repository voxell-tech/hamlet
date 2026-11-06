//! The marks of the model providers, from lobe-icons
//! (`providers/LICENSE`): drawn from their SVGs in white at the
//! icons' sizes, to be tinted by a text tone like the icons are.

use bevy::app::{App, Startup};
use bevy::asset::uuid::Uuid;
use bevy::asset::{Assets, Handle, RenderAssetUsages};
use bevy::ecs::system::ResMut;
use bevy::image::Image;
use bevy::render::render_resource::{
    Extent3d, TextureDimension, TextureFormat,
};
use resvg::{tiny_skia, usvg};

use super::icons::{DENSITY, SIZES};

/// The handles of the marks' images start here.
const FIRST: u128 = 0x6861_6d6c_6574_5f70_726f_7669_0000_0000;

pub fn plugin(app: &mut App) {
    app.add_systems(Startup, draw);
}

/// Every mark, by the name of its file.
const MARKS: [(&str, &str); 29] = [
    ("anthropic", include_str!("providers/anthropic.svg")),
    ("azure", include_str!("providers/azure.svg")),
    ("baseten", include_str!("providers/baseten.svg")),
    ("bedrock", include_str!("providers/bedrock.svg")),
    ("cerebras", include_str!("providers/cerebras.svg")),
    ("cloudflare", include_str!("providers/cloudflare.svg")),
    ("deepseek", include_str!("providers/deepseek.svg")),
    ("fireworks", include_str!("providers/fireworks.svg")),
    ("gemini", include_str!("providers/gemini.svg")),
    ("githubcopilot", include_str!("providers/githubcopilot.svg")),
    ("groq", include_str!("providers/groq.svg")),
    ("huggingface", include_str!("providers/huggingface.svg")),
    ("kimi", include_str!("providers/kimi.svg")),
    ("ling", include_str!("providers/ling.svg")),
    ("meta", include_str!("providers/meta.svg")),
    ("minimax", include_str!("providers/minimax.svg")),
    ("mistral", include_str!("providers/mistral.svg")),
    ("moonshot", include_str!("providers/moonshot.svg")),
    ("nvidia", include_str!("providers/nvidia.svg")),
    ("openai", include_str!("providers/openai.svg")),
    ("opencode", include_str!("providers/opencode.svg")),
    ("openrouter", include_str!("providers/openrouter.svg")),
    ("qwen", include_str!("providers/qwen.svg")),
    ("together", include_str!("providers/together.svg")),
    ("typesafeai", include_str!("providers/typesafeai.svg")),
    ("vercel", include_str!("providers/vercel.svg")),
    ("vertexai", include_str!("providers/vertexai.svg")),
    ("xai", include_str!("providers/xai.svg")),
    ("zai", include_str!("providers/zai.svg")),
];

/// The name of the mark of the provider `id`, as the model catalogue
/// names it.
fn mark_of(id: &str) -> Option<&'static str> {
    Some(match id {
        "anthropic" => "anthropic",
        "openai" | "openai-codex" | "chatgpt" => "openai",
        "google" => "gemini",
        "google-vertex" => "vertexai",
        "amazon-bedrock" => "bedrock",
        "azure" => "azure",
        "baseten" => "baseten",
        "cerebras" => "cerebras",
        "deepseek" => "deepseek",
        "fireworks" => "fireworks",
        "github-copilot" => "githubcopilot",
        "groq" => "groq",
        "huggingface" => "huggingface",
        "kimi-coding" => "kimi",
        "ant-ling" => "ling",
        "meta" => "meta",
        "minimax" | "minimax-cn" => "minimax",
        "mistral" => "mistral",
        "moonshotai" | "moonshotai-cn" => "moonshot",
        "nvidia" => "nvidia",
        "opencode" | "opencode-go" => "opencode",
        "openrouter" => "openrouter",
        "together" => "together",
        "typesafe" => "typesafeai",
        "vercel-ai-gateway" => "vercel",
        "xai" => "xai",
        "zai" => "zai",
        id if id.starts_with("qwen") => "qwen",
        id if id.starts_with("cloudflare") => "cloudflare",
        _ => return None,
    })
}

/// The image of the mark of the provider `id` for a side of `size`;
/// none for a provider with no mark, such as an endpoint of the
/// chief's own.
pub fn provider_mark(id: &str, size: f32) -> Option<Handle<Image>> {
    let name = mark_of(id)?;
    let mark = MARKS.iter().position(|(file, _)| *file == name)?;
    // The smallest drawing that is not stretched, or the largest.
    let drawn = SIZES
        .iter()
        .position(|drawn| *drawn >= size)
        .unwrap_or(SIZES.len() - 1);
    Some(handle(mark, drawn))
}

fn handle(mark: usize, drawn: usize) -> Handle<Image> {
    let at = (mark * SIZES.len() + drawn) as u128;
    Handle::from(Uuid::from_u128(FIRST + at))
}

/// `svg` in white on nothing, filling a square of `size`; none if it
/// does not read.
fn drawing(svg: &str, size: f32) -> Option<Image> {
    let side = (size * DENSITY).round() as u32;
    let tree =
        usvg::Tree::from_str(svg, &usvg::Options::default()).ok()?;
    let drawn = tree.size();
    let scale = side as f32 / drawn.width().max(drawn.height());
    let offset = |along: f32| (side as f32 - along * scale) / 2.0;
    let mut pixmap = tiny_skia::Pixmap::new(side, side)?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale)
            .post_translate(
                offset(drawn.width()),
                offset(drawn.height()),
            ),
        &mut pixmap.as_mut(),
    );
    // Only how covered each pixel is, the colour coming from the
    // tint.
    let data = pixmap
        .data()
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|pixel| [255, 255, 255, pixel[3]])
        .collect();
    Some(Image::new(
        Extent3d {
            width: side,
            height: side,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    ))
}

/// Draws every mark at every size.
fn draw(mut images: ResMut<Assets<Image>>) {
    for (mark, (_, svg)) in MARKS.iter().enumerate() {
        for (drawn, size) in SIZES.into_iter().enumerate() {
            if let Some(image) = drawing(svg, size) {
                let _ = images.insert(&handle(mark, drawn), image);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_mark_reads_and_draws_something() {
        for (name, svg) in MARKS {
            let image = drawing(svg, 16.0).expect(name);
            let data = image.data.expect(name);
            assert!(
                data.as_chunks::<4>()
                    .0
                    .iter()
                    .any(|pixel| pixel[3] > 0),
                "{name} draws nothing"
            );
        }
    }
}
