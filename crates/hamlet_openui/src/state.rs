//! What a panel keeps between its draws.

use bevy::prelude::*;
use openui_lang::Inputs;

/// On a panel's entity: the values of its `$variables`, as the user
/// left them.
#[derive(Component, Default, Debug)]
pub struct State(pub Inputs);

/// On a panel's entity: what the last action could not do. It is
/// shown with the next draw.
#[derive(Component, Default, Debug)]
pub struct Raised(pub Vec<String>);
