//! Cards over the realm: what the pointer rests on, and what waits
//! for the chief at a house.

mod hover;
mod needs;

/// The cards' methods on [`str`].
pub trait WordsExt {
    /// The words cut to `most` letters at a word's end.
    fn short(&self, most: usize) -> String;
}

impl WordsExt for str {
    fn short(&self, most: usize) -> String {
        if self.chars().count() <= most {
            return self.to_owned();
        }
        let cut = self
            .char_indices()
            .nth(most)
            .map_or(self.len(), |(at, _)| at);
        let kept = &self[..cut];
        let kept =
            kept.rsplit_once(' ').map_or(kept, |(kept, _)| kept);
        format!("{kept}...")
    }
}

pub(crate) fn plugin(app: &mut hamlet_engine_bevy::prelude::App) {
    app.add_plugins((hover::plugin, needs::plugin));
}
