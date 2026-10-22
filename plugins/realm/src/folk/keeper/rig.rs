//! A robot put into the realm part by part, and posed each frame it
//! is seen by moving its parts' transforms: the engine has no
//! skinned meshes, so each part is an entity of its own.

use std::f32::consts::TAU;

use glam::Vec3;
use hamlet_engine_sdk::scene::{Entity, Material, Pbr};
use looks::wardrobe::Clip;

use crate::build::frame::Frame;
use crate::build::stock::{Stock, rgba};
use crate::folk::keeper::acts::{self, Act, Quirk};
use crate::folk::keeper::kit::Kit;
use crate::folk::keeper::kit::body::{KEY_AT, LAMP_DROP};
use crate::folk::keeper::kit::hats::SPIRE;
use crate::folk::keeper::pose::{
    self, FACE, HAT_AT, HIP, HOOK, NECK, Pose, Prop, SHOULDER, arc,
    ramp, rx, ry, rz,
};
use crate::folk::keeper::trip::Stride;
use crate::folk::keeper::wear::{
    Dress, Gesture, Mat, Part, WearSettings, moved, relight,
};
use crate::folk::keeper::who::Who;
use crate::folk::keeper::{KState, Motion};
use crate::world::tint::to_linear;

/// A clock time long before any, for a motion that has already
/// played out.
const LONG_AGO: f32 = -1000.0;

/// The glass of a robot's lights while they are out, and lit, in
/// sRGB.
const GLASS_OUT: Vec3 = Vec3::new(0.16, 0.15, 0.14);
const GLASS_LIT: Vec3 = Vec3::new(0.5, 0.46, 0.38);

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
    prop: Entity,
    glint: Entity,
}

impl Parts {
    /// The entity of `slot`.
    fn slot(&self, slot: Slot) -> Option<&Entity> {
        Some(match slot {
            Slot::Root => &self.root,
            Slot::Rig => &self.rig,
            Slot::Body => &self.body,
            Slot::Head => &self.head,
            Slot::Hat => &self.hat,
            Slot::EyeL => &self.eye_l,
            Slot::EyeR => &self.eye_r,
            Slot::LegL => &self.leg_l,
            Slot::LegR => &self.leg_r,
            Slot::WheelL => &self.wheel_l,
            Slot::WheelR => &self.wheel_r,
            Slot::ArmL => &self.arm_l,
            Slot::ArmR => &self.arm_r,
            Slot::Pole => &self.pole,
            Slot::Lamp => &self.lamp,
            Slot::Key => &self.key,
            Slot::Flag => return self.flag.as_ref(),
            Slot::Ring => &self.ring,
            Slot::Prop => &self.prop,
            Slot::Glint => &self.glint,
        })
    }
}

/// One robot as it is drawn: its parts, its motion and how it moves.
pub(super) struct Rig {
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
    /// Where it stands in the realm.
    pub at: Frame,
    parts: Parts,
    /// Entities no pose moves, kept so that they stay.
    _held: Vec<Entity>,
    /// Each mesh of the kit drawn, and whether it is dressed in a
    /// look.
    shapes: Vec<Shape>,
    dressed: bool,
    /// The material of both lanterns and the ring.
    lit: Material,
    lit_as: Vec3,
    /// The colour of its glass, in sRGB, and of its lamp at rest, as
    /// a skin sets them.
    glass: Option<Vec3>,
    lamp: Option<Vec3>,
    /// The clip it wears, and its playing.
    clip: Option<Clip>,
    gesture: Gesture,
    /// The prop shown, and whether each part that shows or hides is
    /// shown.
    shown_prop: Prop,
    on: [bool; 2],
    visible: bool,
    /// The transform each part was last given.
    last: [Option<Frame>; SLOTS],
}

/// A mesh of the kit drawn on a robot: the entity it is on, the part
/// it is of, the material it is drawn with by default and its place
/// in the kit.
struct Shape {
    on: On,
    part: Part,
    mat: Mat,
    mesh: usize,
}

/// The entity a [`Shape`] is drawn on: a part a pose moves, or one
/// kept in `_held`.
#[derive(Clone, Copy)]
enum On {
    Posed(Slot),
    Held(usize),
}

/// A mesh of the kit at `at`, as `part` draws it with `mat`.
type Drawn = (usize, Mat);

/// What a robot is made with: its kit, the materials it is drawn
/// with, and the key of its entities.
struct Making<'a> {
    kit: &'a Kit,
    stock: &'a Stock,
    paint: &'a Material,
    lit: &'a Material,
    key: u32,
    held: Vec<Entity>,
    shapes: Vec<Shape>,
}

impl Making<'_> {
    fn material(&self, mat: Mat) -> &Material {
        match mat {
            Mat::Paint => self.paint,
            Mat::Matte => &self.stock.matte,
            Mat::Brass => &self.stock.brass,
            Mat::Lit => self.lit,
        }
    }

    /// A pivot carrying `part`'s meshes, kept as `on`. The first mesh
    /// rides on the pivot itself, the second on a child of it.
    fn part(
        &mut self,
        parent: &Entity,
        at: Vec3,
        (part, on): (Part, On),
        meshes: [Drawn; 2],
    ) -> Result<Entity, String> {
        let pivot = Entity::make(Some(parent), self.key)?;
        pivot.set_transform(Frame::at(at).scene());
        let mut on_pivot = false;
        for (mesh, mat) in meshes {
            let Some(drawn) = self.kit.mesh(mesh) else {
                continue;
            };
            let material = self.material(mat);
            let on = if on_pivot {
                let child = Entity::make(Some(&pivot), self.key)?;
                child.draw(drawn, material);
                child.watch(true, true);
                self.held.push(child);
                On::Held(self.held.len() - 1)
            } else {
                pivot.draw(drawn, material);
                pivot.watch(true, true);
                on_pivot = true;
                on
            };
            self.shapes.push(Shape {
                on,
                part,
                mat,
                mesh,
            });
        }
        Ok(pivot)
    }

    /// An entity drawing the mesh `drawn` under `parent` at `at`,
    /// which casts no shadow when `shadowless`. A mesh of `part` is
    /// kept as `on`.
    fn piece(
        &mut self,
        parent: &Entity,
        at: Vec3,
        shape: Option<(Part, On)>,
        (mesh, mat): Drawn,
        shadowless: bool,
    ) -> Result<Entity, String> {
        let entity = Entity::make(Some(parent), self.key)?;
        entity.set_transform(Frame::at(at).scene());
        if let Some(drawn) = self.kit.mesh(mesh) {
            entity.draw(drawn, self.material(mat));
            entity.watch(true, true);
            if let Some((part, on)) = shape {
                self.shapes.push(Shape {
                    on,
                    part,
                    mat,
                    mesh,
                });
            }
        }
        if shadowless {
            entity.set_shadows(false, true);
        }
        Ok(entity)
    }

    /// A [`Making::piece`] kept in `held`.
    fn keep(
        &mut self,
        parent: &Entity,
        at: Vec3,
        part: Part,
        drawn: Drawn,
        shadowless: bool,
    ) -> Result<(), String> {
        let on = On::Held(self.held.len());
        let entity = self.piece(
            parent,
            at,
            Some((part, on)),
            drawn,
            shadowless,
        )?;
        self.held.push(entity);
        Ok(())
    }
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
    ) -> Result<Self, String> {
        let root = Entity::make(None, key)?;
        root.set_transform(at.scene());
        let rig = Entity::make(Some(&root), key)?;
        rig.set_transform(
            Frame::IDENTITY.with_size(Vec3::splat(who.size)).scene(),
        );
        // One material for both lanterns and the ring, so they show
        // the same state.
        let lit = Material::make(Pbr {
            roughness: 0.4,
            ..Pbr::colored(rgba(to_linear(GLASS_OUT)))
        })?;
        let mut m = Making {
            kit,
            stock,
            paint: &kit.paints[who.paint as usize],
            lit: &lit,
            key,
            held: Vec::new(),
            shapes: Vec::new(),
        };
        let (paint, matte, brass, glow) =
            (Mat::Paint, Mat::Matte, Mat::Brass, Mat::Lit);
        let posed =
            |part: Part, slot: Slot| Some((part, On::Posed(slot)));
        let body = m.part(
            &rig,
            Vec3::Y * HIP,
            (Part::Torso, On::Posed(Slot::Body)),
            [(kit.torso.0, paint), (kit.torso.1, brass)],
        )?;
        let axle = Pose::stand().place(1.0).axle;
        let mut legs = Vec::new();
        let mut wheels = Vec::new();
        for (x, (part, slot), (wheel, turns)) in [
            (
                -0.11,
                (Part::LegL, Slot::LegL),
                (Part::WheelsL, Slot::WheelL),
            ),
            (
                0.11,
                (Part::LegR, Slot::LegR),
                (Part::WheelsR, Slot::WheelR),
            ),
        ] {
            let leg = m.part(
                &rig,
                Vec3::new(x, HIP, 0.0),
                (part, On::Posed(slot)),
                [(kit.leg.0, matte), (kit.leg.1, brass)],
            )?;
            wheels.push(m.piece(
                &leg,
                axle,
                posed(wheel, turns),
                (kit.wheels, brass),
                false,
            )?);
            legs.push(leg);
        }
        let mut arms = Vec::new();
        for (side, part, slot) in [
            (-1.0, Part::ArmL, Slot::ArmL),
            (1.0, Part::ArmR, Slot::ArmR),
        ] {
            arms.push(m.part(
                &body,
                SHOULDER * Vec3::new(side, 1.0, 1.0),
                (part, On::Posed(slot)),
                [(kit.arm.0, matte), (kit.arm.1, brass)],
            )?);
        }
        m.keep(
            &body,
            Vec3::ZERO,
            Part::Emblem,
            (kit.emblems[who.emblem as usize], matte),
            true,
        )?;
        let key_part = m.piece(
            &body,
            KEY_AT,
            posed(Part::Key, Slot::Key),
            (kit.key, brass),
            false,
        )?;
        if who.leader {
            m.keep(
                &body,
                Vec3::ZERO,
                Part::Mantle,
                (kit.mantle, matte),
                false,
            )?;
        }
        let head = m.piece(
            &body,
            Vec3::Y * NECK,
            posed(Part::Head, Slot::Head),
            (kit.head, brass),
            false,
        )?;
        m.keep(
            &head,
            Vec3::Y * FACE,
            Part::Glass,
            (kit.glass, glow),
            true,
        )?;
        let eye_l = m.piece(
            &head,
            Vec3::Y * FACE,
            posed(Part::EyeL, Slot::EyeL),
            (kit.eye, matte),
            true,
        )?;
        let eye_r = m.piece(
            &head,
            Vec3::Y * FACE,
            posed(Part::EyeR, Slot::EyeR),
            (kit.eye, matte),
            true,
        )?;
        let worn = kit.hats[who.hat as usize];
        let hat = m.part(
            &head,
            Vec3::Y * HAT_AT,
            (Part::Hat, On::Posed(Slot::Hat)),
            [(worn.0, paint), (worn.1, brass)],
        )?;
        let flag = if who.leader {
            Some(m.piece(
                &hat,
                Vec3::Y * (SPIRE - 0.02),
                posed(Part::Pennant, Slot::Flag),
                (kit.pennant, matte),
                false,
            )?)
        } else {
            None
        };
        let pole = m.part(
            &rig,
            Pose::stand().pole_at,
            (Part::Pole, On::Posed(Slot::Pole)),
            [(kit.pole.0, matte), (kit.pole.1, brass)],
        )?;
        let lamp = m.piece(
            &pole,
            HOOK,
            posed(Part::Lamp, Slot::Lamp),
            (kit.lamp, brass),
            false,
        )?;
        m.keep(
            &lamp,
            LAMP_DROP,
            Part::LampGlass,
            (kit.lamp_glass, glow),
            true,
        )?;
        let ring =
            m.piece(&lamp, LAMP_DROP, None, (kit.ring, glow), true)?;
        ring.set_visible(false);
        let prop = m.piece(
            &arms[0],
            Vec3::ZERO,
            None,
            (kit.props[0], matte),
            false,
        )?;
        prop.set_visible(false);
        let glint = Entity::make(Some(&prop), key)?;
        glint.set_shadows(false, true);
        let Making { held, shapes, .. } = m;
        let [leg_l, leg_r] = [legs.remove(0), legs.remove(0)];
        let [wheel_l, wheel_r] = [wheels.remove(0), wheels.remove(0)];
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
                prop,
                glint,
            },
            _held: held,
            shapes,
            dressed: false,
            lit,
            lit_as: Vec3::splat(-1.0),
            glass: None,
            lamp: None,
            clip: None,
            gesture: Gesture::default(),
            shown_prop: Prop::None,
            on: [false, false],
            visible: true,
            last: [None; SLOTS],
        })
    }

    /// Draws the robot in `dress`, and has it play `clip`. A part a
    /// worn mesh stands for is drawn in its first mesh's place and
    /// its other meshes are left out.
    pub fn dress(
        &mut self,
        kit: &Kit,
        stock: &Stock,
        dress: Dress,
        clip: Option<&Clip>,
    ) {
        self.clip = clip.cloned();
        if !self.dressed
            && dress.skin.is_none()
            && dress.mesh.is_none()
        {
            return;
        }
        self.dressed = dress.skin.is_some() || dress.mesh.is_some();
        let mut done = Vec::new();
        for shape in &self.shapes {
            let entity = match shape.on {
                On::Posed(slot) => self.parts.slot(slot),
                On::Held(i) => self._held.get(i),
            };
            let Some(entity) = entity else {
                continue;
            };
            let default = match shape.mat {
                Mat::Paint => &kit.paints[self.who.paint as usize],
                Mat::Matte => &stock.matte,
                Mat::Brass => &stock.brass,
                Mat::Lit => &self.lit,
            };
            let swap =
                dress.mesh.and_then(|parts| parts.of(shape.part));
            match swap {
                Some(_) if done.contains(&shape.part) => {
                    entity.undraw()
                }
                Some(mesh) => {
                    done.push(shape.part);
                    let material = match shape.mat {
                        Mat::Lit => &self.lit,
                        _ => &stock.matte,
                    };
                    entity.draw(mesh, material);
                }
                None => {
                    let material = dress
                        .skin
                        .and_then(|skin| {
                            skin.material(
                                shape.part,
                                shape.mat,
                                self.who.paint,
                            )
                        })
                        .unwrap_or(default);
                    if let Some(mesh) = kit.mesh(shape.mesh) {
                        entity.draw(mesh, material);
                    }
                }
            }
        }
        self.glass = dress.skin.and_then(|skin| skin.glass);
        self.lamp = dress.skin.and_then(|skin| skin.lamp);
        // Lights it again, in the skin's colours.
        self.lit_as = Vec3::splat(-1.0);
    }

    /// Plays the clip it wears from clock time `now`.
    pub fn gesture(&mut self, now: f32) {
        if self.clip.is_some() {
            self.gesture.play(now);
        }
    }

    pub fn who(&self) -> Who {
        self.who
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
        let p = acts::pose(self.act, &self.ctx(now));
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
    pub fn show(&mut self, visible: bool) {
        if self.visible != visible {
            self.visible = visible;
            self.parts.root.set_visible(visible);
        }
    }

    /// Moves the robot a frame on to clock time `now`, and poses it
    /// when `seen`.
    pub fn pose(
        &mut self,
        now: f32,
        (motion, wear): (&Motion, &WearSettings),
        kit: &Kit,
        stock: &Stock,
        seen: bool,
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
        let mut placed = p.place(self.who.size);
        let mut arm_l =
            Frame::at(SHOULDER * Vec3::new(-1.0, 1.0, 1.0))
                .with_turn(p.arm_l);
        let mut arm_r = Frame::at(SHOULDER).with_turn(p.arm_r);
        if let Some(clip) = &self.clip {
            let idle = self.act == Act::Idle && self.from.is_none();
            if let Some(moves) = self.gesture.moves(
                clip,
                now,
                idle,
                self.who.seed,
                wear,
            ) {
                // In the order of `PARTS`.
                moved(
                    [
                        &mut placed.rig,
                        &mut placed.body,
                        &mut placed.head,
                        &mut placed.hat,
                        &mut arm_l,
                        &mut arm_r,
                        &mut placed.leg_l,
                        &mut placed.leg_r,
                    ],
                    moves,
                );
            }
        }
        let (parts, last) = (&self.parts, &mut self.last);
        put(last, Slot::Root, &parts.root, self.at);
        let posed = [
            (Slot::Rig, &parts.rig, placed.rig),
            (Slot::Body, &parts.body, placed.body),
            (Slot::Head, &parts.head, placed.head),
            (Slot::Hat, &parts.hat, placed.hat),
            (Slot::EyeL, &parts.eye_l, placed.eye_l),
            (Slot::EyeR, &parts.eye_r, placed.eye_r),
            (Slot::LegL, &parts.leg_l, placed.leg_l),
            (Slot::LegR, &parts.leg_r, placed.leg_r),
            (Slot::Pole, &parts.pole, placed.pole),
            (Slot::ArmL, &parts.arm_l, arm_l),
            (Slot::ArmR, &parts.arm_r, arm_r),
            (
                Slot::Lamp,
                &parts.lamp,
                Frame::at(HOOK).with_turn(placed.lamp),
            ),
            (
                Slot::Key,
                &parts.key,
                Frame::at(KEY_AT).with_turn(rz(p.key)),
            ),
            (Slot::Glint, &parts.glint, Frame::at(Vec3::Y * -p.bead)),
        ];
        for (slot, entity, to) in posed {
            put(last, slot, entity, to);
        }
        // The wheels turn by the ground covered, so they never slip.
        let turned = walked.gone / (pose::WHEEL * self.who.size);
        for (slot, entity, ahead) in [
            (Slot::WheelL, &parts.wheel_l, 0.0),
            (Slot::WheelR, &parts.wheel_r, 0.9),
        ] {
            let to = Frame::at(placed.axle)
                .with_turn(rx((turned + ahead) % TAU));
            put(last, slot, entity, to);
        }
        if let Some(flag) = &parts.flag {
            // The pennant trails, and droops a little.
            let to = Frame::at(Vec3::Y * (SPIRE - 0.02))
                .with_turn(ry(p.flag) * rx(-0.12));
            put(last, Slot::Flag, flag, to);
        }
        for (slot, which, entity, at, size) in [
            (Slot::Ring, 0, &parts.ring, LAMP_DROP, p.ring),
            (Slot::Prop, 1, &parts.prop, Vec3::ZERO, p.prop_size),
        ] {
            let on = size > 0.02;
            if self.on[which] != on {
                self.on[which] = on;
                entity.set_visible(on);
            }
            let to =
                Frame::at(at).with_size(Vec3::splat(size.max(0.02)));
            put(last, slot, entity, to);
        }
        if p.prop != Prop::None && p.prop != self.shown_prop {
            let i = p.prop as usize - 1;
            if let Some(mesh) = kit.mesh(kit.props[i]) {
                parts.prop.draw(mesh, &stock.matte);
            }
            match kit.mesh(kit.glints[i]) {
                Some(mesh) => parts.glint.draw(mesh, &self.lit),
                None => parts.glint.undraw(),
            }
            self.shown_prop = p.prop;
        }
        let glow = relight(p.glow, self.lamp);
        if (glow - self.lit_as).abs().max_element() > 2e-3 {
            self.lit_as = glow;
            self.lit.set(lit(glow, self.glass));
        }
    }
}

/// The look of a robot's lanterns and ring glowing `glow`, their
/// glass of `glass` in sRGB as a skin sets it.
pub fn lit(glow: Vec3, glass: Option<Vec3>) -> Pbr {
    let lit = (glow.max_element() / 2.0).clamp(0.0, 1.0);
    // A skin's glass pales toward white as it is lit.
    let (out, bright) = match glass {
        Some(c) => (c, c.lerp(Vec3::ONE, 0.35)),
        None => (GLASS_OUT, GLASS_LIT),
    };
    let glass = to_linear(out.lerp(bright, lit));
    Pbr {
        emissive: rgba(glow),
        roughness: 0.4,
        ..Pbr::colored(rgba(glass))
    }
}

/// Sets the transform of `slot`'s `entity` to `to`, unless `last`
/// says it has it.
fn put(
    last: &mut [Option<Frame>; SLOTS],
    slot: Slot,
    entity: &Entity,
    to: Frame,
) {
    let was = &mut last[slot as usize];
    if *was != Some(to) {
        *was = Some(to);
        entity.set_transform(to.scene());
    }
}
