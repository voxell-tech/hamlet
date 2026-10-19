//! A message one agent sends another and waits on.

use anyhow::{Result, bail};
use hamlet_model::log;
use hamlet_rules::hamlet::Hamlet;

use super::{Agent, Event, Listener};

impl Agent {
    /// Carries `message` along the road to `to`, runs the other
    /// agent's turn in its own house, and returns its reply. `to`
    /// is `mayor`, an agent's name, or `mayor@<hamlet>`.
    pub(super) async fn send(
        &mut self,
        to: &str,
        message: &str,
        on: &mut Listener<'_>,
    ) -> Result<String> {
        let (Some(hamlet), Some(name)) = (
            self.hamlet.as_ref().map(Hamlet::again),
            self.resident.as_ref().map(|r| r.name.clone()),
        ) else {
            bail!("not in a hamlet");
        };
        hamlet.may_send(&self.task)?;
        let realm = self.realm(&hamlet);
        // Inside the hamlet first, then across hamlets.
        let (there, resident, me, to) = match hamlet.resolve(to) {
            Some(resident) => {
                let me = self.name();
                let to = hamlet.address(&resident.name);
                if !hamlet.has_road(&me, &to) {
                    bail!("{}", no_road(&to));
                }
                (hamlet.again(), resident, me, to)
            }
            None => {
                let Some((there, resident)) = realm.resolve(to)
                else {
                    bail!("no agent named `{to}`");
                };
                let me = realm.address(&hamlet, &name);
                let to = realm.address(&there, &resident.name);
                if !realm.has_road(&me, &to) {
                    bail!("{}", no_road(&to));
                }
                (there, resident, me, to)
            }
        };
        let house = there.house(&resident.name)?;
        let path = house.sessions().join(format!("{me}.jsonl"));
        let mut agent = self.beside(house, &path)?;
        if there.name() == hamlet.name() {
            hamlet.carried(&me, &to, &self.task)?;
        } else {
            // The asker pays (H14).
            realm.carried(&me, &to, hamlet.name())?;
        }
        on(Event::Message {
            from: me.clone(),
            to: to.clone(),
        });
        agent.hamlet = Some(there);
        agent.resident = Some(resident);
        agent.task = self.task.clone();
        agent.awaited = true;
        agent.overseen = self.overseen;
        // Its tool calls pass the same gate as the sender's.
        agent.gate = self.gate.take();
        agent.plug = self.plug.take();
        let mut relay = |event: Event| {
            on(Event::Agent {
                name: to.clone(),
                event: Box::new(event),
            })
        };
        relay(Event::Question {
            from: me.clone(),
            text: message.to_string(),
            at: log::now_millis(),
        });
        // Boxed because the other agent's turn is this same code.
        let reply =
            Box::pin(agent.run(&me, message, &mut relay)).await;
        self.gate = agent.gate.take();
        self.plug = agent.plug.take();
        reply.map_err(|e| {
            anyhow::anyhow!("`{to}` could not finish: {e}")
        })
    }
}

/// The refusal an agent gets when it writes to one it has no road to.
pub(super) fn no_road(to: &str) -> String {
    format!(
        "you have no road to `{to}`. Ask the mayor to pass a single \
         message on, or ask it for a road with request_road."
    )
}
