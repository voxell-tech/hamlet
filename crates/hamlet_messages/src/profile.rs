//! The calls a `profile` actor takes, and its answers.

use hamlet_actor::Action;
use serde::{Deserialize, Serialize};

use crate::Message;
use crate::address::{Profile, Realm};

/// Makes the branch `name` when the profile does not have it. Called
/// by the realm, for a hire. A repeat finds the branch and answers
/// the same.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Branch {
    pub name: String,
}

impl Action for Branch {
    type Output = ();
    const NAME: &'static str = "branch";
}

impl Message for Branch {
    type To = Profile;
    type From = Realm;
}
