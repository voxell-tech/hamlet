//! An agent's house, the furniture of its yard and its robot.

use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use super::{HouseSignature, Pass};
use crate::generator::building::{Cost, HouseRecipe};
use crate::generator::layout::{HALL_PLOT, Plot, plot};
use crate::generator::shift::{Folded, Prop, Way};
use crate::generator::society::{Agent, AgentId, Town};
use crate::generator::work::{Engine, Lot, Robot};
use crate::generator::yard::Yard;
use crate::generator::{House, Passage, house, state_index};
use crate::keeper::{self, KeeperState};

impl Pass<'_, '_, '_> {
    /// Gives each agent the house the society has for it, and a
    /// living one its robot.
    pub(super) fn houses(&mut self) {
        let (society, land) = (self.society, self.land);
        for agent in &society.agents {
            let Some(town) = society.town(agent.town) else {
                continue;
            };
            let Some(group) = self.drawn.towns.get(&town.id) else {
                continue;
            };
            let near = group.near;
            let signature = HouseSignature {
                site: town.site,
                at: land.sites[town.site].at,
                plot: agent.plot,
                archived: agent.archived,
                cleared: agent.cleared,
                mayor: town.mayor == agent.id,
                wings: agent.wings,
            };
            let spot = plot(land, town.site, agent.plot);
            let yard = house::yard_of(
                land,
                agent.id,
                agent.plot == HALL_PLOT,
                spot.yaw,
                agent.wings,
            );
            let drawn = self.drawn.houses.get(&agent.id);
            let way = if drawn.map(|h| &h.1) != Some(&signature) {
                self.house(agent, town, signature, &spot, yard, near)
            } else {
                None
            };
            self.robot(
                agent,
                town,
                &spot,
                yard,
                way == Some(Way::In),
            );
        }
    }

    /// Spawns the house of `agent` as `signature` has it under
    /// `near`, in place of the one drawn for it. The way it comes
    /// or goes, if it does.
    fn house(
        &mut self,
        agent: &Agent,
        town: &Town,
        signature: HouseSignature,
        spot: &Plot,
        yard: Yard,
        near: Entity,
    ) -> Option<Way> {
        let (land, kit) = (self.land, self.kit);
        let hall = agent.plot == HALL_PLOT;
        let was = self.drawn.houses.get(&agent.id).map(|h| &h.1);
        let moved = was.is_some_and(|was| {
            (was.site, was.plot, was.at)
                != (signature.site, signature.plot, signature.at)
        });
        // The houses of a town that is set down stand up one after
        // another, from the hall outward.
        let wait = if self.carried.contains(&town.id) {
            let turn = agent.plot as f32 * self.motion.move_gap;
            (turn, turn + self.motion.move_after)
        } else {
            (0.0, 0.0)
        };
        let way =
            match (was.map(|was| was.archived), signature.archived) {
                _ if self.frozen
                    || self.founded.contains(&town.id) =>
                {
                    None
                }
                (None | Some(true), false) => Some(Way::In),
                (Some(false), false) if moved => Some(Way::In),
                (Some(false), true) => Some(Way::Out),
                _ => None,
            };
        // The house it replaces stays drawn until the new mesh is
        // ready.
        let mut old = None;
        if let Some((was, signature, _)) =
            self.drawn.houses.remove(&agent.id)
        {
            old = match self.builds.cancel_house(was) {
                Some(shown) => {
                    self.commands.entity(was).try_despawn();
                    Some(shown)
                }
                None => Some(was),
            };
            self.drawn.lamps.remove(&agent.id);
            self.drawn.engines.remove(&agent.id);
            self.stale_on(signature.site);
        }
        let at = Transform::from_translation(land.on_ground(spot.at))
            .with_rotation(Quat::from_rotation_y(spot.yaw));
        let entity = self
            .commands
            .spawn((
                House {
                    agent: agent.id,
                    town: town.id,
                    archived: agent.archived,
                },
                MeshMaterial3d(kit.matte.clone()),
                at,
                // One that comes in or goes out shows when its part
                // begins.
                if way.is_some() {
                    Visibility::Hidden
                } else {
                    Visibility::Inherited
                },
                ChildOf(near),
            ))
            .id();
        self.builds.house(
            land,
            HouseRecipe {
                agent: agent.id,
                site: town.site,
                hall,
                mayor: signature.mayor,
                remains: agent.remains(&land.set.house),
                wings: agent.wings,
                yard,
            },
            entity,
            old,
            way,
            wait,
        );
        if !agent.archived {
            self.furnish(entity, agent.id, &yard);
        }
        if let Some((at, kind, scale)) =
            house::garden_tree(land, agent.id, &yard)
            && !hall
        {
            let at = Transform::from_translation(at)
                .with_scale(Vec3::splat(scale));
            self.commands.spawn((
                Mesh3d(kit.trees[kind].clone()),
                MeshMaterial3d(kit.matte.clone()),
                at,
                Prop::tree(at),
                Visibility::Inherited,
                ChildOf(entity),
            ));
        }
        self.drawn
            .houses
            .insert(agent.id, (entity, signature, Cost::default()));
        self.stale.insert(town.id);
        // A house that changed takes its robot along. One on its
        // way out goes when its bow is over.
        if let Some(robot) = self.drawn.robots.remove(&agent.id)
            && !self.passing.get(robot).is_ok_and(Passage::is_leaving)
        {
            self.commands.entity(robot).try_despawn();
        }
        way
    }

    /// Stands the desk and the engine in the yard of the house
    /// `entity`.
    fn furnish(
        &mut self,
        entity: Entity,
        agent: AgentId,
        yard: &Yard,
    ) {
        let kit = self.kit;
        let [_, engine] = [
            (&kit.desk, yard.desk_at()),
            (&kit.engine, yard.engine_at()),
        ]
        .map(|(mesh, at)| {
            self.commands
                .spawn((
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(kit.matte.clone()),
                    at,
                    Prop::yard(at),
                    Visibility::Inherited,
                    ChildOf(entity),
                ))
                .id()
        });
        let wheel = self
            .commands
            .spawn((
                Mesh3d(kit.wheel.clone()),
                MeshMaterial3d(kit.matte.clone()),
                yard.wheel(0.0),
                ChildOf(engine),
            ))
            .id();
        // Steam shows the light of work.
        let steam = &kit.lamps[state_index(KeeperState::Working)];
        let rings = [(); 2].map(|_| {
            self.commands
                .spawn((
                    Mesh3d(kit.ring.clone()),
                    MeshMaterial3d(steam.clone()),
                    Visibility::Hidden,
                    NotShadowCaster,
                    ChildOf(entity),
                ))
                .id()
        });
        self.drawn.engines.insert(agent, Engine { wheel, rings });
    }

    /// Spawns the robot of an agent that has none under `under`, and
    /// takes away that of one that is gone. One whose house `rises`
    /// appears once it stands.
    fn robot(
        &mut self,
        agent: &Agent,
        town: &Town,
        spot: &Plot,
        yard: Yard,
        rises: bool,
    ) {
        let absent = agent.archived || !self.robots;
        let engine = self.drawn.engines.get(&agent.id).copied();
        match (absent, self.drawn.robots.get(&agent.id), engine) {
            (true, Some(&robot), _) => {
                // One on its way out goes when its bow is over.
                if !self
                    .passing
                    .get(robot)
                    .is_ok_and(Passage::is_leaving)
                {
                    self.commands.entity(robot).try_despawn();
                }
                self.drawn.robots.remove(&agent.id);
            }
            (false, None, Some(engine)) => {
                let lot = Lot::new(self.land, spot, yard);
                let who = if town.mayor == agent.id {
                    keeper::Who::mayor()
                } else {
                    keeper::Who::agent(agent.id.0 * 7 + 3)
                };
                let robot = keeper::spawn_agent(
                    self.commands,
                    self.robot_kit,
                    self.materials,
                    who,
                    Transform::from_translation(lot.porch)
                        .with_rotation(Quat::from_rotation_y(
                            lot.yaw,
                        )),
                    None,
                );
                self.commands.entity(robot).try_insert(Robot::new(
                    agent.id,
                    agent.state,
                    agent.work,
                    lot,
                    engine,
                ));
                if rises {
                    self.commands
                        .entity(robot)
                        .try_insert((Visibility::Hidden, Folded));
                }
                self.drawn.robots.insert(agent.id, robot);
            }
            _ => {}
        }
    }
}
