//! Clicks and strokes on a robot: a poke, a pat and a cheer.

use std::f32::consts::PI;

use glam::Vec3;

use crate::folk::hand::toys::{Kind, Toys};
use crate::folk::hand::{Hand, HandFrame, Rest, toward};
use crate::folk::keeper::{Act, KState, Passage, Robots};
use crate::folk::sounds::FolkSound;
use crate::view::pick::Thing;

/// Answers each click on a robot: a cheer for one whose work has
/// just ended, and a poke for the second click of two.
pub(super) fn pokes(
    frame: &HandFrame,
    hand: &mut Hand,
    robots: &mut Robots,
    toys: &mut Toys,
    cue: &mut impl FnMut(FolkSound, Vec3, u32),
) {
    let (set, now) = (frame.set, frame.now);
    for tapped in std::mem::take(&mut hand.taps) {
        let Some(robot) = robots.robot_mut(tapped) else {
            continue;
        };
        let at = robot.rig.at.at;
        if !robot.free() || robot.mood.sulks(now) {
            continue;
        }
        let proud =
            robot.mood.proud_until.is_some_and(|until| now < until);
        if proud {
            robot.mood.proud_until = None;
            hand.last_tap = None;
            cue(FolkSound::Cheer, at, tapped.0);
            let top = at + Vec3::Y * set.heart_height;
            for i in 0..3 {
                let side = Vec3::X * (i as f32 - 1.0) * 0.3;
                toys.toss(
                    Kind::Spark,
                    now + i as f32 * 0.12,
                    (top + side, top + side * 2.0 + Vec3::Y),
                    i as f32,
                );
            }
            // Its neighbours join in, the nearest first.
            for robot in robots.each_mut() {
                let far = robot.rig.at.at.distance(at);
                if robot.agent == tapped {
                    robot.mood.joins = Some((now, Act::Celebrate));
                } else if far < set.cheer_reach
                    && robot.free()
                    && !robot.mood.sulks(now)
                {
                    let wait = 0.25 + far * set.cheer_lag;
                    robot.mood.joins =
                        Some((now + wait, Act::Celebrate));
                }
            }
            continue;
        }
        let second = hand.last_tap.is_some_and(|(agent, then)| {
            agent == tapped && now - then <= set.poke_gap
        });
        if !second {
            hand.last_tap = Some((tapped, now));
            continue;
        }
        hand.last_tap = None;
        let mood = &mut robot.mood;
        mood.pokes.retain(|then| now - then <= set.annoy_window);
        mood.pokes.push(now);
        let to_eye = toward(at, frame.eye);
        if mood.pokes.len() >= set.annoy_pokes {
            // It turns its back on whoever keeps poking it.
            mood.pokes.clear();
            mood.sulks_until =
                now + Act::Sulk.length().unwrap_or_default();
            mood.face = Some(to_eye + PI);
            robot.passage = Some(Passage::new(Act::Sulk, now));
            cue(FolkSound::Annoy, at, tapped.0);
        } else {
            mood.face = Some(to_eye);
            robot.passage = Some(Passage::new(Act::Startle, now));
            cue(FolkSound::Poke, at, tapped.0);
        }
    }
}

/// Pats the robot the pointer rests on, or strokes back and forth.
pub(super) fn pats(
    frame: &HandFrame,
    hovered: Option<Thing>,
    hand: &mut Hand,
    robots: &mut Robots,
    toys: &mut Toys,
    cue: &mut impl FnMut(FolkSound, Vec3, u32),
) {
    let (set, now) = (frame.set, frame.now);
    let on = match hovered {
        Some(Thing::Robot(agent)) if hand.held.is_none() => {
            Some(agent)
        }
        _ => None,
    };
    let (Some(on), Some(at)) = (on, hand.pointer) else {
        hand.rest = None;
        return;
    };
    let fresh = Rest {
        agent: on,
        anchor: at,
        since: now,
        last: at,
        began: now,
        path: 0.0,
        heading: 0.0,
        turns: 0,
    };
    let mut rest =
        hand.rest.filter(|rest| rest.agent == on).unwrap_or(fresh);
    if rest.anchor.distance(at) > set.pat_still {
        rest.anchor = at;
        rest.since = now;
    }
    let step = at - rest.last;
    rest.last = at;
    if now - rest.began > set.stroke_time {
        (rest.began, rest.path, rest.turns) = (now, 0.0, 0);
    }
    rest.path += step.length();
    if step.x.abs() > 0.5 {
        if rest.heading * step.x < 0.0 {
            rest.turns += 1;
        }
        rest.heading = step.x.signum();
    }
    let Some(robot) = robots.robot_mut(on) else {
        hand.rest = None;
        return;
    };
    let since = now - robot.mood.patted;
    let held =
        now - rest.since >= set.pat_hold && since >= set.pat_rest;
    let stroked = rest.path >= set.stroke_length
        && rest.turns >= set.stroke_turns
        && since >= set.stroke_rest;
    if (held || stroked) && robot.free() && !robot.mood.sulks(now) {
        robot.mood.patted = now;
        robot.mood.face = None;
        (rest.began, rest.path, rest.turns) = (now, 0.0, 0);
        rest.since = now;
        robot.passage = Some(Passage::new(Act::Wiggle, now));
        let at = robot.rig.at.at;
        cue(FolkSound::Pat, at, on.0);
        // A heart for one at rest, a spark for one at work.
        let kind = if robot.state == KState::Working {
            Kind::Spark
        } else {
            Kind::Heart
        };
        let from = at + Vec3::Y * set.heart_height;
        toys.toss(
            kind,
            now,
            (from, from + Vec3::Y * set.heart_rise),
            now,
        );
    }
    hand.rest = Some(rest);
}
