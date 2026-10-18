//! The scratch home a plugin is installed in, and the app that loads
//! it.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use bevy::input::ButtonInput;
use bevy::input::keyboard::KeyCode;
use bevy::prelude::*;
use bevy::time::{TimePlugin, TimeUpdateStrategy};
use hamlet_core::core::HomeDir;
use hamlet_home::Home;
use hamlet_sdk::capability::{Choice, Choices, Record, Store};
use hamlet_sdk::command::Notice;
use hamlet_wasm::install::{self, FILES};
use hamlet_wasm::limits::Limits;
use hamlet_wasm::manifest::Manifest;

/// The time one frame of the app lasts.
const FRAME: Duration = Duration::from_millis(50);

/// A folder under the system's temp folder, removed when it goes.
pub struct Scratch(PathBuf);

impl Scratch {
    pub fn new() -> Result<Self, String> {
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let folder = std::env::temp_dir().join(format!(
            "hamlet-harness-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&folder)
            .map_err(|e| format!("{}: {e}", folder.display()))?;
        Ok(Self(folder))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Installs the plugin in `folder` into `home` as a chief would
/// who let it in and has answered none of its asks. Returns its id.
pub fn install(
    home: &Scratch,
    folder: &Path,
) -> Result<String, String> {
    let text =
        std::fs::read_to_string(folder.join(install::MANIFEST))
            .map_err(|e| {
                format!(
                    "{}: {e}",
                    folder.join(install::MANIFEST).display()
                )
            })?;
    let manifest = Manifest::parse(&text)
        .map_err(|e| format!("manifest.toml: {}", e.message()))?;
    let plugins = home.path().join("plugins");
    let dest = plugins.join(&manifest.id);
    std::fs::create_dir_all(&dest)
        .map_err(|e| format!("{}: {e}", dest.display()))?;
    for file in FILES {
        let from = folder.join(file);
        if from.is_file() {
            std::fs::copy(&from, dest.join(file))
                .map_err(|e| format!("{}: {e}", from.display()))?;
        }
    }
    let installed =
        install::read(&dest).map_err(|r| r.to_string())?;
    let choices = installed.asks.capabilities().into_iter().fold(
        Choices::default(),
        |choices, capability| {
            choices.decided(capability, Choice::AskNextTime, false)
        },
    );
    Store::open(&plugins).set(
        &manifest.id,
        Record::installed(&manifest.version, &choices),
    );
    Ok(manifest.id)
}

/// An app that loads the home as it is. Each frame lasts [`FRAME`] by
/// its clock, and a call may last long enough for a busy machine.
pub fn app(home: &Scratch) -> App {
    let mut app = App::new();
    app.insert_resource(HomeDir(Home {
        root: home.path().to_path_buf(),
        workspace: home.path().join("workspace"),
    }))
    .insert_resource(TimeUpdateStrategy::ManualDuration(FRAME))
    .add_plugins((
        TimePlugin,
        hamlet_sdk::agents::plugin,
        hamlet_sdk::capability::plugin,
        hamlet_sdk::command::plugin,
        hamlet_sdk::context::plugin,
        hamlet_sdk::dock::plugin,
        hamlet_sdk::inspector::plugin,
        hamlet_sdk::keys::plugin,
        hamlet_sdk::menu::plugin,
        hamlet_sdk::plugin::plugin,
        hamlet_sdk::settings::plugin,
        hamlet_sdk::tree::plugin,
        hamlet_wasm::plugin,
    ))
    .init_resource::<ButtonInput<KeyCode>>()
    .init_resource::<Told>()
    .add_observer(
        |notice: On<Notice>, mut told: ResMut<Told>| {
            told.0.push(notice.0.clone());
        },
    );
    app.insert_resource(Limits::default().patient());
    app
}

/// The notices the chief was given in the chat.
#[derive(Resource, Default)]
pub struct Told(pub Vec<String>);
