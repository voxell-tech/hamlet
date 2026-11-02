//! The frame chart: a bar of each recorded frame's main thread,
//! render thread and GPU time side by side, with lines at one and two
//! frames of 60 Hz. A click on a frame picks it.

use std::time::Duration;

use bevy::app::{App, Update};
use bevy::asset::{Assets, RenderAssetUsages};
use bevy::color::{Color, ColorToPacked};
use bevy::ecs::component::Component;
use bevy::ecs::observer::On;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::schedule::common_conditions::resource_exists;
use bevy::ecs::system::{Query, Res, ResMut};
use bevy::image::{Image, ImageSampler};
use bevy::math::UVec2;
use bevy::picking::events::PointerClick;
use bevy::render::render_resource::{
    Extent3d, TextureDimension, TextureFormat,
};
use bevy::ui::widget::ImageNode;
use bevy::ui::{ComputedNode, percent, px};
use bevy_fynix::views::frame;
use hamlet_profiler::{Frame, KEPT, Profiler};
use hamlet_ui::theme::Themed;
use hamlet_ui::{View, built};

use super::Watch;

/// The chart's height, in logical pixels.
const HEIGHT: f32 = 120.0;

/// The narrowest a frame's bars are drawn, in pixels: one each.
const NARROWEST: u32 = 3;

/// The time the top of the chart stands for, at the least.
const LEAST_TOP: Duration = Duration::from_millis(40);

/// One and two frames at 60 Hz.
const LINES: [Duration; 2] =
    [Duration::from_micros(16_667), Duration::from_micros(33_333)];

pub(super) fn plugin(app: &mut App) {
    app.add_systems(Update, draw.run_if(resource_exists::<Profiler>));
}

/// The chart's node, and where its frames were drawn last.
#[derive(Component, Default)]
struct Chart {
    drawn: Option<Drawn>,
}

/// What a chart was drawn from, and its frames' places.
#[derive(Clone, Copy, PartialEq)]
struct Drawn {
    size: UVec2,
    shown: Option<u64>,
    newest: Option<u64>,
    /// How many of the newest frames' times were in.
    filled: usize,
    /// How many frames fit across.
    slots: u32,
    /// The slot of the oldest frame drawn, and its number.
    first: (u32, u64),
}

/// The chart, drawn into an image the size of its node.
pub(super) fn chart() -> impl View {
    built(|cx| {
        let theme = cx.theme();
        let (fill, round) = (theme.palette.input, theme.space.round);
        let node = cx.build(
            frame()
                .width(percent(100.0))
                .height(px(HEIGHT))
                .shrink(0.0)
                .fill(fill)
                .radius(round),
        );
        let image = cx
            .world
            .resource_mut::<Assets<Image>>()
            .add(canvas(UVec2::ONE, vec![0; 4]));
        cx.world
            .entity_mut(node)
            .insert((Chart::default(), ImageNode::new(image)))
            .observe(pick);
        node
    })
}

fn canvas(size: UVec2, pixels: Vec<u8>) -> Image {
    let mut image = Image::new(
        Extent3d {
            width: size.x,
            height: size.y,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::nearest();
    image
}

/// Picks the frame clicked, and stops the recording so it stays.
fn pick(
    click: On<PointerClick>,
    charts: Query<&Chart>,
    profiler: Res<Profiler>,
    mut watch: ResMut<Watch>,
) {
    let (Ok(chart), Some(at)) =
        (charts.get(click.entity), click.hit.position)
    else {
        return;
    };
    let Some(drawn) = chart.drawn else {
        return;
    };
    let slot = ((at.x + 0.5) * drawn.slots as f32) as u32;
    let (first_slot, first) = drawn.first;
    let Some(number) = slot
        .checked_sub(first_slot)
        .map(|after| first + u64::from(after))
    else {
        return;
    };
    if profiler.lock().frame(number).is_none() {
        return;
    }
    if profiler.is_recording() {
        profiler.set_recording(false);
        watch.recording = false;
    }
    watch.picked = Some(number);
}

/// Draws each chart again when its size, the frame shown or the
/// frames have changed.
fn draw(
    mut charts: Query<(&mut Chart, &ImageNode, &ComputedNode)>,
    mut images: ResMut<Assets<Image>>,
    profiler: Res<Profiler>,
    watch: Res<Watch>,
    theme: Themed,
) {
    for (mut chart, image, node) in &mut charts {
        // Drawn a pixel to a logical pixel: the bars stay sharp
        // scaled up, at a quarter of the upload on a 2x screen.
        let size =
            (node.size() * node.inverse_scale_factor()).as_uvec2();
        if size.x == 0 || size.y == 0 {
            continue;
        }
        let slots = (size.x / NARROWEST).clamp(1, KEPT as u32);
        // Copied out, so the lock that every span takes is held
        // only as long as that.
        let bars = profiler
            .lock()
            .frames()
            .rev()
            .take(slots as usize)
            .map(Bars::of)
            .collect::<Vec<_>>();
        let filled = bars
            .iter()
            .take(8)
            .flat_map(|bars| bars.times)
            .filter(Option::is_some)
            .count();
        let drawn = Drawn {
            size,
            shown: watch.shown,
            newest: bars.first().map(|bars| bars.number),
            filled,
            slots,
            first: (
                slots - bars.len() as u32,
                bars.last().map_or(0, |bars| bars.number),
            ),
        };
        if chart.drawn == Some(drawn) {
            continue;
        }
        chart.drawn = Some(drawn);
        let palette = &theme.palette;
        let colours = Colours {
            bars: [palette.gold_2, palette.agent, palette.green],
            lines: [palette.gold_5, palette.red],
            shown: palette.border,
        };
        let pixels = paint(size, slots, &bars, watch.shown, &colours);
        if let Some(mut image) = images.get_mut(&image.image) {
            *image = canvas(size, pixels);
        }
    }
}

/// A frame's main thread, render thread and GPU times.
struct Bars {
    number: u64,
    times: [Option<Duration>; 3],
}

impl Bars {
    fn of(frame: &Frame) -> Self {
        Self {
            number: frame.number,
            times: [
                frame.main.as_ref().map(|timing| timing.total),
                frame.render.as_ref().map(|timing| timing.total),
                frame.gpu,
            ],
        }
    }
}

struct Colours {
    /// The main thread's, the render thread's and the GPU's.
    bars: [Color; 3],
    lines: [Color; 2],
    /// Behind the frame shown.
    shown: Color,
}

/// The chart's pixels, with `frames`, the newest first, ending at the
/// right edge.
fn paint(
    size: UVec2,
    slots: u32,
    frames: &[Bars],
    shown: Option<u64>,
    colours: &Colours,
) -> Vec<u8> {
    let (width, height) = (size.x, size.y);
    let mut pixels = vec![0; (width * height * 4) as usize];
    let mut fill = |x: (u32, u32), y: (u32, u32), colour: Color| {
        let rgba = colour.to_srgba().to_u8_array();
        for row in y.0..y.1.min(height) {
            for column in x.0..x.1.min(width) {
                let at = ((row * width + column) * 4) as usize;
                pixels[at..at + 4].copy_from_slice(&rgba);
            }
        }
    };
    let top = frames
        .iter()
        .flat_map(|frame| frame.times)
        .flatten()
        .max()
        .unwrap_or_default()
        .max(LEAST_TOP);
    let rise = |took: Duration| {
        let share = took.as_secs_f32() / top.as_secs_f32();
        (share * height as f32).round() as u32
    };
    let edge = |slot: u32| slot * width / slots;
    for (back, frame) in frames.iter().enumerate() {
        let slot = slots - 1 - back as u32;
        let (left, right) = (edge(slot), edge(slot + 1));
        if shown == Some(frame.number) {
            fill((left, right), (0, height), colours.shown);
        }
        // A pixel apart from the next frame when there is room.
        let span = right - left - u32::from(right - left > NARROWEST);
        for (at, (took, colour)) in
            frame.times.into_iter().zip(colours.bars).enumerate()
        {
            let Some(took) = took else {
                continue;
            };
            let at = at as u32;
            let x =
                (left + span * at / 3, left + span * (at + 1) / 3);
            let tall = rise(took).max(1).min(height);
            fill(x, (height - tall, height), colour);
        }
    }
    for (line, colour) in LINES.into_iter().zip(colours.lines) {
        if line < top {
            let y = height - rise(line).min(height);
            fill((0, width), (y, y + 1), colour);
        }
    }
    pixels
}
