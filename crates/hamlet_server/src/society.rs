//! The society: chiefs, the Hand, the city's profiles, hamlets with
//! their trees of agents, workers, channels, and the record of
//! decisions. It also holds what the realm spends (H8) and the
//! oversight of agents by chiefs: what waits for a chief, and the
//! gate that decides a call.

pub(crate) mod access;
pub(crate) mod addresses;
pub(crate) mod agents;
pub(crate) mod authority;
pub(crate) mod channels;
pub(crate) mod chiefs;
pub(crate) mod city;
pub(crate) mod dreams;
pub(crate) mod hamlet_agents;
pub(crate) mod hamlets;
pub(crate) mod hand;
pub(crate) mod library;
pub(crate) mod limits;
pub(crate) mod merges;
pub(crate) mod names;
pub(crate) mod oversight;
pub(crate) mod record;
pub(crate) mod rejection;
pub(crate) mod spend;
#[cfg(test)]
mod tests;
pub(crate) mod tree;
pub(crate) mod workers;
