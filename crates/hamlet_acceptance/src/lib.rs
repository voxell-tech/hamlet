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
pub use hamlet_protocol::client::{AgentId, ScholarId};
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
        Self::start_in(Home::new(), faux, env).await
    }

    /// `home`, served with `faux` and `env`, for a scenario that
    /// needs the home's paths before its server runs.
    pub async fn start_in(
        mut home: Home,
        faux: Faux,
        env: &[(&str, &str)],
    ) -> Self {
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

    /// The id of the city's living scholar `name`, made first when
    /// there is none.
    pub async fn scholar(&mut self, name: &str) -> ScholarId {
        let living = |c: &Client| {
            c.scholar(name).filter(|scholar| scholar.left.is_none())
        };
        if let Some(scholar) = living(&self.chief) {
            return scholar.id;
        }
        let hand = self.hand();
        self.chief
            .command(hand, &format!("/scholar new {name}"))
            .await;
        self.chief
            .until(&format!("scholar {name}"), |c| {
                living(c).is_some()
            })
            .await;
        living(&self.chief).expect("the scholar is made").id
    }

    /// Founds the hamlet `hamlet` under the scholar called `scholar`,
    /// made if the city has none, and gives the mayor's id.
    pub async fn found(
        &mut self,
        hamlet: &str,
        scholar: &str,
        purpose: &str,
    ) -> AgentId {
        let scholar = self.scholar(scholar).await;
        let hand = self.hand();
        self.chief
            .command(
                hand,
                &format!(
                    "/create-hamlet {hamlet} scholar#{} {purpose}",
                    scholar.0
                ),
            )
            .await;
        let mayor = format!("{hamlet}#0");
        self.chief
            .until(&format!("{mayor} in the realm"), |c| {
                c.agent(&mayor).is_some()
            })
            .await;
        self.chief.id(&mayor)
    }

    /// Has `lead` hire the scholar called `scholar`, made if the
    /// city has none, and gives the new hire's id.
    pub async fn hire(
        &mut self,
        lead: AgentId,
        scholar: &str,
        purpose: &str,
    ) -> AgentId {
        self.hired(lead, scholar, "", purpose).await
    }

    /// Has `lead` hire the scholar called `scholar` under the agent
    /// at `under`, and gives the new hire's id.
    pub async fn hire_under(
        &mut self,
        lead: AgentId,
        scholar: &str,
        under: &str,
        purpose: &str,
    ) -> AgentId {
        self.hired(lead, scholar, &format!("under {under} "), purpose)
            .await
    }

    async fn hired(
        &mut self,
        lead: AgentId,
        scholar: &str,
        under: &str,
        purpose: &str,
    ) -> AgentId {
        let scholar = self.scholar(scholar).await;
        let hamlet = self
            .chief
            .realm()
            .agents
            .into_iter()
            .find(|a| a.id == lead)
            .and_then(|a| a.hamlet);
        let hires = move |c: &Client| {
            c.realm()
                .agents
                .into_iter()
                .filter(|a| {
                    a.scholar == Some(scholar)
                        && a.hamlet == hamlet
                        && a.left.is_none()
                })
                .collect::<Vec<_>>()
        };
        let before = hires(&self.chief).len();
        self.chief
            .command(
                lead,
                &format!(
                    "/hire scholar#{} {under}{purpose}",
                    scholar.0
                ),
            )
            .await;
        self.chief
            .until("the new hire", |c| hires(c).len() > before)
            .await;
        hires(&self.chief)
            .into_iter()
            .max_by_key(|a| a.number)
            .expect("the hire came")
            .id
    }
}
