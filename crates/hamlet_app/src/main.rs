//! The Hamlet window, connected to an agent server.

use std::path::PathBuf;

use anyhow::{Result, bail};
use bevy::DefaultPlugins;
use bevy::app::PluginGroup;
use bevy_app::{App, AppExit};
use hamlet_client::connection::LocalServer;
use hamlet_client::{ClientPlugin, Connection, Tokio, ViewFolder};
use hamlet_desktop::DesktopPlugin;
use hamlet_protocol::client::discovery::{self, Chosen};
use hamlet_realm::RealmPlugin;
use hamlet_realm::chief::Kept;
use hamlet_ui::{Remembered, UiPlugin, window_plugin};

const USAGE: &str = "\
usage: hamlet_app [-h]

Open a window on an agent server. Closing it leaves agents working.
Start a server with hamlet server; chat in the terminal with hamlet \
                     chat.

  -h, --help   show this usage

env: HAMLET_SERVER and HAMLET_TOKEN select a server;
     otherwise HAMLET_HOME (~/.hamlet) locates the local server.
     HAMLET_VIEW selects the window's layout, size and look folder;
     otherwise the OS's config folder for Hamlet is used.";

/// Whether `args` open the window, rather than ask for its usage.
fn parse(mut args: impl Iterator<Item = String>) -> Result<bool> {
    match args.next().as_deref() {
        None => Ok(true),
        Some("-h" | "--help") => Ok(false),
        Some(other) => bail!("unknown argument `{other}`\n\n{USAGE}"),
    }
}

fn view_dir(
    os: &str,
    vars: impl Fn(&str) -> Option<std::ffi::OsString>,
) -> Result<PathBuf> {
    if let Some(path) = vars("HAMLET_VIEW") {
        return Ok(PathBuf::from(path));
    }
    let path = match os {
        "macos" => vars("HOME").map(|home| {
            PathBuf::from(home)
                .join("Library/Application Support/Hamlet")
        }),
        "windows" => vars("APPDATA")
            .map(|data| PathBuf::from(data).join("Hamlet")),
        _ => vars("XDG_CONFIG_HOME")
            .map(|config| PathBuf::from(config).join("hamlet"))
            .or_else(|| {
                vars("HOME").map(|home| {
                    PathBuf::from(home).join(".config/hamlet")
                })
            }),
    };
    path.ok_or_else(|| {
        anyhow::anyhow!("No config folder found. Set HAMLET_VIEW.")
    })
}

#[expect(clippy::print_stdout, reason = "--help prints the usage")]
fn main() -> Result<()> {
    if !parse(std::env::args().skip(1))? {
        println!("{USAGE}");
        return Ok(());
    }

    let view =
        view_dir(std::env::consts::OS, |key| std::env::var_os(key))?;
    let window = window_plugin(&view)?;
    let remembered = Remembered::read(&view)?;
    let kept = Kept::read(&view)?;
    let rt = tokio::runtime::Runtime::new()?;
    let mut app = App::new();
    app.insert_resource(Tokio(rt.handle().clone()))
        .insert_resource(ViewFolder(view))
        .insert_resource(remembered)
        .insert_resource(kept)
        .add_plugins(DefaultPlugins.set(window));
    client(&mut app, |key| std::env::var(key).ok());
    let exit = app
        .add_plugins((
            RealmPlugin,
            UiPlugin,
            DesktopPlugin { window: true },
        ))
        .run();
    rt.shutdown_background();
    if let AppExit::Error(code) = exit {
        bail!("the window exited with code {code}");
    }
    Ok(())
}

fn client(app: &mut App, vars: impl Fn(&str) -> Option<String>) {
    match discovery::choose(vars) {
        Ok(Chosen::At(server)) => {
            app.add_plugins(ClientPlugin {
                server: Some(server),
            });
        }
        Ok(Chosen::Home(home)) => {
            app.add_plugins(ClientPlugin { server: None })
                .insert_resource(LocalServer(home));
        }
        Err(error) => {
            app.add_plugins(ClientPlugin { server: None })
                .insert_resource(Connection {
                    words: error.to_string(),
                    ..Connection::default()
                });
        }
    }
}
