//! A workspace's icon: one of the icons a workspace may have, written
//! in `workspaces.ron` by its name.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::icons::Glyph;

/// The icons a workspace may have, in the order the picker shows
/// them.
pub const PICKS: [Glyph; 30] = [
    Glyph::House,
    Glyph::Page,
    Glyph::Pencil,
    Glyph::Book,
    Glyph::Folder,
    Glyph::Speech,
    Glyph::Globe,
    Glyph::Chip,
    Glyph::Robot,
    Glyph::Flame,
    Glyph::Bolt,
    Glyph::Sun,
    Glyph::Moon,
    Glyph::Bell,
    Glyph::Clock,
    Glyph::Hourglass,
    Glyph::Coins,
    Glyph::Meter,
    Glyph::Shield,
    Glyph::Crown,
    Glyph::Signpost,
    Glyph::Aim,
    Glyph::Lens,
    Glyph::Sliders,
    Glyph::Signal,
    Glyph::Speaker,
    Glyph::Ballot,
    Glyph::Tray,
    Glyph::Play,
    Glyph::Move,
];

/// Words in a name, and the icon a name with one of them is given.
const WORDS: [(&[&str], Glyph); 14] = [
    (&["main", "home"], Glyph::House),
    (&["writ", "note", "draft", "edit"], Glyph::Pencil),
    (&["read", "book", "study", "learn"], Glyph::Book),
    (&["doc", "page", "spec"], Glyph::Page),
    (&["chat", "talk", "message", "mail"], Glyph::Speech),
    (&["code", "build", "dev", "debug"], Glyph::Chip),
    (&["agent", "bot", "model"], Glyph::Robot),
    (&["world", "realm", "map", "web"], Glyph::Globe),
    (&["plan", "road", "goal"], Glyph::Signpost),
    (&["review", "check", "test"], Glyph::Ballot),
    (&["search", "find", "research"], Glyph::Lens),
    (&["sound", "music", "audio"], Glyph::Speaker),
    (&["focus", "aim"], Glyph::Aim),
    (&["money", "budget", "cost", "sales"], Glyph::Coins),
];

/// The icon for a workspace named `name`: one its words call for,
/// or else the first of [`PICKS`] none of `taken` has.
pub fn suggest(name: &str, taken: &[Glyph]) -> Glyph {
    let name = name.to_lowercase();
    WORDS
        .iter()
        .find(|(words, _)| {
            words.iter().any(|word| name.contains(word))
        })
        .map(|(_, glyph)| *glyph)
        .or_else(|| {
            PICKS[1..]
                .iter()
                .copied()
                .find(|pick| !taken.contains(pick))
        })
        .unwrap_or(Glyph::Page)
}

pub fn serialize<S: Serializer>(
    icon: &Option<Glyph>,
    to: S,
) -> Result<S::Ok, S::Error> {
    icon.map(Glyph::name).serialize(to)
}

/// An icon by its name. A name no icon has is read as none, so a
/// file stays readable when an icon goes.
pub fn deserialize<'de, D: Deserializer<'de>>(
    from: D,
) -> Result<Option<Glyph>, D::Error> {
    let name = Option::<String>::deserialize(from)?;
    Ok(name.as_deref().and_then(Glyph::named))
}
