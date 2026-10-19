//! The Hamlet window, connected to an agent server.

use std::path::PathBuf;

use anyhow::{Result, bail};
use bevy::DefaultPlugins;
use bevy::app::PluginGroup;
use bevy_app::{App, AppExit};
use hamlet_client::{
    ClientPlugin, Connection, ServerAddress, Tokio, ViewDir,
};
use hamlet_desktop::DesktopPlugin;
use hamlet_realm::RealmPlugin;
use hamlet_ui::{UiPlugin, window_plugin};

const USAGE: &str = "\
usage: hamlet-app [-h]

Open a window on an agent server. Closing it leaves agents working.
Start a server with hamlet server; chat in the terminal with hamlet \
                     chat.

  -h, --help   show this usage

env: HAMLET_SERVER and HAMLET_TOKEN select a server;
     otherwise HAMLET_HOME (~/.hamlet) locates the local server.
     HAMLET_VIEW selects the window's layout, size and look folder;
     otherwise the OS's config folder for Hamlet is used.";

fn parse(
    mut args: impl Iterator<Item = String>,
) -> Result<Option<()>> {
    if let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => return Ok(None),
            other => bail!("unknown argument `{other}`\n\n{USAGE}"),
        }
    }
    Ok(Some(()))
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

fn main() -> Result<()> {
    if parse(std::env::args().skip(1))?.is_none() {
        println!("{USAGE}");
        return Ok(());
    }

    let view =
        view_dir(std::env::consts::OS, |key| std::env::var_os(key))?;
    let rt = tokio::runtime::Runtime::new()?;
    let mut app = App::new();
    app.insert_resource(Tokio(rt.handle().clone()))
        .insert_resource(ViewDir(view.clone()))
        .add_plugins(DefaultPlugins.set(window_plugin(&view)));
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
    let (server, error) = match ServerAddress::find(&vars) {
        Ok(server) => (Some(server), None),
        Err(words) => (None, Some(words)),
    };
    app.add_plugins(ClientPlugin { server });
    if let Some(words) = error {
        if vars("HAMLET_SERVER").is_none()
            && let Ok(home) = ServerAddress::local_home(vars)
        {
            app.insert_resource(
                hamlet_client::connection::LocalServer(home),
            );
        }
        app.insert_resource(Connection {
            words,
            ..Connection::default()
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(args: &[&str]) -> Result<Option<()>> {
        parse(args.iter().map(|a| a.to_string()))
    }

    #[test]
    fn help_names_the_window_and_its_connection_and_view_settings() {
        assert_eq!(parsed(&["-h"]).unwrap(), None);
        assert_eq!(parsed(&["--help"]).unwrap(), None);
        assert!(
            USAGE.starts_with("usage: hamlet-app [-h]"),
            "{USAGE}"
        );
        for key in [
            "HAMLET_SERVER",
            "HAMLET_TOKEN",
            "HAMLET_HOME",
            "HAMLET_VIEW",
        ] {
            assert!(USAGE.contains(key), "{USAGE}");
        }
    }

    fn view(os: &str, vars: &[(&str, &str)]) -> Result<PathBuf> {
        view_dir(os, |key| {
            vars.iter()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| value.into())
        })
    }

    #[test]
    fn the_view_override_wins_on_every_platform_without_a_home() {
        for os in ["macos", "linux", "windows"] {
            assert_eq!(
                view(os, &[("HAMLET_VIEW", "/scratch/window")])
                    .unwrap(),
                PathBuf::from("/scratch/window")
            );
        }
    }

    #[test]
    fn the_view_defaults_to_each_platforms_config_folder() {
        let vars = [
            ("HOME", "/chief"),
            ("APPDATA", "/roaming"),
            ("XDG_CONFIG_HOME", "/config"),
            ("HAMLET_HOME", "/server-home"),
        ];
        for (os, expected) in [
            ("macos", "/chief/Library/Application Support/Hamlet"),
            ("linux", "/config/hamlet"),
            ("windows", "/roaming/Hamlet"),
        ] {
            assert_eq!(
                view(os, &vars).unwrap(),
                PathBuf::from(expected)
            );
        }
        assert_eq!(
            view("linux", &[("HOME", "/chief")]).unwrap(),
            PathBuf::from("/chief/.config/hamlet")
        );
        for os in ["macos", "linux", "windows"] {
            let error = view(os, &[]).unwrap_err().to_string();
            assert!(error.contains("HAMLET_VIEW"), "{error}");
        }
    }

    #[test]
    fn runtime_flags_are_unknown_and_name_the_server_commands() {
        for flag in ["--headless", "--repl", "--insecure", "--replay"]
        {
            let error = parsed(&[flag]).unwrap_err().to_string();
            assert!(
                error.contains(&format!("unknown argument `{flag}`")),
                "{error}"
            );
            assert!(error.contains("hamlet server"), "{error}");
            assert!(error.contains("hamlet chat"), "{error}");
        }
        let unknown = parsed(&["--hedless"]).unwrap_err().to_string();
        assert!(unknown.contains("`--hedless`"), "{unknown}");
        assert!(parsed(&[]).unwrap().is_some());
    }
}
