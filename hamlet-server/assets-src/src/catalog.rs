//! Every official asset: its id, what wears it, its kind, its words
//! and its price. The catalogue the server publishes is written from
//! this.

use looks::wardrobe::{Category, Target};
use serde::Serialize;

/// The version every asset is baked as. Raise it when an asset's file
/// changes, so owners are sent the new one.
pub const VERSION: &str = "1.0.0";

/// How many preview pictures are rendered of each asset, turned
/// evenly about it.
pub const FRAMES: u32 = 24;

/// What an asset costs: a colour skin, a skin with a finish of its
/// own, an emote and a mesh. One of each kind for each wearer is
/// free.
pub const COLOUR: u32 = 99;
pub const FINISH: u32 = 199;
pub const EMOTE: u32 = 199;
pub const MESH: u32 = 399;

/// One official asset.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Entry {
    pub id: &'static str,
    pub target: Target,
    pub category: Category,
    pub title: &'static str,
    pub summary: &'static str,
    pub price_cents: u32,
}

const fn entry(
    id: &'static str,
    (target, category): (Target, Category),
    (title, summary): (&'static str, &'static str),
    price_cents: u32,
) -> Entry {
    Entry {
        id,
        target,
        category,
        title,
        summary,
        price_cents,
    }
}

use Category::{Emote, Mesh, Skin};
use Target::{Agent, Hamlet};

/// Every official asset, agents' first.
pub const ALL: [Entry; 48] = [
    entry(
        "agent-emote-wave",
        (Agent, Emote),
        ("Wave", "A cheerful wave to whoever passes by."),
        0,
    ),
    entry(
        "agent-emote-bow",
        (Agent, Emote),
        ("Bow", "A deep, courtly bow, hat held to the chest."),
        EMOTE,
    ),
    entry(
        "agent-emote-dance",
        (Agent, Emote),
        ("Little Dance", "A happy shuffle when the work is done."),
        EMOTE,
    ),
    entry(
        "agent-skin-brass",
        (Agent, Skin),
        (
            "Brass Clockwork",
            "Polished brass plates and a warm gleam.",
        ),
        FINISH,
    ),
    entry(
        "agent-skin-moss",
        (Agent, Skin),
        (
            "Moss and Bark",
            "Weathered bark and soft moss, grown not built.",
        ),
        FINISH,
    ),
    entry(
        "agent-skin-starry",
        (Agent, Skin),
        ("Starry Night", "Deep night blue with a faint silver glow."),
        FINISH,
    ),
    entry(
        "agent-skin-sakura",
        (Agent, Skin),
        ("Sakura", "Soft cherry-blossom pinks."),
        0,
    ),
    entry(
        "agent-skin-ocean",
        (Agent, Skin),
        ("Ocean", "Sea-glass teal and foam white."),
        COLOUR,
    ),
    entry(
        "agent-skin-ember",
        (Agent, Skin),
        ("Ember", "Banked-fire reds with a coal glow."),
        COLOUR,
    ),
    entry(
        "agent-mesh-lantern",
        (Agent, Mesh),
        (
            "Lantern Head",
            "A body with a glowing paper lantern for a head.",
        ),
        MESH,
    ),
    entry(
        "agent-mesh-teapot",
        (Agent, Mesh),
        (
            "Round Teapot",
            "A round, squat body with a spout and a lid.",
        ),
        0,
    ),
    entry(
        "agent-mesh-stilts",
        (Agent, Mesh),
        (
            "Stilt Walker",
            "Tall, thin and stepping high on long legs.",
        ),
        MESH,
    ),
    entry(
        "hamlet-emote-smoke",
        (Hamlet, Emote),
        ("Chimney Smoke", "Lazy smoke curling from every chimney."),
        EMOTE,
    ),
    entry(
        "hamlet-emote-lanterns",
        (Hamlet, Emote),
        (
            "Paper Lanterns",
            "Red paper lanterns swaying under the eaves.",
        ),
        EMOTE,
    ),
    entry(
        "hamlet-emote-banners",
        (Hamlet, Emote),
        ("Banners", "Bright banners fluttering from every roof."),
        0,
    ),
    entry(
        "hamlet-skin-snow",
        (Hamlet, Skin),
        ("Snowy Roofs", "A fresh fall of snow on every roof."),
        FINISH,
    ),
    entry(
        "hamlet-skin-mossy",
        (Hamlet, Skin),
        ("Mossy Stone", "Old grey stone gone green with moss."),
        FINISH,
    ),
    entry(
        "hamlet-skin-plaster",
        (Hamlet, Skin),
        (
            "Painted Plaster",
            "Walls washed in bright southern pastels.",
        ),
        FINISH,
    ),
    entry(
        "hamlet-skin-autumn",
        (Hamlet, Skin),
        ("Autumn", "Rust and amber roofs over warm timber."),
        0,
    ),
    entry(
        "hamlet-skin-seaside",
        (Hamlet, Skin),
        ("Seaside", "Whitewash and sea-blue shutters."),
        COLOUR,
    ),
    entry(
        "hamlet-skin-twilight",
        (Hamlet, Skin),
        ("Twilight", "Dusky violets and indigo roofs."),
        COLOUR,
    ),
    entry(
        "hamlet-mesh-minka",
        (Hamlet, Mesh),
        (
            "Japanese Minka",
            "Deep thatched roofs over dark timber frames.",
        ),
        MESH,
    ),
    entry(
        "hamlet-mesh-stave",
        (Hamlet, Mesh),
        (
            "Nordic Stave Cabin",
            "Steep shingled roofs and carved gables.",
        ),
        MESH,
    ),
    entry(
        "hamlet-mesh-adobe",
        (Hamlet, Mesh),
        ("Desert Adobe", "Rounded earthen walls and flat roofs."),
        0,
    ),
    entry(
        "agent-emote-cheer",
        (Agent, Emote),
        ("Cheer", "Both arms flung high and a little hop for joy."),
        EMOTE,
    ),
    entry(
        "agent-emote-shrug",
        (Agent, Emote),
        ("Shrug", "Shoulders up, hands out, head tilted: who knows?"),
        EMOTE,
    ),
    entry(
        "agent-emote-ponder",
        (Agent, Emote),
        ("Ponder", "A hand to the chin and a long, slow think."),
        EMOTE,
    ),
    entry(
        "agent-skin-copper",
        (Agent, Skin),
        ("Copper Patina", "Old copper gone green at the seams."),
        FINISH,
    ),
    entry(
        "agent-skin-porcelain",
        (Agent, Skin),
        ("Porcelain", "Glazed white with cobalt-blue trim."),
        FINISH,
    ),
    entry(
        "agent-skin-obsidian",
        (Agent, Skin),
        ("Obsidian", "Glossy volcanic black with a molten glow."),
        FINISH,
    ),
    entry(
        "agent-skin-lavender",
        (Agent, Skin),
        ("Lavender", "Soft lilac paint and silver fittings."),
        COLOUR,
    ),
    entry(
        "agent-skin-sunflower",
        (Agent, Skin),
        ("Sunflower", "Bright summer yellow with a warm brown trim."),
        COLOUR,
    ),
    entry(
        "agent-skin-mint",
        (Agent, Skin),
        ("Mint", "Cool mint green and cream."),
        COLOUR,
    ),
    entry(
        "agent-mesh-mushroom",
        (Agent, Mesh),
        (
            "Mushroom Cap",
            "A spotted toadstool cap on a stout little body.",
        ),
        MESH,
    ),
    entry(
        "agent-mesh-snowbot",
        (Agent, Mesh),
        (
            "Snow Bot",
            "Round as a snowman, with a scarf and a carrot nose.",
        ),
        MESH,
    ),
    entry(
        "agent-mesh-knight",
        (Agent, Mesh),
        (
            "Little Knight",
            "Plate armour, a plumed helm and a shield.",
        ),
        MESH,
    ),
    entry(
        "hamlet-emote-petals",
        (Hamlet, Emote),
        ("Falling Petals", "Blossom drifting down past every eave."),
        EMOTE,
    ),
    entry(
        "hamlet-emote-fireflies",
        (Hamlet, Emote),
        (
            "Fireflies",
            "Little lights about every house when night falls.",
        ),
        EMOTE,
    ),
    entry(
        "hamlet-emote-pinwheels",
        (Hamlet, Emote),
        ("Pinwheels", "Bright pinwheels spinning along the eaves."),
        EMOTE,
    ),
    entry(
        "hamlet-skin-sandstone",
        (Hamlet, Skin),
        ("Sandstone", "Warm desert stone and sun-bleached roofs."),
        FINISH,
    ),
    entry(
        "hamlet-skin-tudor",
        (Hamlet, Skin),
        ("Tudor", "Black timbers on white plaster, dark roofs."),
        FINISH,
    ),
    entry(
        "hamlet-skin-gingerbread",
        (Hamlet, Skin),
        ("Gingerbread", "Baked brown walls and icing-white trim."),
        FINISH,
    ),
    entry(
        "hamlet-skin-meadow",
        (Hamlet, Skin),
        ("Meadow", "Spring greens and buttercup yellow."),
        COLOUR,
    ),
    entry(
        "hamlet-skin-midnight",
        (Hamlet, Skin),
        ("Midnight", "Deep navy roofs and moonlit walls."),
        COLOUR,
    ),
    entry(
        "hamlet-skin-coral",
        (Hamlet, Skin),
        ("Coral", "Coral pinks and sea-foam shutters."),
        COLOUR,
    ),
    entry(
        "hamlet-mesh-mushroom",
        (Hamlet, Mesh),
        (
            "Mushroom House",
            "A round stalk of a house under a spotted cap.",
        ),
        MESH,
    ),
    entry(
        "hamlet-mesh-yurt",
        (Hamlet, Mesh),
        (
            "Steppe Yurt",
            "A round felt tent with a painted door and a crown.",
        ),
        MESH,
    ),
    entry(
        "hamlet-mesh-lighthouse",
        (Hamlet, Mesh),
        (
            "Lighthouse Cottage",
            "A keeper's cottage with a striped light tower.",
        ),
        MESH,
    ),
];

/// An asset as the catalogue the server publishes lists it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Listed<'a> {
    pub id: &'a str,
    pub target: Target,
    pub category: Category,
    pub title: &'a str,
    pub summary: &'a str,
    pub price_cents: u32,
    pub version: &'a str,
    pub file: String,
    pub content_type: &'a str,
    pub frames: u32,
}

impl Entry {
    /// The name of its file under `files/`.
    pub fn file(&self) -> String {
        match self.category {
            Category::Mesh => format!("{}.glb", self.id),
            _ => format!("{}.json", self.id),
        }
    }

    pub fn content_type(&self) -> &'static str {
        match self.category {
            Category::Mesh => "model/gltf-binary",
            _ => "application/json",
        }
    }

    /// How it is listed, with `frames` preview pictures.
    pub fn listed(&self, frames: u32) -> Listed<'_> {
        Listed {
            id: self.id,
            target: self.target,
            category: self.category,
            title: self.title,
            summary: self.summary,
            price_cents: self.price_cents,
            version: VERSION,
            file: self.file(),
            content_type: self.content_type(),
            frames,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn the_catalogue_is_as_the_server_takes_it_and_as_was_asked() {
        let ids = ALL.iter().map(|e| e.id).collect::<BTreeSet<_>>();
        assert_eq!(ids.len(), ALL.len());
        for entry in ALL {
            assert!(entry.id.len() <= 64);
            assert!(entry.id.bytes().all(|b| b.is_ascii_lowercase()
                || b.is_ascii_digit()
                || b == b'-'));
            assert!((1..=60).contains(&entry.title.chars().count()));
            assert!(
                (1..=140).contains(&entry.summary.chars().count())
            );
            assert!(
                entry.price_cents == 0
                    || (50..=50_000).contains(&entry.price_cents)
            );
        }
        // Twenty-four for each wearer: six emotes, twelve skins, six
        // meshes, and one of each kind free.
        for target in Target::ALL {
            for (category, count) in
                [(Emote, 6), (Skin, 12), (Mesh, 6)]
            {
                let kind = ALL
                    .iter()
                    .filter(|e| {
                        e.target == target && e.category == category
                    })
                    .collect::<Vec<_>>();
                assert_eq!(
                    kind.len(),
                    count,
                    "{target:?} {category:?}"
                );
                assert_eq!(
                    kind.iter()
                        .filter(|e| e.price_cents == 0)
                        .count(),
                    1
                );
            }
        }
    }
}
