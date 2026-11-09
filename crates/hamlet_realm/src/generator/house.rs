//! Houses: one mesh per agent, put down by a ruleset from the traits
//! the agent's id picks, so each agent's house is its own and always
//! the same. Hamlet's own ruleset is the book `house.ron`.

use std::collections::BTreeMap;
use std::f32::consts::FRAC_PI_2;
use std::sync::Arc;

use bevy::math::Affine3A;
use bevy::prelude::*;
use hamlet_blueprint::{Object, Seed, Seeds, Value, evaluate};
use hamlet_graph::{Book, Registry, Ruleset, Shape};

use super::flora;
use super::land::Land;
use super::noise::key;
use super::society::AgentId;
use super::yard::{self, Yard};
use crate::meshes::{Meshing, Rng};
use crate::objects::{self, Paints, Target};
use crate::palette::Palette;
pub use crate::palette::TownStyle;

/// Hamlet's own book of the house, which makes every house until a
/// realm's host chooses another.
pub const BOOK: &str = include_str!("house.ron");

/// The remains of an archived agent's house.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Deserialize)]
pub enum Remains {
    /// The base, broken walls and moss.
    Ruin,
    /// Nothing but the ground.
    Bare,
}

/// The build of a house.
#[derive(Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HouseSettings {
    /// Height of the stone base.
    pub base: f32,
    /// Overhang of the roof past the walls.
    pub eave: f32,
    /// The remains of a removed agent's house.
    pub removed: Remains,
    /// The remains of a house merged into another.
    pub merged: Remains,
}

impl TownStyle {
    /// The style of the town on a site. Neighbouring sites differ.
    pub fn of(land: &Land, site: usize) -> Self {
        let towns = &land.pal.towns;
        let shift =
            (key(&[land.seed, 40]) % towns.len() as u32) as usize;
        towns[(site + shift) % towns.len()]
    }
}

/// The ruleset houses are made by, and the kinds of node it is
/// checked against.
#[derive(Resource, Clone)]
pub struct HouseRules(Arc<(Ruleset, Registry)>);

impl HouseRules {
    pub fn new(ruleset: Ruleset) -> Self {
        Self(Arc::new((ruleset, hamlet_blueprint::registry())))
    }
}

impl Default for HouseRules {
    /// Hamlet's own, from [`BOOK`].
    fn default() -> Self {
        Self::new(
            Book::from_ron(BOOK)
                .expect("Hamlet's own house book reads")
                .ruleset(),
        )
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Roof {
    /// Ridge across the front.
    Gable,
    /// Ridge front to back, so the gable faces the road.
    Fronted,
    Hip,
    /// A long slope at the front and a short one behind.
    Saltbox,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Wall {
    Plaster,
    Ochre,
    Whitewash,
    Stone,
    Framed,
}

/// The traits that make one house unlike the next.
#[derive(Clone, Copy, Debug)]
pub struct Look {
    half: Vec2,
    storeys: u32,
    roof: Roof,
    wall: Wall,
    livery: Vec3,
    pitch: f32,
    /// -1 or 1: the side the chimney and the garden are on.
    side: f32,
    garden: u32,
    /// Height of the stone base.
    base: f32,
    /// Overhang of the roof past the walls.
    eave: f32,
}

impl Look {
    pub fn of(land: &Land, agent: AgentId, hall: bool) -> Self {
        let pick = |salt: u32, n: u32| {
            (key(&[land.seed, 41, agent.0, salt]) % n) as usize
        };
        let livery = land.pal.livery[pick(3, 8)];
        let (base, eave) = (land.set.house.base, land.set.house.eave);
        if hall {
            return Self {
                half: Vec2::new(2.0, 1.4),
                storeys: 2,
                roof: Roof::Gable,
                wall: Wall::Stone,
                livery,
                pitch: 1.15,
                side: 1.0,
                garden: 4,
                base,
                eave,
            };
        }
        Self {
            half: Vec2::new(
                1.25 + 0.14 * pick(0, 4) as f32,
                1.05 + 0.13 * pick(1, 3) as f32,
            ),
            storeys: 1 + (pick(2, 5) / 3) as u32,
            roof: [
                Roof::Gable,
                Roof::Fronted,
                Roof::Hip,
                Roof::Saltbox,
            ][pick(4, 4)],
            wall: [
                Wall::Plaster,
                Wall::Ochre,
                Wall::Whitewash,
                Wall::Stone,
                Wall::Framed,
            ][pick(5, 5)],
            livery,
            pitch: 0.95 + 0.15 * pick(6, 3) as f32,
            side: if pick(7, 2) == 0 { -1.0 } else { 1.0 },
            garden: pick(8, 4) as u32,
            base,
            eave,
        }
    }

    fn wall_top(&self) -> f32 {
        self.base + 1.45 + 1.15 * (self.storeys - 1) as f32
    }

    fn wall_colour(&self, pal: &Palette) -> Vec3 {
        match self.wall {
            Wall::Plaster | Wall::Framed => pal.plaster,
            Wall::Ochre => pal.ochre,
            Wall::Whitewash => pal.whitewash,
            Wall::Stone => pal.limestone,
        }
    }

    /// The colours the house's assets take.
    fn paints<'a>(
        &self,
        pal: &'a Palette,
        style: &TownStyle,
    ) -> Paints<'a> {
        let wall = self.wall_colour(pal);
        Paints {
            palette: pal,
            style: *style,
            livery: self.livery,
            wall,
            // Stone walls carry plaster above.
            upper_wall: if self.storeys > 1
                && self.wall == Wall::Stone
            {
                pal.plaster
            } else {
                wall
            },
        }
    }

    /// What the house's ruleset is given: these traits, and what the
    /// game knows of the house besides.
    fn inputs(
        &self,
        hall: bool,
        banner: Banner,
        wings: u8,
        yard: &Yard,
    ) -> BTreeMap<String, Value> {
        let number = |value: f32| Value::Number(value);
        let whole = |value: u32| Value::Integer(value as i32);
        [
            ("half_x", number(self.half.x)),
            ("half_z", number(self.half.y)),
            ("base", number(self.base)),
            ("eave", number(self.eave)),
            ("pitch", number(self.pitch)),
            ("side", number(self.side)),
            ("yard_side", number(yard.side)),
            ("storeys", whole(self.storeys)),
            ("roof", whole(self.roof as u32)),
            ("wall", whole(self.wall as u32)),
            ("garden", whole(self.garden)),
            ("wings", whole(u32::from(wings))),
            ("hall", Value::Bool(hall)),
            ("banner", Value::Bool(banner == Banner::Flown)),
        ]
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value))
        .collect()
    }
}

/// The work yard of an agent's house, whose door is turned `yaw`
/// from +Z.
pub fn yard_of(
    land: &Land,
    agent: AgentId,
    hall: bool,
    yaw: f32,
    wings: u8,
) -> Yard {
    Yard::new(
        Look::of(land, agent, hall).half,
        yaw,
        hall,
        wings,
        land.set.yard,
    )
}

/// The tree in a house's garden, if it has one: where it stands
/// about the house, which kind and how big. It keeps to the side
/// away from the yard.
pub(crate) fn garden_tree(
    land: &Land,
    agent: AgentId,
    yard: &Yard,
) -> Option<(Vec3, usize, f32)> {
    let look = Look::of(land, agent, false);
    let h = key(&[land.seed, 42, agent.0]);
    (h % 10 < 7).then(|| {
        (
            Vec3::new(
                -yard.side * (look.half.x + 0.25),
                0.0,
                -look.half.y - 0.55,
            ),
            // Any broadleaf, each as likely as another.
            flora::form(
                (h / 16) as usize % flora::BROADLEAVES,
                (h / 1024) as usize,
            ),
            0.55 + 0.3 * ((h / 256 % 100) as f32 / 100.0),
        )
    })
}

/// A house's meshes, about the middle of its plot with the door
/// toward +Z.
pub struct HouseMesh {
    pub body: Meshing,
    /// The window panes, which light at night.
    pub panes: Meshing,
    /// The place the lantern by the door hangs at, where the ruleset
    /// marks a lamp.
    pub lamp: Option<Vec3>,
    /// The point the mayor's banner hangs from, and the colours of
    /// its stripes in turn.
    pub banner: Option<(Vec3, [Vec3; 2])>,
}

/// Length and height of the mayor's banner.
pub(crate) const BANNER: Vec2 = Vec2::new(0.95, 0.55);

/// Whether a house flies the mayor's banner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Banner {
    Flown,
    None,
}

/// The house of `agent` in the town on `site`, with its work yard,
/// as `rules` put it down. `wings` is the number of houses merged
/// into it. A ruleset that fails to run puts down nothing but the
/// yard, and says why.
pub fn house(
    land: &Land,
    rules: &HouseRules,
    agent: AgentId,
    site: usize,
    hall: bool,
    banner: Banner,
    wings: u8,
    yard: &Yard,
) -> HouseMesh {
    let pal = &land.pal;
    let style = &TownStyle::of(land, site);
    let look = Look::of(land, agent, hall);
    let (ruleset, registry) = &*rules.0;
    let realm = Seed(u64::from(land.seed));
    let seeds = Seeds {
        realm,
        hamlet: realm.mix(site as u64),
        item: realm.mix(u64::from(agent.0)),
    };
    let objects = match evaluate(
        registry,
        ruleset,
        seeds,
        &look.inputs(hall, banner, wings, yard),
    ) {
        Ok(mut made) => match made.remove("objects") {
            Some(Value::Objects(objects)) => objects,
            _ => {
                warn!("the house ruleset gives no objects");
                Vec::new()
            }
        },
        Err(error) => {
            warn!("the house ruleset did not run: {error}");
            Vec::new()
        }
    };
    let mut body = Meshing::new();
    let mut panes = Meshing::new();
    let marks = draw(
        &mut body,
        &mut panes,
        ruleset,
        &objects,
        &look.paints(pal, style),
        seeds.item,
    );
    let mut rng =
        Rng::seeded(&format!("house {} {}", land.seed, agent.0));
    path(&mut body, &mut rng, land, &look);
    yard::pave(&mut body, &mut rng, yard, pal);
    HouseMesh {
        body,
        panes,
        lamp: marks.get("lamp").copied(),
        banner: marks
            .get("banner")
            .map(|at| (*at, [style.banner, style.roof])),
    }
}

/// Draws `objects` as `ruleset`'s catalogue has their assets, into
/// `body` and `panes`. Gives where each mark was put, by name.
fn draw(
    body: &mut Meshing,
    panes: &mut Meshing,
    ruleset: &Ruleset,
    objects: &[Object],
    paints: &Paints,
    seed: Seed,
) -> BTreeMap<String, Vec3> {
    let mut marks = BTreeMap::new();
    let mut target = Target {
        body,
        panes: Some(panes),
    };
    for object in objects {
        let asset = ruleset.assets.get(&object.asset);
        if asset
            .as_ref()
            .is_some_and(|asset| asset.shape == Shape::Mark)
        {
            marks.insert(
                object.asset.clone(),
                object.frame.translation,
            );
            continue;
        }
        objects::stand(
            &mut target,
            &object.frame,
            &object.asset,
            asset.as_ref(),
            None,
            Some(paints),
            seed.mix(object.id.0).0,
        );
    }
    marks
}

/// Flagstones from the door out to where the roads end.
fn path(
    meshing: &mut Meshing,
    rng: &mut Rng,
    land: &Land,
    look: &Look,
) {
    let pal = &land.pal;
    let (base, door_out) = (look.base, land.set.town.door_out);
    let hz = look.half.y;
    let door_x = -look.side * look.half.x * 0.3;
    let mut z = hz + 0.3;
    let mut k = 0;
    while z < door_out + 0.2 {
        let len = rng.range(0.42, 0.6);
        let t = ((z - hz) / (door_out - hz)).clamp(0.0, 1.0);
        for side in [-1.0f32, 1.0] {
            meshing.rbox(
                Vec3::new(
                    door_x * (1.0 - t) + side * 0.3,
                    base - 0.36 * t.min(1.0) - 0.05,
                    z,
                ),
                Vec3::new(0.27, 0.07, len * 0.5 - 0.02),
                0.03,
                rng.wobble(1.4),
                pal.limestone
                    .lerp(pal.limestone_shade, rng.f() * 0.4),
                0.3,
            );
        }
        z += len;
        k += 1;
        if k > 6 {
            break;
        }
    }
}

/// The remains of an archived agent's house: the base, broken walls
/// and moss, on the same footprint.
pub fn ruin(land: &Land, agent: AgentId, hall: bool) -> Meshing {
    let (pal, seed) = (&land.pal, land.seed);
    let look = Look::of(land, agent, hall);
    let base = look.base;
    let mut rng = Rng::seeded(&format!("ruin {seed} {}", agent.0));
    let mut b = Meshing::new();
    let (hx, hz) = (look.half.x, look.half.y);
    let grey = |c: Vec3, rng: &mut Rng| {
        c.lerp(pal.rock, 0.55).lerp(pal.moss, rng.range(0.0, 0.45))
    };
    b.rbox(
        Vec3::new(0.0, base * 0.5 - 0.3, 0.0),
        Vec3::new(hx + 0.06, base * 0.5 + 0.3, hz + 0.06),
        0.05,
        Quat::IDENTITY,
        grey(pal.limestone_shade, &mut rng),
        0.35,
    );
    // Stubs of wall around the edge, highest at the corners.
    let wall = look.wall_colour(pal);
    let around = [
        (Vec2::new(-hx, hz), Vec2::new(hx, hz)),
        (Vec2::new(hx, hz), Vec2::new(hx, -hz)),
        (Vec2::new(hx, -hz), Vec2::new(-hx, -hz)),
        (Vec2::new(-hx, -hz), Vec2::new(-hx, hz)),
    ];
    for (a, c) in around {
        let pieces = 4;
        for i in 0..pieces {
            let (t0, t1) = (
                i as f32 / pieces as f32,
                (i + 1) as f32 / pieces as f32,
            );
            let corner = (t0 + t1 - 1.0).abs();
            let h =
                rng.range(0.1, 0.5) + corner * rng.range(0.3, 1.0);
            // The doorway stays open.
            if a.y == hz && c.y == hz && i == 1 {
                continue;
            }
            let (p, q) = (a.lerp(c, t0), a.lerp(c, t1));
            let mid = (p + q) * 0.5;
            let d = q - p;
            b.rbox(
                Vec3::new(mid.x, base + h * 0.5, mid.y),
                Vec3::new(d.length() * 0.5, h * 0.5, 0.09),
                0.03,
                Quat::from_rotation_y(-d.y.atan2(d.x))
                    * rng.wobble(1.5),
                grey(wall, &mut rng),
                0.25,
            );
        }
    }
    // Fallen stones, and moss taking the floor.
    for _ in 0..7 {
        let r = rng.range(0.1, 0.22);
        let col = grey(pal.limestone, &mut rng);
        b.rbox(
            Vec3::new(
                rng.sym(hx + 0.6),
                base * rng.range(0.0, 1.0) + r * 0.5 - 0.1,
                rng.sym(hz + 0.6),
            ),
            Vec3::new(r, r * 0.7, r * 0.8),
            0.03,
            rng.wobble(25.0),
            col,
            0.3,
        );
    }
    for _ in 0..3 {
        let c = Vec3::new(rng.sym(hx * 0.7), base, rng.sym(hz * 0.7));
        let r = rng.range(0.25, 0.4);
        objects::bush(
            &mut b,
            &mut rng,
            pal.foliage,
            pal.foliage_shade,
            c,
            r,
        );
    }
    b
}

/// A house as a few flat faces, for towns seen from far away.
pub fn far(
    meshing: &mut Meshing,
    panes: &mut Meshing,
    xf: Affine3A,
    land: &Land,
    agent: AgentId,
    style: &TownStyle,
    hall: bool,
    remains: Option<Remains>,
) {
    if remains == Some(Remains::Bare) {
        return;
    }
    let pal = &land.pal;
    let look = Look::of(land, agent, hall);
    let (base, eave) = (look.base, look.eave);
    let (hx, hz) = (look.half.x, look.half.y);
    let wall = look.wall_colour(pal);
    meshing.at(xf);
    if remains.is_some() {
        meshing.rbox(
            Vec3::Y * 0.2,
            Vec3::new(hx, 0.45, hz),
            0.05,
            Quat::IDENTITY,
            wall.lerp(pal.rock, 0.6),
            0.3,
        );
        meshing.at(Affine3A::IDENTITY);
        return;
    }
    let top = look.wall_top();
    let p = |x: f32, y: f32, z: f32| Vec3::new(x, y, z);
    for (a, c, out) in [
        (p(-hx, 0.0, hz), p(hx, 0.0, hz), Vec3::Z),
        (p(hx, 0.0, hz), p(hx, 0.0, -hz), Vec3::X),
        (p(hx, 0.0, -hz), p(-hx, 0.0, -hz), -Vec3::Z),
        (p(-hx, 0.0, -hz), p(-hx, 0.0, hz), -Vec3::X),
    ] {
        let up = Vec3::Y * top;
        meshing.face_cols(
            [a - Vec3::Y * 0.3, c - Vec3::Y * 0.3, c + up, a + up],
            out,
            [wall * 0.7, wall * 0.7, wall, wall],
        );
    }
    let rise = look.pitch * (hz + look.eave);
    let fronted = look.roof == Roof::Fronted;
    for side in [-1.0f32, 1.0] {
        let (a, c, apex, out) = if fronted {
            (
                p(-hx, top, side * hz),
                p(hx, top, side * hz),
                p(0.0, top + rise, side * hz),
                Vec3::Z * side,
            )
        } else {
            (
                p(side * hx, top, -hz),
                p(side * hx, top, hz),
                p(side * hx, top + rise, 0.0),
                Vec3::X * side,
            )
        };
        meshing.face([a, c, apex, apex], out, wall);
    }
    panes.at(xf);
    for (c, face) in [
        (p(0.4, base + 0.82, hz + 0.02), Quat::IDENTITY),
        (
            p(hx + 0.02, base + 0.82, 0.0),
            Quat::from_rotation_y(FRAC_PI_2),
        ),
        (
            p(-hx - 0.02, base + 0.82, 0.0),
            Quat::from_rotation_y(-FRAC_PI_2),
        ),
    ] {
        let at = |x: f32, y: f32| c + face * Vec3::new(x, y, 0.0);
        panes.face(
            [
                at(-0.2, -0.27),
                at(0.2, -0.27),
                at(0.2, 0.27),
                at(-0.2, 0.27),
            ],
            face * Vec3::Z,
            Vec3::ONE,
        );
    }
    panes.at(Affine3A::IDENTITY);
    let (ex, ez) = (hx + look.eave, hz + eave);
    let col = style.roof.lerp(style.drift, 0.4);
    let (y0, y1) = (top - 0.08, top - 0.08 + rise);
    if fronted {
        for side in [-1.0f32, 1.0] {
            meshing.face_cols(
                [
                    p(side * ex, y0, ez),
                    p(side * ex, y0, -ez),
                    p(0.0, y1, -ez),
                    p(0.0, y1, ez),
                ],
                Vec3::new(side, 1.0, 0.0),
                [col * 0.85, col * 0.85, col, col],
            );
        }
    } else {
        for side in [-1.0f32, 1.0] {
            meshing.face_cols(
                [
                    p(-ex, y0, side * ez),
                    p(ex, y0, side * ez),
                    p(ex, y1, 0.0),
                    p(-ex, y1, 0.0),
                ],
                Vec3::new(0.0, 1.0, side),
                [col * 0.85, col * 0.85, col, col],
            );
        }
    }
    meshing.at(Affine3A::IDENTITY);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generator::testing::land;

    #[test]
    fn hamlets_own_book_builds_every_kind_of_house() {
        let land = land(7);
        let rules = HouseRules::default();
        let (ruleset, registry) = &*rules.0;
        let kinds = [
            (false, Banner::None, 0),
            (false, Banner::Flown, 2),
            (true, Banner::Flown, 0),
        ];
        for agent in (0..40).map(AgentId) {
            for (hall, banner, wings) in kinds {
                let look = Look::of(&land, agent, hall);
                let yard = yard_of(&land, agent, hall, 0.0, wings);
                let inputs = look.inputs(hall, banner, wings, &yard);
                if let Err(error) = evaluate(
                    registry,
                    ruleset,
                    Seeds::default(),
                    &inputs,
                ) {
                    panic!("{look:?} did not run: {error}");
                }
                let built = house(
                    &land, &rules, agent, 0, hall, banner, wings,
                    &yard,
                );
                assert!(built.lamp.is_some(), "{look:?} has no lamp");
                assert!(
                    !built.panes.is_empty(),
                    "{look:?} has no panes"
                );
                assert_eq!(
                    built.banner.is_some(),
                    banner == Banner::Flown,
                    "{look:?}"
                );
            }
        }
    }
}
