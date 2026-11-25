//! The stop of agents, one act of the realm.

use std::sync::Arc;

use anyhow::Result;
use hamlet_actor::{Ctx, Handler};
use hamlet_messages::address::Agent;
use hamlet_messages::agent::chat::{Stop, Stopped};
use hamlet_messages::realm::lines::StopAgents;
use hamlet_messages::sender::sender;

use super::Realm;
use super::peer::{self, When};

impl Handler<StopAgents> for Realm {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        stop: StopAgents,
    ) -> Result<Stopped> {
        sender(&ctx, &stop)?;
        let asked = Stop {
            number: 0,
            withdrawn: Vec::new(),
        };
        let mut turns = 0;
        for agent in stop.agents {
            match self
                .peers
                .call(&Agent(agent), &asked, When::BeforeAnswer)
                .await
            {
                Ok(stopped) => turns += stopped.turns,
                Err(error) if peer::refused(&error) => {
                    return Err(error);
                }
                Err(_) => {}
            }
        }
        Ok(Stopped { turns })
    }
}
