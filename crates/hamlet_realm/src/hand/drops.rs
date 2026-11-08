//! Files dropped on the window, told to the chat they fell on.

use bevy::ecs::system::SystemParam;
use bevy::picking::hover::HoverMap;
use bevy::prelude::*;
use bevy::window::FileDragAndDrop;

use super::{FileDrop, FileDropped, Hand, PlaySettings};

/// The nodes that take dropped files, and whether the pointer is on
/// one.
#[derive(SystemParam)]
pub(super) struct DropTargets<'w, 's> {
    over: Res<'w, HoverMap>,
    targets: Query<'w, 's, (), With<FileDrop>>,
    parents: Query<'w, 's, &'static ChildOf>,
}

impl DropTargets<'_, '_> {
    fn is_pointed_at(&self) -> bool {
        self.over.values().flat_map(|hits| hits.keys()).any(|&hit| {
            self.targets.contains(hit)
                || self
                    .parents
                    .iter_ancestors(hit)
                    .any(|node| self.targets.contains(node))
        })
    }
}

/// Tells each file dropped on the window as [`FileDropped`] once the
/// pointer is seen on a [`FileDrop`]. One the pointer is not seen on
/// within the wait is let lie.
pub(super) fn drops(
    time: Res<Time>,
    set: Res<PlaySettings>,
    targets: DropTargets,
    mut told: MessageWriter<FileDropped>,
    mut dropped: MessageReader<FileDragAndDrop>,
    mut hand: ResMut<Hand>,
) {
    for drop in dropped.read() {
        if let FileDragAndDrop::DroppedFile { path_buf, .. } = drop {
            hand.drops.push((path_buf.clone(), set.drop_wait));
        }
    }
    if hand.drops.is_empty() {
        return;
    }
    // The spot it fell on is known once the pointer is seen on
    // something.
    if targets.is_pointed_at() {
        for (path, _) in hand.drops.drain(..) {
            told.write(FileDropped { path });
        }
        return;
    }
    let dt = time.delta_secs();
    hand.drops.retain_mut(|(_, left)| {
        *left -= dt;
        *left > 0.0
    });
}
