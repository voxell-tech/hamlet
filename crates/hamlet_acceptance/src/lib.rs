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
pub use hamlet_protocol::client::{AgentId, ProfileId};
use hamlet_testing::openai;
pub use hamlet_testing::{Server, call, calls, failure, words};
pub use home::Home;

/// A home, its server and the chief's connection to it.
pub struct Session {
    pub home: Home,
    pub server: Server,
    pub chief: Client,
}

/// The file beside the faux script that holds each founded hamlet's
/// id under its name, for a step [`Step::filled_from`] it to name a
/// hamlet as `{{name}}`.
///
/// [`Step::filled_from`]: hamlet_testing::Step::filled_from
pub const NAMES: &str = "names.json";

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

    /// The id of the city's living profile `name`, made first when
    /// there is none.
    pub async fn profile(&mut self, name: &str) -> ProfileId {
        let living = |c: &Client| {
            c.profile(name).filter(|profile| profile.left.is_none())
        };
        if let Some(profile) = living(&self.chief) {
            return profile.id;
        }
        let hand = self.hand();
        self.chief
            .command(hand, &format!("/profile new {name}"))
            .await;
        self.chief
            .until(&format!("profile {name}"), |c| {
                living(c).is_some()
            })
            .await;
        living(&self.chief).expect("the profile is made").id
    }

    /// Founds the hamlet named `hamlet` under the profile called
    /// `profile`, made if the city has none, keeps its id in
    /// [`NAMES`] and gives the lead's id.
    pub async fn found(
        &mut self,
        hamlet: &str,
        profile: &str,
        purpose: &str,
    ) -> AgentId {
        let profile = self.profile(profile).await;
        let hand = self.hand();
        self.chief
            .command(
                hand,
                &format!(
                    "/create-hamlet profile#{} {hamlet}: {purpose}",
                    profile.0
                ),
            )
            .await;
        self.named(hamlet).await
    }

    /// Waits for the hamlet named `hamlet`, keeps its id in
    /// [`NAMES`] for the faux script, and gives its lead's id.
    pub async fn named(&mut self, hamlet: &str) -> AgentId {
        self.chief
            .until(&format!("{hamlet} in the realm"), |c| {
                c.hamlet(hamlet).is_some()
            })
            .await;
        let found = self.chief.hamlet(hamlet).expect("it is founded");
        let file = self.home.side().join(NAMES);
        let mut names = std::fs::read(&file)
            .ok()
            .and_then(|bytes| {
                serde_json::from_slice::<
                    serde_json::Map<String, serde_json::Value>,
                >(&bytes)
                .ok()
            })
            .unwrap_or_default();
        names.insert(hamlet.into(), found.id.into());
        std::fs::write(&file, serde_json::to_vec(&names).unwrap())
            .unwrap();
        found.lead
    }

    /// Has `parent` add an agent of the profile called `profile`,
    /// made if the city has none, and gives the new agent's id.
    pub async fn add_agent(
        &mut self,
        parent: AgentId,
        profile: &str,
        purpose: &str,
    ) -> AgentId {
        self.added(parent, profile, "", purpose).await
    }

    /// Has `parent` add an agent of the profile called `profile`
    /// under the agent at `under`, and gives the new agent's id.
    pub async fn add_agent_under(
        &mut self,
        parent: AgentId,
        profile: &str,
        under: &str,
        purpose: &str,
    ) -> AgentId {
        self.added(
            parent,
            profile,
            &format!("under {under} "),
            purpose,
        )
        .await
    }

    async fn added(
        &mut self,
        parent: AgentId,
        profile: &str,
        under: &str,
        purpose: &str,
    ) -> AgentId {
        let profile = self.profile(profile).await;
        let hamlet = self
            .chief
            .realm()
            .agents
            .into_iter()
            .find(|a| a.id == parent)
            .and_then(|a| a.hamlet);
        let agents = move |c: &Client| {
            c.realm()
                .agents
                .into_iter()
                .filter(|a| {
                    a.profile == Some(profile)
                        && a.hamlet == hamlet
                        && a.left.is_none()
                })
                .collect::<Vec<_>>()
        };
        let before = agents(&self.chief).len();
        self.chief
            .command(
                parent,
                &format!(
                    "/add-agent profile#{} {under}{purpose}",
                    profile.0
                ),
            )
            .await;
        self.chief
            .until("the new agent", |c| agents(c).len() > before)
            .await;
        agents(&self.chief)
            .into_iter()
            .max_by_key(|a| a.id)
            .expect("the agent came")
            .id
    }
}
