//! The realm as a client keeps it: the last whole state with the
//! changes since applied.

use hamlet_protocol::client::*;

/// The realm after every state and change in `log`.
pub fn fold(log: &[ServerMessage]) -> State {
    let mut realm = None::<State>;
    for message in log {
        match message {
            ServerMessage::State(state) => {
                realm = Some(state.clone());
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
                    state.apply(update);
                }
                state.revision = changes.revision;
            }
            _ => {}
        }
    }
    realm.expect("a client has a state after hello")
}
