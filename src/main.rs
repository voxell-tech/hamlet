//! The hamlet binary: picks a mode and runs the app.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Result, bail};
use bevy::DefaultPlugins;
use bevy::app::PluginGroup;
use bevy_app::{
    App, AppExit, ScheduleRunnerPlugin, TerminalCtrlCHandlerPlugin,
};
use hamlet_core::agents::WatchEvery;
use hamlet_core::core::CorePlugin;
use hamlet_core::oversight::{OversightPlugin, Unreviewed};
use hamlet_core::session::SessionPlugin;
use hamlet_core::terminal::TerminalPlugin;
use hamlet_desktop::DesktopPlugin;
use hamlet_home::instance::{self, Instance};
use hamlet_home::{Home, limits};
use hamlet_model::config::Config;
use hamlet_model::log;
use hamlet_plugins::Builtin;
use hamlet_realm::RealmPlugin;
use hamlet_sandbox as sandbox;
use hamlet_ui::{ShowRequests, UiPlugin, window_plugin};

const USAGE: &str = "\
usage: hamlet [--headless | --repl] [--insecure] [--replay \
                     <session.jsonl>]

  (default)    open the command center window
  --headless   no window, for machines without a display; Ctrl-C \
                     stops
  --repl       chat in the terminal (debugging)
  --insecure   run tools without a sandbox
  --replay     answer with the assistant messages recorded in a \
                     session log instead of a model

Settings live in <home>/config.toml and are edited from the command \
                     center. The chief's limits (budget, models, \
                     caps) live in <home>/limits.toml.
env: HAMLET_HOME (~/.hamlet), HAMLET_WORKSPACE (<home>/workspace), \
                     HAMLET_USER ($USER);
     HAMLET_BASE_URL, HAMLET_MODEL, HAMLET_API_KEY, \
                     HAMLET_REFLECT_EVERY, HAMLET_NET override the \
                     settings.";

/// Time between two updates of an app with no window.
const TICK: Duration = Duration::from_millis(30);

#[derive(Debug, PartialEq)]
enum Mode {
    Window,
    Headless,
    Repl,
}

#[derive(Debug, PartialEq)]
struct Args {
    mode: Mode,
    insecure: bool,
    replay: Option<PathBuf>,
}

/// Reads the command line. `None` when it asks for the usage text.
fn parse(
    mut args: impl Iterator<Item = String>,
) -> Result<Option<Args>> {
    let mut parsed = Args {
        mode: Mode::Window,
        insecure: false,
        replay: None,
    };
    while let Some(a) = args.next() {
        match a.as_str() {
            "--headless" => parsed.mode = Mode::Headless,
            "--repl" => parsed.mode = Mode::Repl,
            "--insecure" => parsed.insecure = true,
            "--replay" => {
                let Some(path) = args.next() else {
                    bail!(
                        "--replay needs the path of a session \
                         log\n\n{USAGE}"
                    );
                };
                parsed.replay = Some(PathBuf::from(path));
            }
            "-h" | "--help" => return Ok(None),
            other => bail!("unknown argument `{other}`\n\n{USAGE}"),
        }
    }
    Ok(Some(parsed))
}

fn main() -> Result<()> {
    let Some(Args {
        mode,
        insecure,
        replay,
    }) = parse(std::env::args().skip(1))?
    else {
        println!("{USAGE}");
        return Ok(());
    };

    let root = std::env::var_os("HAMLET_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::home_dir().map(|h| h.join(".hamlet")))
        .ok_or_else(|| {
            anyhow::anyhow!("no home directory; set HAMLET_HOME")
        })?;
    // A relative path and an absolute one are the same home.
    let root = std::path::absolute(&root).unwrap_or(root);
    let home = Home::open(
        root.clone(),
        std::env::var_os("HAMLET_WORKSPACE").map(PathBuf::from),
    )?;

    let show_requests = match instance::claim(&home.root)? {
        Instance::Secondary => {
            eprintln!(
                "hamlet is already running for {}; asked it to show \
                 its window.",
                home.root.display()
            );
            return Ok(());
        }
        Instance::Primary(rx) => rx,
    };
    limits::write_template(&home.root)?;
    if insecure {
        eprintln!("warning: --insecure, tools run without a sandbox");
    }

    // Model streams and subprocesses run here; the app's systems only
    // apply their results.
    let rt = tokio::runtime::Runtime::new()?;
    let mut app = App::new();
    let config = Config::load(&home.root)?;
    let replayed = replay.is_some();
    app.add_plugins((
        CorePlugin {
            sandbox: sandbox::detect(insecure)?,
            config,
            replay: replay
                .map(|p| {
                    log::read_events(&p)
                        .map(|e| log::assistant_messages(&e))
                })
                .transpose()?,
            tokio: rt.handle().clone(),
            home,
        },
        SessionPlugin,
        OversightPlugin,
        DesktopPlugin {
            window: mode == Mode::Window,
        },
    ));
    if replayed {
        // A recorded session holds no answers for the mayor's
        // reviews.
        app.insert_resource(Unreviewed);
    }
    let tick = ScheduleRunnerPlugin::run_loop(TICK);
    match mode {
        Mode::Repl => {
            app.add_plugins((tick, TerminalPlugin)).run();
        }
        Mode::Headless => {
            eprintln!("hamlet: running headless; Ctrl-C stops");
            app.add_plugins((tick, TerminalCtrlCHandlerPlugin)).run();
        }
        Mode::Window => {
            let exit = app
                .insert_resource(ShowRequests(show_requests))
                // A window draws far more often than the records
                // change.
                .insert_resource(WatchEvery(Duration::from_millis(
                    200,
                )))
                .add_plugins((
                    DefaultPlugins.set(window_plugin(&root)),
                    RealmPlugin,
                    UiPlugin,
                    Builtin,
                ))
                .run();
            if let AppExit::Error(code) = exit {
                bail!("the window exited with code {code}");
            }
        }
    }
    // Tasks still running are abandoned.
    rt.shutdown_background();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(args: &[&str]) -> Result<Option<Args>> {
        parse(args.iter().map(|a| a.to_string()))
    }

    #[test]
    fn flags_are_read_and_mistakes_are_named() {
        assert_eq!(
            parsed(&["--repl", "--insecure", "--replay", "a.jsonl"])
                .unwrap(),
            Some(Args {
                mode: Mode::Repl,
                insecure: true,
                replay: Some("a.jsonl".into()),
            })
        );
        assert_eq!(parsed(&["--headless", "-h"]).unwrap(), None);
        let unknown = parsed(&["--hedless"]).unwrap_err().to_string();
        assert!(unknown.contains("`--hedless`"), "{unknown}");
        let short = parsed(&["--replay"]).unwrap_err().to_string();
        assert!(short.contains("--replay needs"), "{short}");
    }
}
