//! The library in the world: a stone house under a dome, on ground of
//! its own beside the first town and in no hamlet, with its librarian
//! on the porch. It stands for the skills every agent shares.

mod build;
mod librarian;

use glam::{Quat, Vec2, Vec3};
use hamlet_engine_bevy::api::agents::{
    AgentsEvent, AgentsReply, AgentsRequest,
};
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::scene::{
    Light, Material, Mesh, MeshData, Pbr, PointLight,
};
use hamlet_engine_bevy::{AgentsEventReceived, Replied, Reply};

use crate::build::frame::Frame;
use crate::build::stock::rgba;
use crate::build::upload::{Draw, Unsent};
use crate::folk::chief::mascot::{Doing, Pose, Rig, parts, wick};
use crate::folk::chief::{ChiefSettings, place};
use crate::places::library::librarian::{
    ATTENDS, GROWN, LOOK, STANDS, sage,
};
use crate::places::moving::SetDown;
use crate::realm::{Ctx, Shared};
use crate::society::{Claim, Place, Society};
use crate::view::pick::Thing;
use crate::world::land::Land;
use crate::world::palette::Palette;

/// The radius of the ground the library keeps level and clear.
const GROUNDS: f32 = 9.5;

/// What the library is stood anew with.
type Grounds<'w> =
    (Res<'w, Palette>, ResMut<'w, Land>, ResMut<'w, Society>);

/// Reach of the library from its middle and its height, for the ring
/// about it and the name over it.
pub const SIZE: (f32, f32) = (7.2, 9.2);

/// The light of one of its lamps after dark, and how far it reaches.
const LAMP: f32 = 90_000.0;
const LAMP_REACH: f32 = 11.0;

/// Brightness of the windows' glass by day, and the gain after dark.
const GLASS: (f32, f32) = (1.2, 16.0);

/// The beacon's light at its brightest and how far it reaches, and
/// how many times brighter the windows burn with it.
const BEACON: f32 = 900_000.0;
const BEACON_REACH: f32 = 26.0;
const FLASH: f32 = 9.0;

/// Brightness of the stone in the librarian's staff, the light it
/// throws and its reach.
const STONE: (f32, f32, f32) = (14.0, 30_000.0, 4.5);

/// What a mesh handed over is for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Slot {
    Walls,
    Glass,
    /// A part of the librarian, in the order of `Parts::each`.
    Part(usize),
    Wick,
    Hair,
    Staff,
    Stone,
}

/// The library, once the land has room for it.
#[derive(Resource)]
pub struct Library {
    stood: Option<Stood>,
    /// The spot a chief set it down on, as the home keeps it.
    /// Without one it stands where the land puts it.
    spot: Option<Vec2>,
    /// True once the chief's record is asked for.
    asked: bool,
    /// The clock time an agent last called on the librarian.
    called: Option<f32>,
}

/// The library as it is drawn.
struct Stood {
    /// The middle of the ground it holds.
    held: Vec2,
    frame: Frame,
    house: Entity,
    glass: Entity,
    lamps: [(Entity, f32); 3],
    keeper: Rig,
    hair: Entity,
    staff: Entity,
    stone: Entity,
    windows: Handle<Material>,
    stone_look: Handle<Material>,
    flame: Handle<Material>,
    /// The steps of [`Stood::build`] taken, the meshes they built
    /// that are left to hand over, and the librarian's handed
    /// over.
    built: usize,
    pending: Vec<(Slot, MeshData)>,
    parts: [Option<Handle<Mesh>>; 8],
    wick: Option<Handle<Mesh>>,
    dressed: bool,
    /// The candle it was last lit for, and how much it last flared.
    candle: Option<f32>,
    flared: f32,
    /// The light over the dome that flares when the librarian is
    /// called on.
    beacon: Entity,
}

impl Library {
    /// The library on the spot the land puts it, beside the first
    /// town, its ground claimed and held level. None stands where
    /// the land has no room for it.
    pub fn raise(
        pal: &Palette,
        land: &mut Land,
        society: &mut Society,
        draw: &mut Draw,
    ) -> Result<Self, String> {
        let taken = society
            .towns
            .iter()
            .map(|town| land.sites[town.site].at)
            .chain(society.claims.iter().map(|claim| claim.at))
            .collect::<Vec<_>>();
        let stood = match place::stand(land, &taken) {
            Some(at) => {
                Some(Stood::raise(pal, land, society, at, draw)?)
            }
            None => None,
        };
        Ok(Self {
            stood,
            spot: None,
            asked: false,
            called: None,
        })
    }

    /// Takes the library up and stands it on `to`, where the chief
    /// set it down.
    fn resettle(
        &mut self,
        pal: &Palette,
        land: &mut Land,
        society: &mut Society,
        to: Vec2,
        draw: &mut Draw,
    ) -> Result<(), String> {
        if let Some(stood) = self.stood.take() {
            society
                .claims
                .retain(|claim| claim.place != Place::Library);
            let rounds = land
                .held()
                .into_iter()
                .filter(|&round| round != (stood.held, GROUNDS))
                .collect::<Vec<_>>();
            land.hold(&rounds);
            draw.despawn(stood.house);
        }
        self.stood =
            Some(Stood::raise(pal, land, society, to, draw)?);
        Ok(())
    }

    /// Where it stands, on the ground.
    pub fn at(&self) -> Option<Vec3> {
        self.stood.as_ref().map(|stood| stood.frame.at)
    }

    /// Builds what is left to build, lights it as the hour has it,
    /// and moves the librarian a frame further.
    pub fn update(&mut self, ctx: &Ctx, draw: &mut Draw) {
        let level =
            self.called.map_or(0.0, |at| flare(ctx.clock - at));
        if let Some(stood) = &mut self.stood {
            stood.update(ctx, level, draw);
        }
    }
}

impl Stood {
    /// The library on `at`, its ground claimed and held level.
    fn raise(
        pal: &Palette,
        land: &mut Land,
        society: &mut Society,
        at: Vec2,
        draw: &mut Draw,
    ) -> Result<Self, String> {
        society.claims.push(Claim {
            place: Place::Library,
            at,
            radius: land.set.town.keep_apart * 0.5,
        });
        let mut rounds = land.held();
        rounds.push((at, GROUNDS));
        rounds.sort_by(|a, b| {
            a.0.x.total_cmp(&b.0.x).then(a.0.y.total_cmp(&b.0.y))
        });
        land.hold(&rounds);
        // The door looks toward the first town.
        let home = land
            .sites
            .iter()
            .find(|site| site.buildable)
            .map_or(Vec2::ZERO, |site| site.at);
        let toward = (home - at).normalize_or(Vec2::Y);
        let frame = Frame::at(land.on_ground(at)).with_turn(
            Quat::from_rotation_y(toward.x.atan2(toward.y)),
        );

        let windows = draw.material(Pbr {
            emissive: rgba(pal.glass_lit * GLASS.0),
            ..Pbr::colored(rgba(pal.glass_lit))
        })?;
        let light = LOOK.light.colour(pal);
        let flame = draw.material(Pbr {
            unlit: true,
            fog: false,
            ..Pbr::colored(rgba(light))
        })?;
        let stone_look = draw.material(Pbr {
            emissive: rgba(light * STONE.0),
            ..Pbr::colored(rgba(light))
        })?;
        let key = Thing::Library.key();
        let house = draw.spawn(None, key);
        draw.place(house, frame);
        draw.watch(house);
        let glass = draw.spawn(Some(house), key);
        draw.shadows(glass, false, true);
        draw.watch(glass);
        // A lamp at each post and one under the porch, lit after
        // dark.
        let lamps = [
            (Vec3::new(3.5, 2.3, 6.3), LAMP),
            (Vec3::new(-3.5, 2.3, 6.3), LAMP),
            (Vec3::new(0.0, 3.6, 4.4), LAMP * 1.6),
        ]
        .map(|(at, lumens)| {
            let lamp = draw.spawn(Some(house), 0);
            draw.place(lamp, Frame::at(at));
            (lamp, lumens)
        });
        let beacon = draw.spawn(Some(house), 0);
        draw.place(beacon, Frame::at(Vec3::new(0.0, 10.2, 1.0)));
        // The librarian, on the porch before its doors.
        let keeper = Rig::make(key, draw);
        draw.commands.entity(keeper.root).insert(ChildOf(house));
        draw.place(keeper.root, Frame::at(STANDS));
        let hair = draw.spawn(Some(keeper.head()), key);
        let staff = draw.spawn(Some(keeper.figure()), key);
        let stone = draw.spawn(Some(keeper.figure()), 0);
        draw.shadows(stone, false, true);
        Ok(Self {
            held: at,
            frame,
            house,
            glass,
            lamps,
            keeper,
            hair,
            staff,
            stone,
            windows,
            stone_look,
            flame,
            built: 0,
            pending: Vec::new(),
            parts: Default::default(),
            wick: None,
            dressed: false,
            candle: None,
            flared: 0.0,
            beacon,
        })
    }

    /// Builds, lights and moves it a frame further, flaring `level`
    /// strong for a call on the librarian.
    fn update(&mut self, ctx: &Ctx, level: f32, draw: &mut Draw) {
        self.hand_over(ctx, draw);
        let candle = ctx.sky.look.candle;
        let glass = ctx.pal.glass_lit;
        if self.candle != Some(candle) || self.flared != level {
            // Dim by day, glowing after dark, and brighter still
            // while the librarian is called on.
            let (day, night) = GLASS;
            draw.set_material(
                &self.windows,
                Pbr {
                    emissive: rgba(
                        glass
                            * (day + night * candle + FLASH * level),
                    ),
                    ..Pbr::colored(rgba(glass))
                },
            );
        }
        if self.flared != level {
            let mut beacon = draw.commands.entity(self.beacon);
            if level > 0.0 {
                beacon.insert(SceneLight(Light::Point(PointLight {
                    color: rgba(ctx.pal.rune),
                    intensity: BEACON * level,
                    range: BEACON_REACH,
                    radius: 0.0,
                    shadows: false,
                })));
            } else {
                beacon.remove::<SceneLight>();
            }
            self.flared = level;
        }
        if self.candle != Some(candle) {
            for (lamp, lumens) in self.lamps {
                draw.commands.entity(lamp).insert(SceneLight(
                    Light::Point(PointLight {
                        color: rgba(ctx.pal.glass_lit),
                        intensity: lumens * candle,
                        range: LAMP_REACH,
                        radius: 0.0,
                        shadows: false,
                    }),
                ));
            }
            self.candle = Some(candle);
        }
        let set: &ChiefSettings = &ctx.tuning.chief;
        let doing = if level > 0.0 {
            Doing::Wave
        } else {
            Doing::Idle
        };
        self.keeper.place(
            &Pose::of(doing, ctx.clock),
            set.size * GROWN,
            LOOK.hat.crest(),
            draw,
        );
    }

    /// The meshes of step `step`: the house, then the librarian. None
    /// past the last.
    fn build(
        &self,
        step: usize,
        pal: &Palette,
    ) -> Option<Vec<(Slot, MeshData)>> {
        match step {
            0 => {
                let (walls, glass) = build::build(pal);
                Some(vec![
                    (Slot::Walls, walls.data()),
                    (Slot::Glass, glass.data()),
                ])
            }
            1 => {
                let (hair, staff, (stone, _)) = sage(pal);
                let mut out = parts(pal, LOOK)
                    .each()
                    .into_iter()
                    .enumerate()
                    .map(|(i, mb)| (Slot::Part(i), mb.data()))
                    .collect::<Vec<_>>();
                out.extend([
                    (Slot::Wick, wick().data()),
                    (Slot::Hair, hair.data()),
                    (Slot::Staff, staff.data()),
                    (Slot::Stone, stone.data()),
                ]);
                Some(out)
            }
            _ => None,
        }
    }

    /// Builds what meshes the frame has room for, a step at a time,
    /// hands them over, and draws each with what it is for.
    fn hand_over(&mut self, ctx: &Ctx, draw: &mut Draw) {
        while draw.left() {
            if self.pending.is_empty() {
                let Some(built) = self.build(self.built, ctx.pal)
                else {
                    break;
                };
                self.pending = built;
                self.built += 1;
                continue;
            }
            let (slot, data) = self.pending.remove(0);
            let mesh = match draw.mesh(data) {
                Ok(mesh) => mesh,
                Err(Unsent::Full(data)) => {
                    self.pending.insert(0, (slot, data));
                    break;
                }
                Err(Unsent::Refused(why)) => {
                    hamlet_engine_bevy::warn(&format!(
                        "the library: {why}"
                    ));
                    continue;
                }
            };
            let stock = ctx.stock;
            match slot {
                Slot::Walls => {
                    draw.draw(self.house, &mesh, &stock.matte)
                }
                Slot::Glass => {
                    draw.draw(self.glass, &mesh, &self.windows)
                }
                Slot::Part(i) => self.parts[i] = Some(mesh),
                Slot::Wick => self.wick = Some(mesh),
                Slot::Hair => {
                    draw.draw(self.hair, &mesh, &stock.matte)
                }
                Slot::Staff => {
                    draw.draw(self.staff, &mesh, &stock.matte)
                }
                Slot::Stone => {
                    draw.draw(self.stone, &mesh, &self.stone_look);
                    let (_, lumens, reach) = STONE;
                    let (.., (_, tip)) = sage(ctx.pal);
                    let glow = draw.spawn(Some(self.stone), 0);
                    draw.place(glow, Frame::at(tip));
                    draw.commands.entity(glow).insert(SceneLight(
                        Light::Point(PointLight {
                            color: rgba(LOOK.light.colour(ctx.pal)),
                            intensity: lumens,
                            range: reach,
                            radius: 0.0,
                            shadows: false,
                        }),
                    ));
                }
            }
        }
        // The librarian is dressed once all of its meshes are there.
        let parts = self.parts.iter().flatten().collect::<Vec<_>>();
        if !self.dressed
            && parts.len() == self.parts.len()
            && let Some(wick) = &self.wick
        {
            self.keeper.dress(
                &parts,
                &ctx.stock.matte,
                (wick, &self.flame),
                draw,
            );
            self.dressed = true;
        }
    }
}

/// Brightness of the library `since` seconds after a call: up at
/// once, wavering while the librarian attends, and down again.
fn flare(since: f32) -> f32 {
    if !(0.0..ATTENDS).contains(&since) {
        return 0.0;
    }
    let up = (since / 0.25).min(1.0);
    let down = ((ATTENDS - since) / 1.8).min(1.0);
    up * down * (0.82 + 0.18 * (since * 9.0).sin())
}

/// Stands the library where a chief sets it down, or where the
/// chief's record says one did, flares it when an agent calls on the
/// librarian, builds what is left of it, and moves its librarian a
/// frame further.
pub fn build_library(
    mut library: ResMut<Library>,
    mut replies: MessageReader<Replied>,
    mut events: MessageReader<AgentsEventReceived>,
    mut set_downs: MessageReader<SetDown>,
    mut world: ParamSet<(Shared, Grounds)>,
    mut draw: Draw,
) {
    if !std::mem::replace(&mut library.asked, true) {
        draw.commands.agents(&AgentsRequest::Chief);
    }
    let replied = replies.read().filter_map(|replied| match &replied
        .reply
    {
        Ok(Reply::Agents(AgentsReply::Chief(record))) => Some(record),
        _ => None,
    });
    let mut records = replied.cloned().collect::<Vec<_>>();
    let mut called = false;
    for event in events.read() {
        match &event.event {
            AgentsEvent::Chief(record) => {
                records.push(record.clone())
            }
            AgentsEvent::Consulted { .. } => called = true,
            _ => {}
        }
    }
    if let Some(record) = records.last() {
        library.spot = record.library.map(|(x, z)| Vec2::new(x, z));
    }
    // Set down by the chief: it stands there at once, and the home
    // keeps the spot.
    for set in set_downs.read() {
        if set.place == Place::Library {
            library.spot = Some(set.to);
            draw.commands.agents(&AgentsRequest::SetLibrary((
                set.to.x, set.to.y,
            )));
        }
    }
    if called {
        library.called = Some(world.p0().clock.now);
    }
    if let Some(to) = library.spot
        && library.stood.as_ref().is_none_or(|stood| stood.held != to)
    {
        let (pal, mut land, mut society) = world.p1();
        if let Err(why) = library.resettle(
            &pal,
            &mut land,
            &mut society,
            to,
            &mut draw,
        ) {
            hamlet_engine_bevy::warn(&format!("the library: {why}"));
        }
    }
    let shared = world.p0();
    let floor = shared.floor();
    library.update(&shared.ctx(&floor), &mut draw);
}
