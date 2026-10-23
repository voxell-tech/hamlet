//! A robot carried in the hand, and its fall when it is let go.

use glam::{Quat, Vec2, Vec3, Vec3Swizzles};

use crate::folk::hand::{
    Hand, HandFrame, Held, agent_of, on_level, toward, yaw_of,
};
use crate::folk::keeper::{Act, Passage, Robot, Robots};
use crate::folk::sounds::FolkSound;
use crate::society::{AgentId, Pair, Society};
use crate::view::pick::Thing;

/// A fall to the ground, and what comes after it.
#[derive(Clone, Copy, Debug)]
pub struct Fall {
    speed: f32,
    /// The point of the ground it comes down on.
    to: Vec2,
    /// The robot it was set down to meet.
    meets: Option<AgentId>,
}

/// True when `agent` holds the office of its hamlet.
fn mayor(society: &Society, agent: AgentId) -> bool {
    society
        .agent(agent)
        .and_then(|agent| society.town(agent.town))
        .is_some_and(|town| town.leader == agent)
}

/// Moves what the hand holds, and lets it go when the button is. A
/// robot set down on another's robot or house meets it, and gives
/// the two agents to be joined by a road.
pub(super) fn carry(
    frame: &HandFrame,
    box_height: f32,
    hovered: Option<Thing>,
    society: &Society,
    hand: &mut Hand,
    robots: &mut Robots,
    watched: &mut Vec<(AgentId, bool)>,
    cue: &mut impl FnMut(FolkSound, Vec3, u32),
) -> Option<(AgentId, AgentId)> {
    let set = frame.set;
    let Held { agent, lifted } = hand.held?;
    let released = hand.released || !hand.down;
    let dragged = hand.travel > frame.click_slop;
    let Some(robot) = robots.robot_mut(agent) else {
        hand.held = None;
        return None;
    };
    if !lifted && dragged && !released {
        hand.held = Some(Held {
            agent,
            lifted: true,
        });
        robot.mood.face = None;
        robot.mood.joins = None;
        robot.passage = Some(Passage::hold(Act::Dangle, frame.now));
        // The pointer must reach what is under it.
        watched.push((agent, false));
        cue(FolkSound::Lift, robot.rig.at.at, agent.0);
    }
    let at = &mut robot.rig.at;
    if lifted && !released {
        let ground = Robot::ground(frame.land, at.at.xz());
        let high = ground + set.lift_height;
        let want = on_level(frame.ray, high + box_height)
            .map_or(at.at.with_y(high), |p| p.with_y(high));
        let k = 1.0 - (-set.lift_follow * frame.dt).exp();
        let was = at.at;
        at.at = was.lerp(want, k);
        // It swings behind the hand that carries it.
        let speed = (at.at - was) / frame.dt.max(1e-4);
        let yaw = Quat::from_rotation_y(yaw_of(at.turn));
        let own = yaw.inverse() * speed;
        let lean = |v: f32| (v * set.lift_lean).clamp(-0.6, 0.6);
        let hung = yaw
            * Quat::from_rotation_x(-lean(own.z))
            * Quat::from_rotation_z(lean(own.x));
        at.turn = at.turn.slerp(hung, k);
    }
    if !released {
        return None;
    }
    let (here, home) = (at.at, robot.porch());
    hand.held = None;
    hand.released = false;
    if !lifted {
        return None;
    }
    let other = agent_of(hovered).filter(|other| *other != agent);
    let mut fall = Fall {
        speed: 0.0,
        to: here.xz(),
        meets: None,
    };
    let mut wish = None;
    if let Some(other) = other {
        let known = society.road(Pair::new(agent, other)).is_some();
        let office = mayor(society, agent) || mayor(society, other);
        if !known && !office {
            wish = Some((agent, other));
        }
        // It comes down a step from the other, on the side it came
        // from.
        let theirs = robots.robot(other);
        let them = theirs.map_or(here, |robot| robot.rig.at.at);
        let aside = (here.xz() - them.xz())
            .try_normalize()
            .or((home.xz() - them.xz()).try_normalize())
            .unwrap_or(Vec2::X);
        fall.to = them.xz() + aside * set.greet_gap;
        fall.meets = theirs.map(|robot| robot.agent);
    }
    if let Some(robot) = robots.robot_mut(agent) {
        robot.mood.fall = Some(fall);
    }
    wish
}

/// Brings each robot that was let go down to the ground, where it
/// lands and makes for home, or greets the robot it was set down to
/// meet.
pub(super) fn fall(
    frame: &HandFrame,
    robots: &mut Robots,
    watched: &mut Vec<(AgentId, bool)>,
    cue: &mut impl FnMut(FolkSound, Vec3, u32),
) {
    let (set, now, dt) = (frame.set, frame.now, frame.dt);
    let mut greeted = Vec::new();
    for robot in robots.each_mut() {
        let Some(mut fall) = robot.mood.fall else {
            continue;
        };
        fall.speed += set.fall * dt;
        let k = 1.0 - (-set.lift_follow * dt).exp();
        let at = &mut robot.rig.at;
        let over = at.at.xz().lerp(fall.to, k);
        let ground = Robot::ground(frame.land, over);
        let height = at.at.y - fall.speed * dt;
        at.at = Vec3::new(over.x, height.max(ground), over.y);
        let upright = Quat::from_rotation_y(yaw_of(at.turn));
        at.turn = at.turn.slerp(upright, k);
        if height > ground {
            robot.mood.fall = Some(fall);
            continue;
        }
        robot.mood.fall = None;
        at.turn = upright;
        at.at = Vec3::new(fall.to.x, ground, fall.to.y);
        let here = at.at;
        // The longer the way home, the faster it goes, up to a
        // point.
        let far = here.distance(robot.porch());
        let haste = (far / set.scurry_longest)
            .clamp(set.scurry, set.scurry_fastest.max(set.scurry));
        robot.put_down(Some(haste));
        watched.push((robot.agent, true));
        let (act, sound) = match fall.meets {
            Some(_) => (Act::Greet, FolkSound::Introduce),
            None => (Act::Land, FolkSound::Drop),
        };
        robot.passage = Some(Passage::new(act, now));
        cue(sound, here, robot.agent.0);
        if let Some(other) = fall.meets {
            greeted.push((robot.agent, other, here));
        }
    }
    for (comer, other, here) in greeted {
        let Some(robot) = robots.robot_mut(other) else {
            continue;
        };
        let there = robot.rig.at.at;
        if robot.free() && !robot.mood.sulks(now) {
            robot.mood.joins = Some((now, Act::Greet));
            robot.mood.face = Some(toward(there, here));
        }
        if let Some(robot) = robots.robot_mut(comer) {
            robot.mood.face = Some(toward(here, there));
        }
    }
}
