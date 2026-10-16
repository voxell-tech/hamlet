//! Bakes the official assets for hamlet-server: their files, their
//! preview pictures and the catalogue `pnpm assets:publish` reads.
//!
//! cargo run -p hamlet_assets_src --release -- [--out DIR] [--only
//! ID,ID] [--no-previews]

use std::path::PathBuf;

use anyhow::{Context, Result};
use hamlet_assets_src::catalog::{ALL, FRAMES};
use hamlet_assets_src::file;
use hamlet_assets_src::previews::{self, Job};

/// The argument after `name` on the command line.
fn word(name: &str) -> Option<String> {
    let args = std::env::args().collect::<Vec<_>>();
    let at = args.iter().position(|a| a == name)?;
    args.get(at + 1).cloned()
}

fn main() -> Result<()> {
    let out = word("--out").map_or_else(
        || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("out"),
        PathBuf::from,
    );
    let only = word("--only").map(|ids| {
        ids.split(',').map(str::to_string).collect::<Vec<_>>()
    });
    let previews = !std::env::args().any(|a| a == "--no-previews");
    std::fs::create_dir_all(out.join("files"))?;
    let mut jobs = Vec::new();
    for entry in ALL {
        let file = file(&entry)?;
        std::fs::write(out.join("files").join(entry.file()), &file)
            .with_context(|| format!("writing {}", entry.id))?;
        let wanted = only
            .as_ref()
            .is_none_or(|only| only.iter().any(|id| id == entry.id));
        if previews && wanted {
            jobs.push(Job {
                entry,
                file,
                out: out.join("previews").join(entry.id),
            });
        }
    }
    if !jobs.is_empty() {
        previews::render(jobs, FRAMES);
    }
    // Each asset lists the pictures it has, drawn now or before.
    let listed = ALL
        .iter()
        .map(|entry| {
            let dir = out.join("previews").join(entry.id);
            let frames = (0..FRAMES)
                .take_while(|n| {
                    dir.join(format!("{n}.png")).is_file()
                })
                .count() as u32;
            entry.listed(frames)
        })
        .collect::<Vec<_>>();
    std::fs::write(
        out.join("catalog.json"),
        serde_json::to_vec_pretty(&listed)?,
    )?;
    eprintln!("baked {} assets into {}", ALL.len(), out.display());
    Ok(())
}
