#![doc = include_str!("../README.md")]

pub mod cli;
pub mod client;
pub mod fold;
pub mod realm;

pub use cli::{Cli, Ended};
pub use client::{Client, Ending, Wire, chat, mayor, to};
use hamlet_testing::{Home, Script, Server, openai};

/// A home, its server and the chief's connection to it.
pub struct Session {
    pub home: Home,
    pub server: Server,
    pub chief: Client,
}

/// The environment every scenario's server gets unless it says
/// otherwise: no reflection between turns.
pub const QUIET: &[(&str, &str)] = &[("HAMLET_REFLECT_EVERY", "0")];

impl Session {
    /// A fresh home served with `script` and [`QUIET`], and the
    /// chief's connection.
    pub async fn start(script: Script) -> Self {
        Self::start_with(script, QUIET).await
    }

    /// A fresh home served with `script` and `env`.
    pub async fn start_with(
        script: Script,
        env: &[(&str, &str)],
    ) -> Self {
        let mut home = Home::new();
        let server = home.serve(&script, env).await;
        let chief = Client::join(&server).await;
        Self {
            home,
            server,
            chief,
        }
    }

    /// A fresh home served with its real model providers, pointed at
    /// `endpoint` for the model `local::org/large`.
    pub async fn with_endpoint(endpoint: &openai::Endpoint) -> Self {
        let mut home = Home::new();
        let env = [
            ("HAMLET_REFLECT_EVERY", "0"),
            ("HAMLET_MODEL", "local::org/large"),
            ("HAMLET_BASE_URL", endpoint.url.as_str()),
        ];
        let server = home.serve_providers(&env).await;
        let chief = Client::join(&server).await;
        Self {
            home,
            server,
            chief,
        }
    }

    /// Another connection of the chief's.
    pub async fn join(&self) -> Client {
        Client::join(&self.server).await
    }
}
