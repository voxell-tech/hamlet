//! The realm as a client keeps it: the last whole state with the
//! changes since applied.

use hamlet_protocol::v0::*;

/// The realm after every state and change in `log`.
pub fn fold(log: &[ServerMessage]) -> State {
    let mut realm = None::<State>;
    for message in log {
        match message {
            ServerMessage::State(state) => {
                realm = Some(state.clone())
            }
            ServerMessage::Change(changes) => {
                let state = realm
                    .as_mut()
                    .expect("a change comes after a whole state");
                assert_eq!(
                    changes.epoch, state.epoch,
                    "a change belongs to the current epoch"
                );
                assert!(
                    changes.revision > state.revision,
                    "a change advances the revision"
                );
                for update in &changes.changes {
                    apply(state, update);
                }
                state.revision = changes.revision;
            }
            _ => {}
        }
    }
    realm.expect("a client has a state after hello")
}

fn apply(state: &mut State, update: &Update) {
    match update {
        Update::Hamlet { hamlet } => {
            match state.hamlets.iter_mut().find(|h| h.id == hamlet.id)
            {
                Some(old) => *old = hamlet.clone(),
                None => state.hamlets.push(hamlet.clone()),
            }
        }
        Update::Agent { agent } => {
            match state
                .agents
                .iter_mut()
                .find(|a| a.ref_ == agent.ref_)
            {
                Some(old) => *old = agent.clone(),
                None => state.agents.push(agent.clone()),
            }
        }
        Update::AgentGone { ref_ } => {
            state.agents.retain(|a| &a.ref_ != ref_);
        }
        Update::Wait { wait } => {
            match state.waits.iter_mut().find(|w| w.id == wait.id) {
                Some(old) => *old = wait.clone(),
                None => state.waits.push(wait.clone()),
            }
        }
        Update::WaitClosed { id, .. } => {
            state.waits.retain(|w| w.id != *id);
        }
        Update::Grants { grants } => state.grants.clone_from(grants),
        Update::Settings { settings } => {
            state.settings.clone_from(settings);
        }
        Update::Providers { providers } => {
            state.providers.clone_from(providers);
        }
    }
}
