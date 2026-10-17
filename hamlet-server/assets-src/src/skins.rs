//! The skins: the palette colours each wears in place of the world's,
//! and for an agent, how its paint takes the light.

use std::collections::BTreeMap;

use hamlet_model::wardrobe::{FORMAT, Finish, Skin};

fn skin(colours: &[(&str, &str)], finish: Option<Finish>) -> Skin {
    Skin {
        format: FORMAT,
        colours: colours
            .iter()
            .map(|(slot, hex)| (slot.to_string(), hex.to_string()))
            .collect::<BTreeMap<_, _>>(),
        finish,
    }
}

fn finish(
    metallic: f32,
    roughness: f32,
    glow: f32,
) -> Option<Finish> {
    Some(Finish {
        metallic,
        roughness,
        glow,
    })
}

/// The skin of the asset `id`, if it is one.
pub fn of(id: &str) -> Option<Skin> {
    Some(match id {
        "agent-skin-brass" => skin(
            &[
                ("paint", "#c9a24a"),
                ("brass", "#f0d27a"),
                ("steel", "#8a6a3a"),
                ("glass", "#fff1c9"),
                ("lamp", "#ffd27a"),
            ],
            finish(0.85, 0.3, 0.0),
        ),
        "agent-skin-moss" => skin(
            &[
                ("paint", "#5f7f3e"),
                ("brass", "#6b4a2f"),
                ("steel", "#4a3a2a"),
                ("glass", "#d9e8b8"),
                ("lamp", "#c8f08a"),
            ],
            finish(0.0, 0.95, 0.0),
        ),
        "agent-skin-starry" => skin(
            &[
                ("paint", "#1b2550"),
                ("brass", "#c9d3e8"),
                ("steel", "#2e3a6b"),
                ("glass", "#b8c8ff"),
                ("lamp", "#dfe6ff"),
            ],
            finish(0.35, 0.4, 0.18),
        ),
        "agent-skin-sakura" => skin(
            &[
                ("paint", "#f4b6c2"),
                ("brass", "#fbe3e8"),
                ("lamp", "#ffd6e0"),
            ],
            None,
        ),
        "agent-skin-ocean" => skin(
            &[
                ("paint", "#2a8c8c"),
                ("brass", "#e6f4f1"),
                ("glass", "#bfe9f2"),
                ("lamp", "#9fe3ff"),
            ],
            None,
        ),
        "agent-skin-ember" => skin(
            &[
                ("paint", "#9c2a1c"),
                ("brass", "#e07a2e"),
                ("lamp", "#ff8a3d"),
            ],
            finish(0.1, 0.55, 0.08),
        ),
        "hamlet-skin-snow" => skin(
            &[
                ("roof", "#eef3f7"),
                ("drift", "#ffffff"),
                ("slate_roof", "#dfe7ee"),
                ("terracotta", "#e9eef3"),
                ("timber", "#5a4030"),
                ("timber_dark", "#3f2d22"),
            ],
            None,
        ),
        "hamlet-skin-mossy" => skin(
            &[
                ("limestone", "#8f9a7a"),
                ("limestone_shade", "#5f6a4f"),
                ("plaster", "#a9b08a"),
                ("plaster_worn", "#7f8f5a"),
                ("moss", "#5f8f3a"),
                ("roof", "#4f6a3f"),
                ("drift", "#7f9a4f"),
            ],
            None,
        ),
        "hamlet-skin-plaster" => skin(
            &[
                ("plaster", "#f2c4a0"),
                ("plaster_worn", "#e9a98a"),
                ("whitewash", "#fbe7c6"),
                ("ochre", "#f2c14e"),
                ("livery", "#3f8fbf"),
                ("roof", "#c8553d"),
            ],
            None,
        ),
        "hamlet-skin-autumn" => skin(
            &[
                ("roof", "#a8431f"),
                ("drift", "#d9822b"),
                ("terracotta", "#b8501f"),
                ("timber", "#6b3f22"),
                ("livery", "#7a2e1f"),
                ("banner", "#e0a030"),
            ],
            None,
        ),
        "hamlet-skin-seaside" => skin(
            &[
                ("whitewash", "#f7f7f2"),
                ("plaster", "#f2f0e6"),
                ("roof", "#2f6fa8"),
                ("drift", "#5f9fd0"),
                ("livery", "#1f5f9f"),
                ("banner", "#f2d35b"),
            ],
            None,
        ),
        "hamlet-skin-twilight" => skin(
            &[
                ("plaster", "#b9a8d0"),
                ("whitewash", "#d6cce6"),
                ("roof", "#3a2f6b"),
                ("drift", "#5f4f9f"),
                ("livery", "#2a1f4f"),
                ("banner", "#c86bd0"),
            ],
            None,
        ),
        "agent-skin-copper" => skin(
            &[
                ("paint", "#4f9c86"),
                ("brass", "#b8733a"),
                ("steel", "#6e4a2e"),
                ("glass", "#d6f2e6"),
                ("lamp", "#9fe8c8"),
            ],
            finish(0.7, 0.45, 0.0),
        ),
        "agent-skin-porcelain" => skin(
            &[
                ("paint", "#f4f2ec"),
                ("brass", "#2f55a3"),
                ("steel", "#d8dde8"),
                ("glass", "#eaf1ff"),
                ("lamp", "#cfe0ff"),
            ],
            finish(0.05, 0.15, 0.0),
        ),
        "agent-skin-obsidian" => skin(
            &[
                ("paint", "#16141a"),
                ("brass", "#ff6a2a"),
                ("steel", "#2a2630"),
                ("glass", "#ffb27a"),
                ("lamp", "#ff7a3a"),
            ],
            finish(0.4, 0.12, 0.12),
        ),
        "agent-skin-lavender" => skin(
            &[
                ("paint", "#b9a3d9"),
                ("brass", "#dfe3ea"),
                ("lamp", "#e6d6ff"),
            ],
            None,
        ),
        "agent-skin-sunflower" => skin(
            &[
                ("paint", "#f2c42a"),
                ("brass", "#7a4a1f"),
                ("lamp", "#ffe08a"),
            ],
            None,
        ),
        "agent-skin-mint" => skin(
            &[
                ("paint", "#9fdcc0"),
                ("brass", "#f3ead2"),
                ("glass", "#e8fff4"),
            ],
            None,
        ),
        "hamlet-skin-sandstone" => skin(
            &[
                ("limestone", "#d9a66b"),
                ("limestone_shade", "#a8763f"),
                ("plaster", "#e8c08a"),
                ("plaster_worn", "#d2a26a"),
                ("whitewash", "#f2dcb2"),
                ("roof", "#e6d2a8"),
                ("drift", "#f4e6c4"),
                ("livery", "#2f7f8f"),
            ],
            None,
        ),
        "hamlet-skin-tudor" => skin(
            &[
                ("plaster", "#f2eee2"),
                ("plaster_worn", "#e2dccb"),
                ("whitewash", "#f6f2e8"),
                ("timber", "#2a2420"),
                ("timber_dark", "#1a1614"),
                ("roof", "#3f3a3a"),
                ("drift", "#5a5252"),
                ("livery", "#2a2420"),
            ],
            None,
        ),
        "hamlet-skin-gingerbread" => skin(
            &[
                ("plaster", "#a0602a"),
                ("plaster_worn", "#8a4f22"),
                ("ochre", "#b8723a"),
                ("whitewash", "#fff6ea"),
                ("timber", "#fff6ea"),
                ("timber_dark", "#f0e2d0"),
                ("roof", "#6b3a1f"),
                ("drift", "#fffaf2"),
                ("livery", "#e2405a"),
                ("banner", "#e2405a"),
            ],
            None,
        ),
        "hamlet-skin-meadow" => skin(
            &[
                ("roof", "#6f9f3f"),
                ("drift", "#a8d06a"),
                ("livery", "#f2c42a"),
                ("banner", "#f2c42a"),
            ],
            None,
        ),
        "hamlet-skin-midnight" => skin(
            &[
                ("plaster", "#c9cfe0"),
                ("whitewash", "#dde2ef"),
                ("roof", "#1c2547"),
                ("drift", "#3a4a7a"),
                ("livery", "#141c3a"),
                ("banner", "#e8d38a"),
            ],
            None,
        ),
        "hamlet-skin-coral" => skin(
            &[
                ("plaster", "#f2a28a"),
                ("whitewash", "#fbe2d6"),
                ("roof", "#e0604f"),
                ("drift", "#f29a7a"),
                ("livery", "#5fbfb0"),
                ("banner", "#5fbfb0"),
            ],
            None,
        ),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use hamlet_model::wardrobe::{Category, Skin};

    use super::*;
    use crate::catalog::ALL;

    #[test]
    fn every_skin_reads_back_as_the_client_reads_it() {
        for entry in
            ALL.iter().filter(|e| e.category == Category::Skin)
        {
            let skin = of(entry.id).expect(entry.id);
            let bytes = serde_json::to_vec(&skin).unwrap();
            assert_eq!(
                Skin::read(&bytes, entry.target).unwrap(),
                skin,
                "{}",
                entry.id
            );
        }
    }
}
