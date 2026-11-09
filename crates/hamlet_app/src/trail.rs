//! The pages a panel has gone through, to go back and forward among
//! them. Each panel keeps a trail of its own: the primary key with
//! `[` or `]`, and the mouse's back and forward buttons, move the
//! panel under the pointer.

use bevy::ecs::hierarchy::ChildOf;
use bevy::ecs::system::Res;
use bevy::ecs::world::World;
use bevy::input::ButtonInput;
use bevy::input::mouse::MouseButton;
use bevy::picking::hover::HoverMap;
use bevy::picking::pointer::PointerId;
use bevy_fynix::shortcut::{Scope, ScopeId};

/// How many pages a trail keeps behind.
const KEPT: usize = 50;

/// A way along a trail.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Way {
    Back,
    Forward,
}

/// The pages behind the one a panel shows and those ahead of it.
#[derive(Clone, Debug)]
pub(crate) struct Trail<T> {
    behind: Vec<T>,
    ahead: Vec<T>,
    now: Option<T>,
}

impl<T> Default for Trail<T> {
    fn default() -> Self {
        Self {
            behind: Vec::new(),
            ahead: Vec::new(),
            now: None,
        }
    }
}

impl<T: Copy + PartialEq> Trail<T> {
    /// Keeps the page left behind as the panel turns to `here`. A
    /// page reached afresh leaves nothing ahead.
    pub(crate) fn keep(&mut self, here: T) {
        match self.now.replace(here) {
            Some(was) if was != here => {
                self.behind.push(was);
                if self.behind.len() > KEPT {
                    self.behind.remove(0);
                }
                self.ahead.clear();
            }
            _ => {}
        }
    }

    /// The nearest page `way` of `now` that `stands`, which the trail
    /// then takes as shown. Pages passed over that no longer stand
    /// are dropped.
    pub(crate) fn travel(
        &mut self,
        way: Way,
        now: T,
        stands: impl Fn(&T) -> bool,
    ) -> Option<T> {
        let next = loop {
            let taken = match way {
                Way::Back => self.behind.pop(),
                Way::Forward => self.ahead.pop(),
            }?;
            if stands(&taken) {
                break taken;
            }
        };
        match way {
            Way::Back => self.ahead.push(now),
            Way::Forward => self.behind.push(now),
        }
        // Shown as it is, it is not kept again as a page left.
        self.now = Some(next);
        Some(next)
    }
}

/// Whether the mouse's back or forward button was pressed this
/// frame: the condition of a system that travels by them.
pub(crate) fn mouse_pressed(
    mouse: Option<Res<ButtonInput<MouseButton>>>,
) -> bool {
    mouse.is_some_and(|mouse| {
        mouse.any_just_pressed([
            MouseButton::Back,
            MouseButton::Forward,
        ])
    })
}

/// The way the mouse's back or forward button pressed this frame
/// leads, if one was.
pub(crate) fn mouse_way(world: &World) -> Option<Way> {
    let mouse = world.get_resource::<ButtonInput<MouseButton>>()?;
    if mouse.just_pressed(MouseButton::Back) {
        Some(Way::Back)
    } else if mouse.just_pressed(MouseButton::Forward) {
        Some(Way::Forward)
    } else {
        None
    }
}

/// Whether the mouse's pointer is over a panel of `scope`: the node
/// nearest under it, or one it is inside of, stands for the scope.
pub(crate) fn pointer_over(world: &World, scope: ScopeId) -> bool {
    let top = world
        .get_resource::<HoverMap>()
        .and_then(|hover| hover.get(&PointerId::Mouse))
        .and_then(|hits| {
            hits.iter()
                .min_by(|(_, a), (_, b)| a.depth.total_cmp(&b.depth))
                .map(|(&node, _)| node)
        });
    let mut at = top;
    while let Some(node) = at {
        if world.get::<Scope>(node).is_some_and(|s| s.0 == scope) {
            return true;
        }
        at = world.get::<ChildOf>(node).map(ChildOf::parent);
    }
    false
}
