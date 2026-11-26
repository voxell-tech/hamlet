//! The world as the client protocol shows it, in a chief's words.

use hamlet_messages::realm::client::ReadState;
use hamlet_messages::sender::Client;
use hamlet_protocol::client as protocol;

use super::Realm;
use super::act::Look;

impl Look<Client> for ReadState {
    type Output = protocol::State;

    fn look(&self, _by: &Client, _realm: &Realm) -> protocol::State {
        todo!("the realm's part of the client's State")
    }
}
