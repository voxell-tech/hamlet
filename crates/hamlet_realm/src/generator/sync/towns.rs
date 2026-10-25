//! A town's groups, its common and its far mesh.

use std::collections::HashSet;

use bevy::light::NotShadowCaster;
use bevy::math::Affine3A;
use bevy::prelude::*;

use super::{Pass, TownDrawn};
use crate::generator::house::{self, TownStyle};
use crate::generator::layout::{HALL_PLOT, plot};
use crate::generator::lights::Spill;
use crate::generator::{Common, common};
use crate::meshes::Meshing;
use crate::sky::SkySettings;

impl Pass<'_, '_, '_> {
    /// Takes out the towns the society no longer holds, and the
    /// houses and robots of agents it no longer holds.
    pub(super) fn clear(&mut self) {
        let Self {
            commands,
            society,
            drawn,
            builds,
            stale,
            ..
        } = self;
        drawn.towns.retain(|id, group| {
            let stays = society
                .town(*id)
                .is_some_and(|t| t.site == group.site);
            if !stays {
                for entity in [group.near, group.far] {
                    commands.entity(entity).try_despawn();
                }
            }
            stays
        });
        let housed = society
            .agents
            .iter()
            .filter(|a| drawn.towns.contains_key(&a.town))
            .map(|a| a.id)
            .collect::<HashSet<_>>();
        drawn.houses.retain(|id, (entity, was, _)| {
            let stays = housed.contains(id);
            if !stays {
                if let Some(old) = builds.cancel_house(*entity) {
                    commands.entity(old).try_despawn();
                }
                commands.entity(*entity).try_despawn();
                stale.extend(
                    society
                        .towns
                        .iter()
                        .filter(|t| t.site == was.site)
                        .map(|t| t.id),
                );
            }
            stays
        });
        drawn.lamps.retain(|id, _| housed.contains(id));
        drawn.engines.retain(|id, _| housed.contains(id));
        drawn.robots.retain(|id, robot| {
            let stays = housed.contains(id);
            if !stays {
                commands.entity(*robot).try_despawn();
            }
            stays
        });
    }

    /// Spawns the groups and the common of each town that is new,
    /// and moves the common of one that was set down elsewhere.
    pub(super) fn towns(&mut self, sky: &SkySettings) {
        let (land, kit) = (self.land, self.kit);
        let frame = |site: usize| {
            let centre = land.sites[site].at;
            Transform::from_xyz(
                centre.x,
                land.set.land.site_y,
                centre.y,
            )
            .with_rotation(Quat::from_rotation_y(
                -land.sites[site].turn,
            ))
        };
        for town in &self.society.towns {
            let centre = land.sites[town.site].at;
            if let Some(group) = self.drawn.towns.get_mut(&town.id) {
                if group.at != centre {
                    group.at = centre;
                    self.commands
                        .entity(group.common)
                        .try_insert(frame(town.site));
                    self.stale.insert(town.id);
                    self.carried.insert(town.id);
                }
                continue;
            }
            let near = self
                .commands
                .spawn((Transform::default(), Visibility::Inherited))
                .id();
            let far = self
                .commands
                .spawn((
                    Mesh3d::default(),
                    MeshMaterial3d(kit.matte.clone()),
                    Visibility::Hidden,
                ))
                .id();
            let far_panes = self
                .commands
                .spawn((
                    Mesh3d::default(),
                    MeshMaterial3d(kit.windows.clone()),
                    NotShadowCaster,
                    ChildOf(far),
                ))
                .id();
            let common = self
                .commands
                .spawn((
                    Common { town: town.id },
                    MeshMaterial3d(kit.matte.clone()),
                    frame(town.site),
                    Visibility::Inherited,
                    ChildOf(near),
                    children![(
                        Mesh3d(kit.lamp_glass.clone()),
                        MeshMaterial3d(kit.lamps[0].clone()),
                        Transform::from_translation(
                            land.set.town.lamp
                        ),
                        NotShadowCaster,
                        children![Spill::bundle(sky)],
                    ),],
                ))
                .id();
            self.builds.common(land, town.site, common);
            self.drawn.towns.insert(
                town.id,
                TownDrawn {
                    site: town.site,
                    at: centre,
                    common,
                    near,
                    far,
                    far_panes,
                },
            );
            self.stale.insert(town.id);
            self.founded.insert(town.id);
        }
    }

    /// Builds the far mesh of each town that changed.
    pub(super) fn far(&mut self, meshes: &mut Assets<Mesh>) {
        let (land, society) = (self.land, self.society);
        for id in self.stale.drain() {
            let Some(group) = self.drawn.towns.get(&id) else {
                continue;
            };
            let attire = self.attires.get(id);
            let skinned = attire.and_then(|a| a.palette(&land.pal));
            let pal = skinned.as_ref().unwrap_or(&land.pal);
            let worn = attire
                .and_then(|a| a.mesh.as_ref())
                .map(|shell| &shell.bulk);
            let style = TownStyle::within(pal, land.seed, group.site);
            let (mut meshing, mut panes) =
                (Meshing::new(), Meshing::new());
            let centre = land.sites[group.site].at;
            meshing.at(Affine3A::from_translation(Vec3::new(
                centre.x,
                land.set.land.site_y,
                centre.y,
            )));
            common::far(&mut meshing, land);
            for agent in
                society.agents.iter().filter(|a| a.town == id)
            {
                let spot = plot(land, group.site, agent.plot);
                let xf = Affine3A::from_rotation_translation(
                    Quat::from_rotation_y(spot.yaw),
                    land.on_ground(spot.at),
                );
                house::far(
                    &mut meshing,
                    &mut panes,
                    xf,
                    land,
                    pal,
                    agent.id,
                    &style,
                    agent.plot == HALL_PLOT,
                    agent.remains(&land.set.house),
                    worn,
                );
            }
            self.commands
                .entity(group.far)
                .try_insert(Mesh3d(meshes.add(meshing.build())));
            self.commands
                .entity(group.far_panes)
                .try_insert(Mesh3d(meshes.add(panes.build())));
        }
    }
}
