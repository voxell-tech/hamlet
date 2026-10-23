//! The places of the world: where each stands, where one fits, and
//! founding and moving a town.

use bevy::prelude::*;

use super::{Society, Town, TownId};
use crate::generator::land::{Land, Unfit};
use crate::generator::layout::{HALL_PLOT, plot};

/// Something that stands on a round of ground of its own, and can
/// be picked up and set down somewhere else.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Ground {
    Town(TownId),
    /// The chief's own place.
    Chief,
    /// The library, which is nobody's and everyone's.
    Library,
}

/// The ground held by a place that is not a town.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Claim {
    pub place: Ground,
    pub at: Vec2,
    pub radius: f32,
}

impl Society {
    /// The spot of a place and the round of ground it holds.
    pub fn stands(
        &self,
        land: &Land,
        place: Ground,
    ) -> Option<(Vec2, f32)> {
        match place {
            Ground::Town(id) => self.town(id).map(|town| {
                (
                    land.sites[town.site].at,
                    land.set.town.keep_apart * 0.5,
                )
            }),
            _ => self
                .claims
                .iter()
                .find(|claim| claim.place == place)
                .map(|claim| (claim.at, claim.radius)),
        }
    }

    /// Every place there is.
    pub fn places(&self) -> impl Iterator<Item = Ground> {
        self.towns
            .iter()
            .map(|town| Ground::Town(town.id))
            .chain(self.claims.iter().map(|claim| claim.place))
    }

    /// True when a round of ground at `at` runs into any place but
    /// `but`. With `home`, the round is a town's own first ground,
    /// and towns that never moved are as near as the land put them.
    fn crowded(
        &self,
        land: &Land,
        but: Ground,
        at: Vec2,
        radius: f32,
        home: bool,
    ) -> bool {
        let settled = |place: Ground| match place {
            Ground::Town(id) => self
                .town(id)
                .is_some_and(|town| !land.sites[town.site].moved()),
            _ => false,
        };
        self.places()
            .filter(|place| {
                *place != but && !(home && settled(*place))
            })
            .any(|place| {
                self.stands(land, place).is_some_and(
                    |(other, room)| {
                        other.distance(at) < radius + room
                    },
                )
            })
    }

    /// `Ok` when `place` could be set down at `to`: on ground that
    /// suits it, clear of every other place, and for a town with
    /// every house it has on dry ground.
    pub fn fits(
        &self,
        land: &Land,
        place: Ground,
        to: Vec2,
    ) -> Result<(), Unfit> {
        let Some((_, radius)) = self.stands(land, place) else {
            return Err(Unfit::Crowded);
        };
        // The ground a town was founded on takes it back as it is.
        let home = match place {
            Ground::Town(id) => self
                .town(id)
                .is_some_and(|town| land.sites[town.site].home == to),
            _ => false,
        };
        if !home {
            land.suits(to)?;
        }
        if self.crowded(land, place, to, radius, home) {
            return Err(Unfit::Crowded);
        }
        let Ground::Town(id) = place else {
            return Ok(());
        };
        let Some(town) = self.town(id) else {
            return Err(Unfit::Crowded);
        };
        // The plots as they would lie there.
        let mut there = land.clone();
        there.move_site(town.site, to);
        let dry = self
            .agents
            .iter()
            .filter(|a| a.town == id && !a.cleared)
            .all(|a| plot(&there, town.site, a.plot).usable);
        if dry { Ok(()) } else { Err(Unfit::Water) }
    }

    /// Sets a town down at `to` with everyone and everything in it.
    /// The roads to it find their way anew.
    pub fn move_town(
        &mut self,
        land: &mut Land,
        id: TownId,
        to: Vec2,
    ) -> Result<(), Unfit> {
        self.fits(land, Ground::Town(id), to)?;
        let site = self.town(id).map(|town| town.site);
        if let Some(site) = site {
            land.move_site(site, to);
        }
        Ok(())
    }

    /// The first site no town stands on, where it would lie on its
    /// own. `clear` leaves out those another place has come to
    /// stand on.
    fn free_site(&self, land: &Land, clear: bool) -> Option<usize> {
        let room = land.set.town.keep_apart * 0.5;
        (0..land.sites.len()).find(|&s| {
            let site = &land.sites[s];
            // Towns that never moved are as near as the land put
            // them.
            let taken = clear
                && (self.claims.iter().any(|claim| {
                    claim.at.distance(site.home) < room + claim.radius
                }) || self.towns.iter().any(|town| {
                    let other = &land.sites[town.site];
                    other.moved()
                        && other.at.distance(site.home) < room * 2.0
                }));
            site.buildable
                && !taken
                && !self.towns.iter().any(|t| t.site == s)
        })
    }

    /// Founds a town with a leader on the first free site, and lays
    /// a road from the new leader to every other leader. There is
    /// always a site: when all the land has are taken, it readies
    /// one more, further out.
    pub fn found_town(&mut self, land: &mut Land) -> TownId {
        let site = match self.free_site(land, true) {
            Some(site) => {
                let home = land.sites[site].home;
                land.move_site(site, home);
                site
            }
            None => {
                let room = land.set.town.keep_apart * 0.5;
                land.open_site(|at| {
                    self.claims.iter().any(|claim| {
                        claim.at.distance(at) < room + claim.radius
                    })
                })
            }
        };
        self.found_on(site)
    }

    /// Founds a town as [`Society::found_town`] does and sets it
    /// down at `at`, if it fits there.
    pub(crate) fn found_town_at(
        &mut self,
        land: &mut Land,
        at: Vec2,
    ) -> TownId {
        // It takes the site it would have had with nothing in the
        // way, since it is not to stay there.
        let Some(site) = self.free_site(land, false) else {
            let id = self.found_town(land);
            let _ = self.move_town(land, id, at);
            return id;
        };
        let id = self.found_on(site);
        if self.move_town(land, id, at).is_ok() {
            return id;
        }
        let home = land.sites[site].home;
        let room = land.set.town.keep_apart * 0.5;
        if !self.crowded(land, Ground::Town(id), home, room, true) {
            land.move_site(site, home);
            return id;
        }
        // Neither there nor on its own site: on the next that is
        // free.
        if let Some(next) = self.free_site(land, true) {
            let home = land.sites[next].home;
            land.move_site(next, home);
            self.towns
                .iter_mut()
                .filter(|town| town.id == id)
                .for_each(|town| town.site = next);
        }
        id
    }

    fn found_on(&mut self, site: usize) -> TownId {
        let id = TownId(self.next_town);
        self.next_town += 1;
        let leader = self.spawn_agent(id, HALL_PLOT);
        let others =
            self.towns.iter().map(|t| t.leader).collect::<Vec<_>>();
        self.towns.push(Town { id, site, leader });
        for other in others {
            self.add_road(leader, other);
        }
        id
    }
}
