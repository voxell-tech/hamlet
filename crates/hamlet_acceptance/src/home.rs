//! A scratch home and the servers started on it.

use std::path::{Path, PathBuf};

use hamlet_testing::{Models, Scratch, Server, Unstarted, cli};

use crate::Faux;

/// A home and the folder beside it for scripts and fixtures, kept
/// across the servers started on it.
pub struct Home {
    home: Scratch,
    side: Scratch,
    started: u32,
}

impl Home {
    pub fn new() -> Self {
        Self {
            home: Scratch::new("home"),
            side: Scratch::new("side"),
            started: 0,
        }
    }

    /// The home's folder, given to the server as `HAMLET_HOME`.
    pub fn path(&self) -> &Path {
        &self.home
    }

    /// A folder of the scenario's own, outside the home.
    pub fn side(&self) -> &Path {
        &self.side
    }

    /// The path of `relative` in the home.
    pub fn file(&self, relative: &str) -> PathBuf {
        self.path().join(relative)
    }

    /// The folder of agent `agent`'s own files, its filesystem in the
    /// realm's actor folder.
    pub fn agent_files(&self, agent: u64) -> PathBuf {
        self.file(&format!("actors/agent/{agent}.fs"))
    }

    /// Starts a server on this home answering with `faux`, with
    /// `env` added to its environment.
    pub async fn serve(
        &mut self,
        faux: &Faux,
        env: &[(&str, &str)],
    ) -> Server {
        self.launch(faux, env)
            .await
            .unwrap_or_else(|failure| panic!("{failure}"))
    }

    /// Starts a server that is expected to fail, and gives what it
    /// printed.
    pub async fn try_serve(
        &mut self,
        faux: &Faux,
        env: &[(&str, &str)],
    ) -> String {
        match self.launch(faux, env).await {
            Ok(_) => panic!("the server started"),
            Err(failure) => failure.to_string(),
        }
    }

    /// Starts a server on this home with its real model providers,
    /// without `--faux`. `env` points them at a loopback endpoint.
    pub async fn serve_providers(
        &mut self,
        env: &[(&str, &str)],
    ) -> Server {
        Server::start(
            &server_command(),
            self.path(),
            Models::Providers,
            env,
        )
        .await
        .unwrap_or_else(|failure| panic!("{failure}"))
    }

    async fn launch(
        &mut self,
        faux: &Faux,
        env: &[(&str, &str)],
    ) -> Result<Server, Unstarted> {
        self.started += 1;
        let file =
            self.side().join(format!("script-{}.json", self.started));
        faux.write(&file);
        Server::start(
            &server_command(),
            self.path(),
            Models::Faux(&file),
            env,
        )
        .await
    }
}

impl Default for Home {
    fn default() -> Self {
        Self::new()
    }
}

/// The command that starts a server, before `--port 0` and
/// `--faux <file>`: the `hamlet` CLI's `server`.
fn server_command() -> Vec<String> {
    let cli = cli();
    assert!(
        cli.is_file(),
        "{} is missing: build it with `cargo build -p hamlet_cli`, \
         or set HAMLET_CLI to a built CLI",
        cli.display()
    );
    vec![cli.display().to_string(), "server".into()]
}
