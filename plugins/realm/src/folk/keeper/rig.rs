//! A robot put into the realm part by part, and posed each frame it
//! is seen by moving its parts' transforms: the engine has no
//! skinned meshes, so each part is an entity of its own.

use std::f32::consts::TAU;

use glam::{Vec2, Vec3};
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::scene::{
    Light, Material, Mesh, PointLight, StandardMaterial,
};

use crate::build::frame::Frame;
use crate::build::stock::{Stock, rgba};
use crate::build::upload::Draw;
use crate::folk::keeper::acts::{self, Act, Quirk};
use crate::folk::keeper::kit::Kit;
use crate::folk::keeper::kit::body::{KEY_AT, LAMP_DROP};
use crate::folk::keeper::kit::hats::SPIRE;
use crate::folk::keeper::pose::{
    self, FACE, HAT_AT, HIP, HOOK, LIT, NECK, Pose, Prop, SHOULDER,
    arc, ramp, rx, ry, rz,
};
use crate::folk::keeper::trip::Stride;
use crate::folk::keeper::who::Who;
use crate::folk::keeper::{KState, Motion};
use crate::folk::sounds::FolkSound;
use crate::world::tint::to_linear;

/// A clock time long before any, for a motion that has already
/// played out.
const LONG_AGO: f32 = -1000.0;

/// The glass of a robot's lights while they are out, and lit, in
/// sRGB.
const GLASS_OUT: Vec3 = Vec3::new(0.16, 0.15, 0.14);
const GLASS_LIT: Vec3 = Vec3::new(0.5, 0.46, 0.38);

/// How far the carried lantern's light reaches, and the size of its
/// flame.
const LANTERN_REACH: f32 = 5.0;
const LANTERN_FLAME: f32 = 0.05;

/// The parts a pose moves, in the order their last transforms are
/// kept.
#[derive(Clone, Copy)]
enum Slot {
    Root,
    Rig,
    Body,
    Head,
    Hat,
    EyeL,
    EyeR,
    LegL,
    LegR,
    WheelL,
    WheelR,
    ArmL,
    ArmR,
    Pole,
    Lamp,
    Key,
    Flag,
    Ring,
    Prop,
    Glint,
}

const SLOTS: usize = 20;

/// The entities a pose moves.
struct Parts {
    root: Entity,
    rig: Entity,
    body: Entity,
    head: Entity,
    hat: Entity,
    eye_l: Entity,
    eye_r: Entity,
    leg_l: Entity,
    leg_r: Entity,
    wheel_l: Entity,
    wheel_r: Entity,
    arm_l: Entity,
    arm_r: Entity,
    pole: Entity,
    lamp: Entity,
    key: Entity,
    flag: Option<Entity>,
    ring: Entity,
    /// The light of the carried lantern.
    light: Entity,
    prop: Entity,
    glint: Entity,
}

/// One robot as it is drawn: its parts, its motion and how it moves.
pub struct Rig {
    pub state: KState,
    who: Who,
    quirk: Quirk,
    act: Act,
    /// Clock time the motion began.
    since: f32,
    /// The pose it was in when the motion began, and the motion it
    /// left.
    from: Option<(Pose, Act)>,
    /// The state the motion was last chosen for.
    synced: KState,
    gait: f32,
    pace: f32,
    /// Its change from stepping, 0, to rolling, 1.
    roll: f32,
    /// Clock time it was last drawn; none before it ever was.
    seen: Option<f32>,
    /// The walk someone else moves it along.
    strode: Option<Stride>,
    /// Turn and lift of its head toward something it watches, on top
    /// of its motion.
    look: Vec2,
    /// Where it stands in the realm.
    pub at: Frame,
    parts: Parts,
    /// The material of both lanterns and the ring.
    lit: Handle<Material>,
    lit_as: Vec3,
    /// The lumens its lantern's light was last given at full
    /// strength, and the glow it was given for. None while it has
    /// no light.
    lantern: Option<(f32, Vec3)>,
    /// The prop shown, and whether each part that shows or hides is
    /// shown.
    shown_prop: Prop,
    on: [bool; 2],
    visible: bool,
    /// The transform each part was last given.
    last: [Option<Frame>; SLOTS],
}

/// A pivot carrying a part's meshes. The first mesh rides on the
/// pivot itself, the second on a child of it.
fn part(
    parent: Entity,
    key: u32,
    at: Vec3,
    meshes: [(Option<&Handle<Mesh>>, &Handle<Material>); 2],
    draw: &mut Draw,
) -> Entity {
    let pivot = draw.spawn(Some(parent), key);
    draw.place(pivot, Frame::at(at));
    let mut on_pivot = false;
    for (mesh, material) in meshes {
        let Some(mesh) = mesh else { continue };
        if on_pivot {
            let child = draw.spawn(Some(pivot), key);
            draw.draw(child, mesh, material);
            draw.watch(child);
        } else {
            draw.draw(pivot, mesh, material);
            draw.watch(pivot);
            on_pivot = true;
        }
    }
    pivot
}

/// An entity drawing `mesh` under `parent` at `at`, which casts no
/// shadow when `shadowless`.
fn piece(
    parent: Entity,
    key: u32,
    at: Vec3,
    mesh: Option<&Handle<Mesh>>,
    material: &Handle<Material>,
    shadowless: bool,
    draw: &mut Draw,
) -> Entity {
    let entity = draw.spawn(Some(parent), key);
    draw.place(entity, Frame::at(at));
    if let Some(mesh) = mesh {
        draw.draw(entity, mesh, material);
        draw.watch(entity);
    }
    if shadowless {
        draw.shadows(entity, false, true);
    }
    entity
}

impl Rig {
    /// A robot standing at `at`, settled in the plain motion of
    /// `state`. Its entities all have the key `key`.
    pub fn make(
        kit: &Kit,
        stock: &Stock,
        who: Who,
        at: Frame,
        state: KState,
        key: u32,
        draw: &mut Draw,
    ) -> Result<Self, String> {
        // One material for both lanterns and the ring, so they show
        // the same state.
        let lit = draw.material(StandardMaterial {
            perceptual_roughness: 0.4,
            ..StandardMaterial::colored(rgba(to_linear(Vec3::new(
                0.2, 0.19, 0.17,
            ))))
        })?;
        let mesh = |at: usize| kit.mesh(at);
        let (matte, brass) = (&stock.matte, &stock.brass);
        let root = draw.spawn(None, key);
        draw.place(root, at);
        let rig = draw.spawn(Some(root), key);
        draw.place(
            rig,
            Frame::IDENTITY.with_size(Vec3::splat(who.size)),
        );
        let paint = &kit.paints[who.paint as usize];
        let body = part(
            rig,
            key,
            Vec3::Y * HIP,
            [(mesh(kit.torso.0), paint), (mesh(kit.torso.1), brass)],
            draw,
        );
        let mut legs = Vec::new();
        for x in [-0.11, 0.11] {
            legs.push(part(
                rig,
                key,
                Vec3::new(x, HIP, 0.0),
                [(mesh(kit.leg.0), matte), (mesh(kit.leg.1), brass)],
                draw,
            ));
        }
        let axle = Pose::stand().place(1.0).axle;
        let wheel_l = piece(
            legs[0],
            key,
            axle,
            mesh(kit.wheels),
            brass,
            false,
            draw,
        );
        let wheel_r = piece(
            legs[1],
            key,
            axle,
            mesh(kit.wheels),
            brass,
            false,
            draw,
        );
        let mut arms = Vec::new();
        for side in [-1.0, 1.0] {
            arms.push(part(
                body,
                key,
                SHOULDER * Vec3::new(side, 1.0, 1.0),
                [(mesh(kit.arm.0), matte), (mesh(kit.arm.1), brass)],
                draw,
            ));
        }
        piece(
            body,
            key,
            Vec3::ZERO,
            mesh(kit.emblems[who.emblem as usize]),
            matte,
            true,
            draw,
        );
        let key_part = piece(
            body,
            key,
            KEY_AT,
            mesh(kit.key),
            brass,
            false,
            draw,
        );
        if who.leader {
            piece(
                body,
                key,
                Vec3::ZERO,
                mesh(kit.mantle),
                matte,
                false,
                draw,
            );
        }
        let head = piece(
            body,
            key,
            Vec3::Y * NECK,
            mesh(kit.head),
            brass,
            false,
            draw,
        );
        piece(
            head,
            key,
            Vec3::Y * FACE,
            mesh(kit.glass),
            &lit,
            true,
            draw,
        );
        let eye_l = piece(
            head,
            key,
            Vec3::Y * FACE,
            mesh(kit.eye),
            matte,
            true,
            draw,
        );
        let eye_r = piece(
            head,
            key,
            Vec3::Y * FACE,
            mesh(kit.eye),
            matte,
            true,
            draw,
        );
        let worn = kit.hats[who.hat as usize];
        let hat = part(
            head,
            key,
            Vec3::Y * HAT_AT,
            [(mesh(worn.0), paint), (mesh(worn.1), brass)],
            draw,
        );
        let flag = who.leader.then(|| {
            piece(
                hat,
                key,
                Vec3::Y * (SPIRE - 0.02),
                mesh(kit.pennant),
                matte,
                false,
                draw,
            )
        });
        let pole = part(
            rig,
            key,
            Pose::stand().pole_at,
            [(mesh(kit.pole.0), matte), (mesh(kit.pole.1), brass)],
            draw,
        );
        let lamp = piece(
            pole,
            key,
            HOOK,
            mesh(kit.lamp),
            brass,
            false,
            draw,
        );
        piece(
            lamp,
            key,
            LAMP_DROP,
            mesh(kit.lamp_glass),
            &lit,
            true,
            draw,
        );
        let ring = piece(
            lamp,
            key,
            LAMP_DROP,
            mesh(kit.ring),
            &lit,
            true,
            draw,
        );
        draw.show(ring, false);
        // The light goes with the carried lantern, which leads.
        let light = draw.spawn(Some(lamp), key);
        draw.place(
            light,
            Frame::at(LAMP_DROP + Vec3::new(0.0, 0.0, 0.12)),
        );
        let prop = piece(
            arms[0],
            key,
            Vec3::ZERO,
            mesh(kit.props[0]),
            matte,
            false,
            draw,
        );
        draw.show(prop, false);
        let glint =
            piece(prop, key, Vec3::ZERO, None, &lit, true, draw);
        let [leg_l, leg_r] = [legs.remove(0), legs.remove(0)];
        let [arm_l, arm_r] = [arms.remove(0), arms.remove(0)];
        Ok(Self {
            state,
            who,
            quirk: Quirk::of(who.seed),
            act: Act::of(state),
            since: LONG_AGO,
            from: None,
            synced: state,
            gait: 0.0,
            pace: 0.0,
            roll: 0.0,
            seen: None,
            strode: None,
            look: Vec2::ZERO,
            at,
            parts: Parts {
                root,
                rig,
                body,
                head,
                hat,
                eye_l,
                eye_r,
                leg_l,
                leg_r,
                wheel_l,
                wheel_r,
                arm_l,
                arm_r,
                pole,
                lamp,
                key: key_part,
                flag,
                ring,
                light,
                prop,
                glint,
            },
            lit,
            lit_as: Vec3::splat(-1.0),
            lantern: None,
            shown_prop: Prop::None,
            on: [false, false],
            visible: true,
            last: [None; SLOTS],
        })
    }

    pub fn who(&self) -> Who {
        self.who
    }

    pub fn act(&self) -> Act {
        self.act
    }

    /// Has its carried lantern throw light, `lumens` strong when it
    /// burns lit up, as its glow last was; none takes the light away.
    pub fn lantern(&mut self, lumens: Option<f32>, draw: &mut Draw) {
        let want = lumens.map(|lumens| (lumens, self.lit_as));
        if self.lantern == want {
            return;
        }
        self.lantern = want;
        let entity = self.parts.light;
        match want {
            Some((lumens, glow)) => {
                let strength = glow.max_element().max(0.0);
                let hue = glow / strength.max(1e-4);
                draw.commands.entity(entity).insert(SceneLight(
                    Light::Point(PointLight {
                        color: rgba(hue),
                        intensity: strength / LIT * lumens,
                        range: LANTERN_REACH,
                        radius: LANTERN_FLAME,
                        shadow_maps_enabled: false,
                    }),
                ));
            }
            None => {
                draw.commands.entity(entity).remove::<SceneLight>();
            }
        }
    }

    /// Turns its head by `turn` and lifts it by `lift`, in radians,
    /// on top of whatever its motion does. Zero looks nowhere.
    pub fn look(&mut self, turn: f32, lift: f32) {
        self.look = Vec2::new(turn, lift);
    }

    /// The sound its motion makes, and a count that passes a whole
    /// number each time it makes it.
    pub fn beat(&self, now: f32) -> Option<(FolkSound, f32)> {
        acts::beat(self.act, &self.ctx(now))
    }

    /// Begins a motion at clock time `now`, easing out of the pose it
    /// is in then.
    pub fn play(&mut self, act: Act, now: f32) {
        self.start(act, now);
        self.state = act.state();
        self.synced = self.state;
    }

    /// Takes up a motion that began at clock time `since`. With
    /// `now` it eases out of the pose it is in then; without, it is
    /// as if it had been in the motion all along.
    pub fn take_up(
        &mut self,
        act: Act,
        since: f32,
        now: Option<f32>,
    ) {
        self.from = now.map(|now| (self.eval(now), self.act));
        self.act = act;
        self.since = since;
        self.state = act.state();
        self.synced = self.state;
    }

    /// Sets the stride of a robot that is being moved over the
    /// ground. None has it walk on the spot.
    pub fn stride(&mut self, walked: Option<Stride>) {
        self.strode = walked;
    }

    /// True when a motion that takes the robot out of sight has
    /// finished.
    pub fn gone(&self, now: f32) -> bool {
        self.act.vanishes()
            && self
                .act
                .length()
                .is_some_and(|l| now - self.since >= l)
    }

    fn start(&mut self, act: Act, now: f32) {
        self.from = Some((self.eval(now), self.act));
        self.act = act;
        self.since = now;
    }

    /// Takes up a motion as if it had always been in it.
    fn settle(&mut self, act: Act) {
        self.from = None;
        self.act = act;
        self.since = LONG_AGO;
        self.state = act.state();
        self.synced = self.state;
    }

    fn ctx(&self, now: f32) -> acts::Ctx {
        acts::Ctx {
            t: now * self.quirk.tempo + self.quirk.phase,
            tau: now - self.since,
            gait: self.gait,
            pace: self.pace,
            roll: ramp(0.0, 1.0, self.roll),
            q: self.quirk,
        }
    }

    fn eval(&self, now: f32) -> Pose {
        let mut p = acts::pose(self.act, &self.ctx(now));
        if self.look != Vec2::ZERO {
            // The eyes go a little further than the head.
            p.head = ry(self.look.x) * p.head * rx(-self.look.y);
            p.gaze.x += 0.25 * self.look.x;
        }
        let Some((from, was)) = &self.from else {
            return p;
        };
        let (len, dip) = self.act.easing(*was);
        let u = (now - self.since) / len.max(1e-4);
        if u >= 1.0 {
            return p;
        }
        let mut p = from.mix(&p, ramp(0.0, 1.0, u));
        // A small dip between two motions, as a breath before the
        // next.
        let d = dip * arc(0.0, 1.0, u);
        p.hips.y -= 0.02 * d;
        p.squash *= 1.0 - 0.03 * d;
        p
    }

    /// Puts the robot where `at` says.
    pub fn place(&mut self, at: Frame) {
        self.at = at;
    }

    /// Shows or hides the whole robot.
    pub fn show(&mut self, visible: bool, draw: &mut Draw) {
        if self.visible != visible {
            self.visible = visible;
            draw.show(self.parts.root, visible);
        }
    }

    /// Takes the whole robot away.
    pub fn despawn(&self, draw: &mut Draw) {
        draw.despawn(self.parts.root);
    }

    /// Moves the robot a frame on to clock time `now`, and poses it
    /// when `seen`.
    pub fn pose(
        &mut self,
        now: f32,
        motion: &Motion,
        kit: &Kit,
        stock: &Stock,
        seen: bool,
        draw: &mut Draw,
    ) {
        if self.state != self.synced {
            let act = Act::of(self.state);
            if self.seen.is_none() {
                self.settle(act);
            } else {
                self.synced = self.state;
                self.start(act, now);
            }
        }
        let stride = motion.stride * self.who.size;
        let passed = self.seen.map_or(0.0, |seen| (now - seen).abs());
        let first = self.seen.is_none();
        // Without a stride it walks on the spot.
        let walked = self.strode.unwrap_or(Stride {
            gone: now * motion.walk_speed,
            pace: 1.0,
            speed: motion.walk_speed,
        });
        self.gait = walked.gone / stride * TAU;
        self.pace = walked.pace;
        let rolls = f32::from(walked.speed > motion.roll_above);
        self.roll = if first {
            rolls
        } else {
            let most = passed / motion.roll_ease.max(1e-4);
            self.roll + (rolls - self.roll).clamp(-most, most)
        };
        if let Some(len) = self.act.length()
            && now - self.since >= len
            && let Some(next) = self.act.then()
        {
            let at = self.since + len;
            self.start(next, at);
        }
        if let Some((_, was)) = &self.from
            && now - self.since >= self.act.easing(*was).0
        {
            self.from = None;
        }
        // A pose is a function of the clock alone, so a robot out of
        // sight is left as it was and picks up when it is seen again.
        if !seen && !first {
            return;
        }
        self.seen = Some(now);
        let p = self.eval(now);
        let placed = p.place(self.who.size);
        let (parts, last) = (&self.parts, &mut self.last);
        let mut put = |slot: Slot, entity: Entity, to: Frame| {
            let was = &mut last[slot as usize];
            if *was != Some(to) {
                *was = Some(to);
                draw.place(entity, to);
            }
        };
        put(Slot::Root, parts.root, self.at);
        let posed = [
            (Slot::Rig, parts.rig, placed.rig),
            (Slot::Body, parts.body, placed.body),
            (Slot::Head, parts.head, placed.head),
            (Slot::Hat, parts.hat, placed.hat),
            (Slot::EyeL, parts.eye_l, placed.eye_l),
            (Slot::EyeR, parts.eye_r, placed.eye_r),
            (Slot::LegL, parts.leg_l, placed.leg_l),
            (Slot::LegR, parts.leg_r, placed.leg_r),
            (Slot::Pole, parts.pole, placed.pole),
            (
                Slot::ArmL,
                parts.arm_l,
                Frame::at(SHOULDER * Vec3::new(-1.0, 1.0, 1.0))
                    .with_turn(p.arm_l),
            ),
            (
                Slot::ArmR,
                parts.arm_r,
                Frame::at(SHOULDER).with_turn(p.arm_r),
            ),
            (
                Slot::Lamp,
                parts.lamp,
                Frame::at(HOOK).with_turn(placed.lamp),
            ),
            (
                Slot::Key,
                parts.key,
                Frame::at(KEY_AT).with_turn(rz(p.key)),
            ),
            (Slot::Glint, parts.glint, Frame::at(Vec3::Y * -p.bead)),
        ];
        for (slot, entity, to) in posed {
            put(slot, entity, to);
        }
        // The wheels turn by the ground covered, so they never slip.
        let turned = walked.gone / (pose::WHEEL * self.who.size);
        for (slot, entity, ahead) in [
            (Slot::WheelL, parts.wheel_l, 0.0),
            (Slot::WheelR, parts.wheel_r, 0.9),
        ] {
            let to = Frame::at(placed.axle)
                .with_turn(rx((turned + ahead) % TAU));
            put(slot, entity, to);
        }
        if let Some(flag) = parts.flag {
            // The pennant trails, and droops a little.
            let to = Frame::at(Vec3::Y * (SPIRE - 0.02))
                .with_turn(ry(p.flag) * rx(-0.12));
            put(Slot::Flag, flag, to);
        }
        let sized = [
            (Slot::Ring, 0, parts.ring, LAMP_DROP, p.ring),
            (Slot::Prop, 1, parts.prop, Vec3::ZERO, p.prop_size),
        ];
        for (slot, _, entity, at, size) in sized {
            let to =
                Frame::at(at).with_size(Vec3::splat(size.max(0.02)));
            put(slot, entity, to);
        }
        for (_, which, entity, _, size) in sized {
            let on = size > 0.02;
            if self.on[which] != on {
                self.on[which] = on;
                draw.show(entity, on);
            }
        }
        if p.prop != Prop::None && p.prop != self.shown_prop {
            let i = p.prop as usize - 1;
            if let Some(mesh) = kit.mesh(kit.props[i]) {
                draw.draw(parts.prop, mesh, &stock.matte);
            }
            match kit.mesh(kit.glints[i]) {
                Some(mesh) => draw.draw(parts.glint, mesh, &self.lit),
                None => draw.undraw(parts.glint),
            }
            self.shown_prop = p.prop;
        }
        if (p.glow - self.lit_as).abs().max_element() > 2e-3 {
            self.lit_as = p.glow;
            let lit = (p.glow.max_element() / 2.0).clamp(0.0, 1.0);
            let glass = to_linear(GLASS_OUT.lerp(GLASS_LIT, lit));
            draw.set_material(
                &self.lit,
                StandardMaterial {
                    emissive: rgba(p.glow),
                    perceptual_roughness: 0.4,
                    ..StandardMaterial::colored(rgba(glass))
                },
            );
        }
    }
}
