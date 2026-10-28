//! Every cursor on one sheet, written without a window
//! (docs/cursors.md).
//!
//! `cargo run --example cursor_sheet`
//! To another file: `cargo run --example cursor_sheet -- <file>`

#![expect(
    clippy::print_stdout,
    reason = "it prints where it wrote the sheet"
)]

use std::path::PathBuf;

use bevy::asset::RenderAssetUsages;
use bevy::color::{Color, ColorToPacked, Srgba};
use bevy::image::Image;
use bevy::render::render_resource::{
    Extent3d, TextureDimension, TextureFormat,
};
use hamlet_app::cursor::{Inks, Mode, face, frames, hot, inks, side};
use hamlet_app::theme::HamletTheme;
use hamlet_assets::Tuning;
use hamlet_realm::palette::Palette;

/// The colours of the sheet.
struct Paper {
    panel: [u8; 4],
    grass: [u8; 4],
    seam: [u8; 4],
    ink: [u8; 4],
    dim: [u8; 4],
    /// The mark on the hot spot.
    mark: [u8; 4],
    inks: Inks,
}

struct Sheet {
    width: usize,
    height: usize,
    data: Vec<u8>,
}

impl Sheet {
    fn new(width: usize, height: usize, ground: [u8; 4]) -> Self {
        let data = ground.repeat(width * height);
        Sheet {
            width,
            height,
            data,
        }
    }

    /// Lays `colour` over the pixel at `x`, `y` as far as its alpha
    /// says.
    fn dot(&mut self, x: usize, y: usize, colour: [u8; 4]) {
        if x >= self.width || y >= self.height {
            return;
        }
        let at = (y * self.width + x) * 4;
        let cover = colour[3] as u32;
        let under = &mut self.data[at..at + 3];
        for (under, over) in under.iter_mut().zip(colour) {
            let mixed =
                over as u32 * cover + *under as u32 * (255 - cover);
            *under = (mixed / 255) as u8;
        }
    }

    fn fill(
        &mut self,
        (x, y): (usize, usize),
        (wide, high): (usize, usize),
        colour: [u8; 4],
    ) {
        for row in y..y + high {
            for column in x..x + wide {
                self.dot(column, row, colour);
            }
        }
    }

    /// The outline of a box, one pixel thick.
    fn frame(
        &mut self,
        (x, y): (usize, usize),
        (wide, high): (usize, usize),
        colour: [u8; 4],
    ) {
        self.fill((x, y), (wide, 1), colour);
        self.fill((x, y + high - 1), (wide, 1), colour);
        self.fill((x, y), (1, high), colour);
        self.fill((x + wide - 1, y), (1, high), colour);
    }

    /// A cursor with its corner at `x`, `y`, each of its pixels
    /// `scale` across.
    fn cursor(
        &mut self,
        image: &Image,
        (x, y): (usize, usize),
        scale: usize,
    ) {
        let Some(data) = image.data.as_ref() else {
            return;
        };
        for (i, pixel) in data.chunks(4).enumerate() {
            let (column, row) = (i % side(), i / side());
            let colour = [pixel[0], pixel[1], pixel[2], pixel[3]];
            self.fill(
                (x + column * scale, y + row * scale),
                (scale, scale),
                colour,
            );
        }
    }

    /// A cursor in the middle of a box of `ground`.
    fn swatch(
        &mut self,
        image: &Image,
        at: (usize, usize),
        size: (usize, usize),
        scale: usize,
        ground: [u8; 4],
        seam: [u8; 4],
    ) {
        self.fill(at, size, ground);
        self.frame(at, size, seam);
        let across = side() * scale;
        let corner = (
            at.0 + (size.0 - across) / 2,
            at.1 + (size.1 - across) / 2,
        );
        self.cursor(image, corner, scale);
    }

    /// Words in capitals five pixels by seven, each pixel `scale`
    /// across.
    fn words(
        &mut self,
        text: &str,
        (x, y): (usize, usize),
        scale: usize,
        colour: [u8; 4],
    ) {
        for (i, letter) in text.chars().enumerate() {
            let rows = glyph(letter.to_ascii_uppercase());
            for (row, bits) in rows.into_iter().enumerate() {
                for column in 0..5 {
                    if bits & (0b10000 >> column) != 0 {
                        self.fill(
                            (
                                x + (i * 6 + column) * scale,
                                y + row * scale,
                            ),
                            (scale, scale),
                            colour,
                        );
                    }
                }
            }
        }
    }

    fn image(self) -> Image {
        Image::new(
            Extent3d {
                width: self.width as u32,
                height: self.height as u32,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            self.data,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        )
    }
}

/// The rows of a letter, its left pixel in the highest of five bits.
/// Anything unknown is a space.
fn glyph(letter: char) -> [u8; 7] {
    match letter {
        'A' => [14, 17, 17, 31, 17, 17, 17],
        'B' => [30, 17, 17, 30, 17, 17, 30],
        'C' => [14, 17, 16, 16, 16, 17, 14],
        'D' => [30, 17, 17, 17, 17, 17, 30],
        'E' => [31, 16, 16, 30, 16, 16, 31],
        'F' => [31, 16, 16, 30, 16, 16, 16],
        'G' => [14, 17, 16, 23, 17, 17, 15],
        'H' => [17, 17, 17, 31, 17, 17, 17],
        'I' => [14, 4, 4, 4, 4, 4, 14],
        'J' => [7, 2, 2, 2, 2, 18, 12],
        'K' => [17, 18, 20, 24, 20, 18, 17],
        'L' => [16, 16, 16, 16, 16, 16, 31],
        'M' => [17, 27, 21, 21, 17, 17, 17],
        'N' => [17, 25, 21, 19, 17, 17, 17],
        'O' => [14, 17, 17, 17, 17, 17, 14],
        'P' => [30, 17, 17, 30, 16, 16, 16],
        'Q' => [14, 17, 17, 17, 21, 18, 13],
        'R' => [30, 17, 17, 30, 20, 18, 17],
        'S' => [15, 16, 16, 14, 1, 1, 30],
        'T' => [31, 4, 4, 4, 4, 4, 4],
        'U' => [17, 17, 17, 17, 17, 17, 14],
        'V' => [17, 17, 17, 17, 17, 10, 4],
        'W' => [17, 17, 17, 21, 21, 27, 17],
        'X' => [17, 17, 10, 4, 10, 17, 17],
        'Y' => [17, 17, 10, 4, 4, 4, 4],
        'Z' => [31, 1, 2, 4, 8, 16, 31],
        '0' => [14, 17, 19, 21, 25, 17, 14],
        '1' => [4, 12, 4, 4, 4, 4, 14],
        '2' => [14, 17, 1, 2, 4, 8, 31],
        '3' => [30, 1, 1, 14, 1, 1, 30],
        '4' => [2, 6, 10, 18, 31, 2, 2],
        '5' => [31, 16, 30, 1, 1, 17, 14],
        '6' => [6, 8, 16, 30, 17, 17, 14],
        '7' => [31, 1, 2, 4, 8, 8, 8],
        '8' => [14, 17, 17, 14, 17, 17, 14],
        '9' => [14, 17, 17, 15, 1, 2, 12],
        ',' => [0, 0, 0, 0, 4, 4, 8],
        '-' => [0, 0, 0, 31, 0, 0, 0],
        _ => [0; 7],
    }
}

fn bytes(colour: Color) -> [u8; 4] {
    Srgba::from(colour).to_u8_array()
}

/// One mode: its name and hot spot, the cursor eight times its size
/// with the hot spot marked, and beside it at its own size and at
/// twice that, on the panel and on grass.
fn cell(
    sheet: &mut Sheet,
    mode: Mode,
    (x, y): (usize, usize),
    paper: &Paper,
) {
    let image = face(mode, 0, &paper.inks);
    let (hx, hy) = hot(mode);
    let name = format!("{mode:?}");
    sheet.words(&name, (x, y), 2, paper.ink);
    sheet.words(
        &format!("hot {hx},{hy}"),
        (x + 232, y),
        2,
        paper.dim,
    );

    let top = y + 22;
    sheet.swatch(
        &image,
        (x, top),
        (256, 256),
        8,
        paper.panel,
        paper.seam,
    );
    let hot = (x + hx as usize * 8, top + hy as usize * 8);
    sheet.frame((hot.0 - 1, hot.1 - 1), (10, 10), paper.mark);
    // Place of the hot spot along each side.
    sheet.fill((hot.0 + 3, top), (2, 8), paper.mark);
    sheet.fill((x, hot.1 + 3), (8, 2), paper.mark);

    let beside = x + 264;
    let swatches = [
        (0, 44, 1, paper.panel),
        (49, 44, 1, paper.grass),
        (98, 76, 2, paper.panel),
        (180, 76, 2, paper.grass),
    ];
    for (down, high, scale, ground) in swatches {
        sheet.swatch(
            &image,
            (beside, top + down),
            (76, high),
            scale,
            ground,
            paper.seam,
        );
    }
}

/// The pictures the waiting cursor goes through, in order.
fn turning(sheet: &mut Sheet, (x, y): (usize, usize), paper: &Paper) {
    sheet.words("Wait, as it turns", (x, y), 2, paper.ink);
    let top = y + 22;
    for frame in 0..frames(Mode::Wait) {
        let image = face(Mode::Wait, frame, &paper.inks);
        let (column, row) = (frame % 4, frame / 4);
        sheet.swatch(
            &image,
            (x + column * 85, top + row * 85),
            (85, 85),
            2,
            paper.panel,
            paper.seam,
        );
        let ground = if row == 0 { paper.panel } else { paper.grass };
        sheet.swatch(
            &image,
            (x + column * 85, top + 176 + row * 40),
            (85, 40),
            1,
            ground,
            paper.seam,
        );
    }
}

fn main() {
    let out = std::env::args().nth(1).map_or_else(
        || PathBuf::from("examples/cursor_sheet/out/sheet.png"),
        PathBuf::from,
    );
    let sun = Palette::default().grass_sun;
    let theme = HamletTheme::designed();
    let p = &theme.palette;
    let paper = Paper {
        panel: bytes(p.surface),
        grass: bytes(Color::linear_rgb(sun.x, sun.y, sun.z)),
        seam: bytes(p.gold_6),
        ink: bytes(p.gold_1),
        dim: bytes(p.grey_1),
        mark: bytes(Color::srgb(1.0, 0.2, 0.9)),
        inks: inks(&theme),
    };
    let (wide, high, gap) = (340, 278, 20);
    let modes = Mode::all();
    let rows = (modes.len() + 1).div_ceil(4);
    let mut sheet = Sheet::new(
        gap + 4 * (wide + gap),
        gap + rows * (high + gap),
        paper.panel,
    );
    let place = |i: usize| {
        (gap + i % 4 * (wide + gap), gap + i / 4 * (high + gap))
    };
    for (i, mode) in modes.iter().enumerate() {
        cell(&mut sheet, *mode, place(i), &paper);
    }
    turning(&mut sheet, place(modes.len()), &paper);

    if let Some(folder) = out.parent() {
        std::fs::create_dir_all(folder).expect("the folder");
    }
    sheet
        .image()
        .try_into_dynamic()
        .expect("an image of eight bits a channel")
        .save(&out)
        .expect("the sheet written");
    println!("{}", out.display());
}
