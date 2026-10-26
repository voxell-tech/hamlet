//! Stills of the panels and their controls, drawn without a window
//! (docs/ui-warmth.md). The scenes are in `scenes.rs`.
//!
//! Every still: `cargo run --example ui_gallery`
//! Into a folder: `cargo run --example ui_gallery -- --out <dir>`
//! One scene: `cargo run --example ui_gallery -- --scene controls`
//! Sooner, to catch what fades: the same with `--strike 12`
//! Another size, or another picture of the world, shown whole:
//! `--size 1600x900 --world <png>`

#[path = "../common/offscreen.rs"]
mod offscreen;
mod scenes;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use hamlet_app::account::Page;
use hamlet_app::bazaar::Stall;
use hamlet_app::pictures::Pictures;
use hamlet_app::welcome::Step;

use self::offscreen::{Target, word};
use self::scenes::{GalleryPlugin, Scene, Stand, Strike, Trade};

/// A still of the world, shown where the world is drawn.
const WORLD: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/examples/ui_gallery/world.png"
);

/// One still: its name, its scene, its size in the interface's
/// pixels and the pixels to one of them.
struct Shot {
    name: &'static str,
    scene: Scene,
    size: UVec2,
    scale: f32,
}

/// A still of the Agent Bazaar.
const fn bazaar(name: &'static str, scene: Scene) -> Shot {
    Shot {
        name,
        scene,
        size: UVec2::new(1440, 1000),
        scale: 1.0,
    }
}

/// A still of the first-run screen at a page of a Hamlet account.
const fn account(name: &'static str, page: Page) -> Shot {
    Shot {
        name,
        scene: Scene::Account(page),
        size: UVec2::new(1180, 760),
        scale: 1.0,
    }
}

/// A still of the Agent Bazaar with its server answering as `trade`
/// says.
const fn market(
    name: &'static str,
    stall: Stall,
    trade: Trade,
    words: &'static [&'static str],
) -> Shot {
    Shot {
        name,
        scene: Scene::Market(stall, trade, words),
        size: UVec2::new(1440, 1100),
        scale: 1.0,
    }
}

const SHOTS: [Shot; 71] = [
    bazaar("bazaar-models", Scene::Stall(Stall::Models, &[])),
    bazaar(
        "bazaar-model",
        Scene::Stall(Stall::Models, &["Claude Sonnet 4.5"]),
    ),
    bazaar("bazaar-skills", Scene::Stall(Stall::Skills, &[])),
    bazaar(
        "bazaar-skill",
        Scene::Stall(Stall::Skills, &["vercel-react-best-practices"]),
    ),
    bazaar("bazaar-plugins", Scene::Stall(Stall::Plugins, &[])),
    bazaar(
        "bazaar-plugin",
        Scene::Stall(Stall::Plugins, &["Excalidraw Whiteboard"]),
    ),
    bazaar("bazaar-agents", Scene::Stall(Stall::Community, &[])),
    bazaar(
        "bazaar-agent",
        Scene::Stall(Stall::Community, &["Research Assistant"]),
    ),
    bazaar("bazaar-hamlets", Scene::Stall(Stall::Hamlets, &[])),
    bazaar(
        "bazaar-hamlet",
        Scene::Stall(Stall::Hamlets, &["Open Source Maintainer Kit"]),
    ),
    market("market-agents", Stall::Community, Trade::SignedIn, &[]),
    market(
        "market-agents-signed-out",
        Stall::Community,
        Trade::SignedOut,
        &[],
    ),
    market(
        "market-agent-buy",
        Stall::Community,
        Trade::SignedOut,
        &["Wick Lantern"],
    ),
    market(
        "market-agent-signed-in",
        Stall::Community,
        Trade::SignedIn,
        &["Wick Lantern"],
    ),
    market(
        "market-agent-paying",
        Stall::Community,
        Trade::Paying,
        &["Wick Lantern"],
    ),
    market(
        "market-agent-installing",
        Stall::Community,
        Trade::Installing,
        &["Wick Lantern"],
    ),
    market(
        "market-agent-update",
        Stall::Community,
        Trade::SignedIn,
        &["Letter Scribe"],
    ),
    market(
        "market-tools-owned",
        Stall::Plugins,
        Trade::SignedIn,
        &["Cal Clock"],
    ),
    market(
        "market-hamlet",
        Stall::Hamlets,
        Trade::SignedIn,
        &["Small Press"],
    ),
    market("market-sign-in", Stall::Community, Trade::SignIn, &[]),
    market(
        "market-seller-terms",
        Stall::Community,
        Trade::Terms,
        &[],
    ),
    market("looks", Stall::Assets, Trade::SignedOut, &[]),
    market(
        "looks-asset",
        Stall::Assets,
        Trade::SignedOut,
        &["Sakura"],
    ),
    market(
        "looks-asset-bigger",
        Stall::Assets,
        Trade::SignedOut,
        &["Sakura", "BIGGER"],
    ),
    market(
        "looks-asset-emote",
        Stall::Assets,
        Trade::SignedOut,
        &["Wave"],
    ),
    market(
        "looks-asset-mesh",
        Stall::Assets,
        Trade::SignedOut,
        &["Minka"],
    ),
    market(
        "looks-asset-owned",
        Stall::Assets,
        Trade::SignedIn,
        &["Sakura"],
    ),
    market(
        "looks-fitting",
        Stall::Assets,
        Trade::SignedIn,
        &["FITTING"],
    ),
    market(
        "looks-fitting-one",
        Stall::Assets,
        Trade::SignedIn,
        &["FITTING", "FOUNDER"],
    ),
    market(
        "market-desk",
        Stall::Community,
        Trade::SignedIn,
        &["SELL AN AGENT"],
    ),
    market(
        "market-yours",
        Stall::Community,
        Trade::SignedIn,
        &["YOURS"],
    ),
    market(
        "market-desk-change",
        Stall::Community,
        Trade::SignedIn,
        &["YOURS", "Tutor Owl"],
    ),
    Shot {
        name: "menu",
        scene: Scene::Menu,
        size: UVec2::new(1440, 860),
        scale: 1.0,
    },
    Shot {
        name: "view-menu",
        scene: Scene::ViewMenu,
        size: UVec2::new(1440, 860),
        scale: 1.0,
    },
    Shot {
        name: "icons",
        scene: Scene::Icons,
        size: UVec2::new(1200, 760),
        scale: 1.0,
    },
    Shot {
        name: "settings",
        scene: Scene::Settings,
        size: UVec2::new(1440, 1100),
        scale: 1.0,
    },
    Shot {
        name: "chief",
        scene: Scene::Chief,
        size: UVec2::new(1440, 860),
        scale: 1.0,
    },
    Shot {
        name: "history",
        scene: Scene::History,
        size: UVec2::new(1440, 860),
        scale: 1.0,
    },
    Shot {
        name: "reading",
        scene: Scene::Reading,
        size: UVec2::new(1440, 860),
        scale: 1.0,
    },
    Shot {
        name: "overview",
        scene: Scene::Overview,
        size: UVec2::new(1440, 860),
        scale: 1.0,
    },
    Shot {
        name: "hamlet",
        scene: Scene::Hamlet,
        size: UVec2::new(1440, 860),
        scale: 1.0,
    },
    Shot {
        name: "bylaws",
        scene: Scene::Bylaws,
        size: UVec2::new(1440, 1500),
        scale: 1.0,
    },
    Shot {
        name: "chat",
        scene: Scene::Chat,
        size: UVec2::new(1440, 860),
        scale: 1.0,
    },
    Shot {
        name: "group",
        scene: Scene::Group,
        size: UVec2::new(1440, 1500),
        scale: 1.0,
    },
    Shot {
        name: "links",
        scene: Scene::Links,
        size: UVec2::new(1440, 860),
        scale: 1.0,
    },
    Shot {
        name: "edits",
        scene: Scene::Edits,
        size: UVec2::new(1440, 1500),
        scale: 1.0,
    },
    Shot {
        name: "pictures",
        scene: Scene::Pictures,
        size: UVec2::new(1440, 1560),
        scale: 1.0,
    },
    Shot {
        name: "rewind",
        scene: Scene::Rewind,
        size: UVec2::new(1440, 860),
        scale: 1.0,
    },
    Shot {
        name: "quiet",
        scene: Scene::Quiet,
        size: UVec2::new(1440, 860),
        scale: 1.0,
    },
    Shot {
        name: "notes",
        scene: Scene::Notes,
        size: UVec2::new(1440, 860),
        scale: 1.0,
    },
    Shot {
        name: "character",
        scene: Scene::Character,
        size: UVec2::new(1440, 860),
        scale: 1.0,
    },
    Shot {
        name: "prompt",
        scene: Scene::Prompt,
        size: UVec2::new(1440, 860),
        scale: 1.0,
    },
    Shot {
        name: "charter",
        scene: Scene::Charter,
        size: UVec2::new(1440, 1100),
        scale: 1.0,
    },
    Shot {
        name: "sight",
        scene: Scene::Sight,
        size: UVec2::new(1440, 1100),
        scale: 1.0,
    },
    Shot {
        name: "workspace",
        scene: Scene::Workspace,
        size: UVec2::new(1440, 1100),
        scale: 1.0,
    },
    Shot {
        name: "controls",
        scene: Scene::Controls,
        size: UVec2::new(900, 800),
        scale: 2.0,
    },
    Shot {
        name: "panels",
        scene: Scene::Panels,
        size: UVec2::new(1440, 860),
        scale: 1.0,
    },
    Shot {
        name: "waiting",
        scene: Scene::Waiting,
        size: UVec2::new(1440, 860),
        scale: 1.0,
    },
    Shot {
        name: "field",
        scene: Scene::Field,
        size: UVec2::new(1440, 860),
        scale: 1.0,
    },
    Shot {
        name: "welcome",
        scene: Scene::Welcome(Step::Choice),
        size: UVec2::new(1180, 680),
        scale: 1.0,
    },
    Shot {
        name: "welcome-spend-limit",
        scene: Scene::Welcome(Step::Limit),
        size: UVec2::new(1180, 680),
        scale: 1.0,
    },
    Shot {
        name: "welcome-account",
        scene: Scene::Welcome(Step::Account),
        size: UVec2::new(1180, 680),
        scale: 1.0,
    },
    account("account-sign-up", Page::SignUp),
    account("account-code", Page::Code),
    account("account-username", Page::Username),
    account("account-sign-in", Page::SignIn),
    account("account-forgot", Page::Reset),
    Shot {
        name: "settings-account",
        scene: Scene::AccountSettings,
        size: UVec2::new(1440, 1100),
        scale: 1.0,
    },
    Shot {
        name: "welcome-key",
        scene: Scene::Welcome(Step::Key),
        size: UVec2::new(1180, 680),
        scale: 1.0,
    },
    Shot {
        name: "welcome-local",
        scene: Scene::Welcome(Step::Local),
        size: UVec2::new(1180, 680),
        scale: 1.0,
    },
    Shot {
        name: "welcome-models",
        scene: Scene::Welcome(Step::Models),
        size: UVec2::new(1180, 680),
        scale: 1.0,
    },
];

/// The update the done buttons are pressed on unless `--strike` says,
/// the updates after it the still is taken, and the updates its file
/// is given to be written.
const STRIKE: u32 = 240;
const AFTER: u32 = 7;
const WRITE: u32 = 60;

#[derive(Resource)]
struct Output(PathBuf);

fn main() {
    let out = word("--out").map(PathBuf::from).unwrap_or_else(|| {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("examples/ui_gallery/out")
    });
    std::fs::create_dir_all(&out).expect("output folder");
    let Some(scene) = word("--scene") else {
        // A scene to a process: each starts from nothing.
        let me = std::env::current_exe().expect("this program");
        for shot in &SHOTS {
            let status = Command::new(&me)
                .args(["--scene", shot.name, "--out"])
                .arg(&out)
                .status()
                .expect("run a scene");
            assert!(status.success(), "{} failed", shot.name);
        }
        return;
    };
    let shot = SHOTS
        .iter()
        .find(|shot| shot.name == scene)
        .expect("a scene named in SHOTS");
    draw(shot, out.join(format!("{}.png", shot.name)));
}

fn draw(shot: &Shot, path: PathBuf) {
    let size = word("--size")
        .and_then(|size| {
            let (wide, tall) = size.split_once('x')?;
            Some(UVec2::new(wide.parse().ok()?, tall.parse().ok()?))
        })
        .unwrap_or(shot.size);
    let mut app = App::new();
    // Before the asset plugin: the looks viewed are read from memory.
    hamlet_realm::dress::register_source(&mut app);
    app.add_plugins(offscreen::plugins(size));
    let pixels = (size.as_vec2() * shot.scale).as_uvec2();
    Target::insert(&mut app, pixels, shot.scale);
    let own = word("--world");
    let from = own.clone().unwrap_or_else(|| WORLD.to_string());
    let world = std::fs::read(from).ok().and_then(|bytes| {
        Image::from_buffer(
            &bytes,
            ImageType::Extension("png"),
            CompressedImageFormats::NONE,
            true,
            ImageSampler::linear(),
            default(),
        )
        .ok()
    });
    let mut images = app.world_mut().resource_mut::<Assets<Image>>();
    let world = world.map(|world| images.add(world));
    if let Some(world) = world {
        let whole = images.get(&world).map(Image::size_f32);
        let part = match (own, whole) {
            (Some(_), Some(whole)) => {
                Rect::from_corners(Vec2::ZERO, whole)
            }
            // The house and its lantern, wider than tall.
            _ => Rect::new(0.0, 150.0, 720.0, 850.0),
        };
        app.insert_resource(Stand(world, part));
    }
    app.insert_resource(Output(path))
        .insert_resource(Strike(
            word("--strike")
                .and_then(|at| at.parse::<u32>().ok())
                .unwrap_or(STRIKE),
        ))
        .insert_resource(TimeUpdateStrategy::ManualDuration(
            Duration::from_secs_f64(1.0 / 60.0),
        ))
        .add_plugins(GalleryPlugin(shot.scene))
        .add_systems(Startup, camera)
        .add_systems(Update, capture);
    app.run();
}

fn camera(mut commands: Commands, target: Res<Target>) {
    commands.spawn((Camera2d, target.drawn(), IsDefaultUiCamera));
}

/// Takes the still a moment after the strike, and leaves once it is
/// written. An update on which a picture is still being read is not
/// counted.
fn capture(
    mut commands: Commands,
    target: Res<Target>,
    out: Res<Output>,
    strike: Res<Strike>,
    pictures: Res<Pictures>,
    mut update: Local<u32>,
    mut exit: MessageWriter<AppExit>,
) {
    if pictures.is_waiting() {
        return;
    }
    *update += 1;
    if *update == strike.0 + AFTER {
        target.shoot(&mut commands, out.0.clone());
    }
    if *update > strike.0 + AFTER + WRITE {
        exit.write(AppExit::Success);
    }
}
