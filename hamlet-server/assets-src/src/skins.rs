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
