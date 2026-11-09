//! Writes the contrast report of every shipped theme, one Markdown
//! file a variant and a README, into the folder given, else
//! `target/contrast`.

use std::fmt::Write as _;
use std::path::PathBuf;
use std::{env, fs, io};

use bevy::window::WindowTheme;
use hamlet_assets::Tuning;
use hamlet_ui::theme::{Measures, Themes};

fn main() -> io::Result<()> {
    let folder = env::args_os().nth(1).map_or_else(
        || {
            PathBuf::from(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../target/contrast"
            ))
        },
        PathBuf::from,
    );
    fs::create_dir_all(&folder)?;
    let measures = Measures::designed();
    let mut readme =
        String::from("# Contrast of the shipped themes\n\n");
    for theme in Themes::shipped().iter() {
        for (appearance, report) in theme.reports(&measures) {
            let mode = match appearance {
                WindowTheme::Light => "light",
                WindowTheme::Dark => "dark",
            };
            let file = format!("{}-{mode}.md", theme.id);
            fs::write(folder.join(&file), report.to_string())?;
            let _ = writeln!(
                readme,
                "- [{}: {}]({file}): {}",
                theme.name,
                theme.variants.get(appearance).name,
                report.summary()
            );
        }
    }
    fs::write(folder.join("README.md"), readme)
}
