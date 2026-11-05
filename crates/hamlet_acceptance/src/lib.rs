#![doc = include_str!("../README.md")]

pub mod cli;
pub mod client;
pub mod faux;
pub mod fold;
pub mod home;
pub mod realm;

pub use cli::{Cli, Ended};
pub use client::{Client, Ending, Wire, chat};
pub use faux::Faux;
pub use hamlet_protocol::client::AgentId;
use hamlet_testing::openai;
pub use hamlet_testing::{Server, call, calls, failure, words};
pub use home::Home;

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
    /// A fresh home served with `faux` and [`QUIET`], and the
    /// chief's connection.
    pub async fn start(faux: Faux) -> Self {
        Self::start_with(faux, QUIET).await
    }

    /// A fresh home served with `faux` and `env`.
    pub async fn start_with(
        faux: Faux,
        env: &[(&str, &str)],
    ) -> Self {
        let mut home = Home::new();
        let server = home.serve(&faux, env).await;
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

    /// The Hand's id.
    pub fn hand(&self) -> AgentId {
        self.chief.hand()
    }

    /// Founds the hamlet `hamlet` under the scholar `scholar`, new
    /// or the city's, and gives the mayor's id.
    pub async fn found(
        &mut self,
        hamlet: &str,
        scholar: &str,
        purpose: &str,
    ) -> AgentId {
        let hand = self.hand();
        self.chief
            .command(
                hand,
                &format!("/found {hamlet} {scholar} {purpose}"),
            )
            .await;
        let name = format!(
            "{}@{hamlet}",
            scholar.split('@').next().unwrap_or(scholar)
        );
        self.chief
            .until(&format!("{name} in the realm"), |c| {
                c.agent(&name).is_some()
            })
            .await;
        self.chief.id(&name)
    }
}
