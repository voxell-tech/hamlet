//! The `loading` marker of a library stays until the app has run
//! some clean frames since the library's entry returned, because a
//! library can also fail in the systems its entry added.

use bevy::prelude::*;
use hamlet_assets::{File, Tuning, designed, follow};
use hamlet_base::HomeDir;

use crate::install::Install;
use crate::marker;

/// Adds the tuning and the system that settles libraries.
pub fn plugin(app: &mut App) {
    app.add_plugins(
        hamlet_assets::TuningPlugin::<NativeTuning>::default(),
    )
    .init_resource::<Guard>()
    .init_resource::<Install>()
    .init_resource::<Settling>()
    .add_systems(Update, settle.run_if(unsettled))
    .add_systems(
        Last,
        (
            settle_all
                .run_if(unsettled.and_then(on_message::<AppExit>)),
            exited.run_if(on_message::<AppExit>),
        ),
    );
}

/// The guard on the libraries.
#[derive(Resource, Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Guard {
    /// The frames the app runs, after a library's entry returned,
    /// before the library is trusted and its marker goes.
    pub clean_frames: u32,
    /// The runs in a row that opened a library and did not exit by
    /// themselves, after which a start opens none. 0 never does.
    pub unclean_starts: u32,
}

/// The sections of `assets/native.toml`.
#[derive(Asset, TypePath, Clone, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeTuning {
    pub guard: Guard,
    pub install: Install,
}

impl Tuning for NativeTuning {
    const FILE: File = hamlet_assets::NATIVE;

    fn apply(&self, was: &Self, world: &mut World) {
        follow(world, &was.guard, &self.guard);
        follow(world, &was.install, &self.install);
    }
}

designed!(NativeTuning { Guard => guard, Install => install });

/// The libraries whose entry returned and that are not yet trusted,
/// each with the clean frames still to run.
#[derive(Resource, Default, Debug)]
pub struct Settling(Vec<(String, u32)>);

impl Settling {
    pub fn add(&mut self, id: &str, frames: u32) {
        self.0.retain(|(waiting, _)| waiting != id);
        self.0.push((id.into(), frames));
    }

    pub fn forget(&mut self, id: &str) {
        self.0.retain(|(waiting, _)| waiting != id);
    }

    pub fn waiting(&self) -> bool {
        !self.0.is_empty()
    }

    /// Counts one clean frame and gives the ids that have had enough.
    pub fn frame(&mut self) -> Vec<String> {
        for (_, left) in &mut self.0 {
            *left = left.saturating_sub(1);
        }
        let done = self
            .0
            .iter()
            .filter(|(_, left)| *left == 0)
            .map(|(id, _)| id.clone())
            .collect();
        self.0.retain(|(_, left)| *left > 0);
        done
    }
}

fn unsettled(settling: Res<Settling>) -> bool {
    settling.waiting()
}

fn settle(
    mut settling: ResMut<Settling>,
    home: Option<Res<HomeDir>>,
) {
    for id in settling.frame() {
        if let Some(home) = &home {
            marker::settled(&home.0.root, &id);
        }
    }
}

/// The app is exiting by itself: the count of unclean runs starts
/// again.
fn exited(home: Option<Res<HomeDir>>) {
    if let Some(home) = home {
        marker::exited(&home.0.root);
    }
}

/// The app is exiting by itself, so no library stopped it.
fn settle_all(
    mut settling: ResMut<Settling>,
    home: Option<Res<HomeDir>>,
) {
    for (id, _) in std::mem::take(&mut settling.0) {
        if let Some(home) = &home {
            marker::settled(&home.0.root, &id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_library_is_trusted_after_its_clean_frames() {
        let mut settling = Settling::default();
        settling.add("spark", 3);
        settling.add("glow", 1);
        assert_eq!(settling.frame(), vec!["glow".to_string()]);
        assert!(settling.frame().is_empty());
        assert_eq!(settling.frame(), vec!["spark".to_string()]);
        assert!(!settling.waiting());
    }
}
