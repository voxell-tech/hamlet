//! What an agent or a hamlet wears, as a part of its page in the
//! Inspector: a choice of the owned assets that fit, by kind, and for
//! an agent, its emote played on demand.

use bevy::ecs::world::World;
use bevy::ui::{AlignItems, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{FrameProps, column, dropdown, label, row};
use bevy_fynix::{ViewExt, derived, keyed};
use hamlet_core::wardrobe::{PlayEmote, Wardrobe, Wear};
use hamlet_model::wardrobe::{Category, Target};
use hamlet_world::bazaar::Stall;

use crate::icons::Glyph;
use crate::looks::{ICON, action, bare, buttons, note};
use crate::roll::moved;
use crate::{AnyView, Cx};

/// The width of the name of a kind before its choice.
const KIND_WIDTH: f32 = 64.0;

/// One kind's choice, as it is drawn: the owned assets that fit, by
/// id and title, and the place of the one worn among them, 0 for
/// none.
#[derive(Clone, PartialEq, Debug)]
struct Choice {
    category: Category,
    options: Vec<(String, String)>,
    chosen: usize,
}

/// All the part is drawn from.
#[derive(Clone, PartialEq, Debug)]
struct Now {
    choices: Vec<Choice>,
    /// An emote is worn, to be played.
    emote: bool,
    said: Option<(String, bool)>,
}

/// The place of `worn` among `options` in a list that begins with
/// "None"; 0 for none, or for one that is not among them.
fn chosen(options: &[(String, String)], worn: Option<&str>) -> usize {
    worn.and_then(|worn| {
        options.iter().position(|(id, _)| id == worn)
    })
    .map_or(0, |at| at + 1)
}

fn now(world: &World, target: Target, name: &str) -> Now {
    let wardrobe = world.resource::<Wardrobe>();
    let worn = wardrobe.looks.of(target, name);
    let choices = Category::ALL
        .into_iter()
        .map(|category| {
            let options = wardrobe
                .wearable(target, category)
                .map(|asset| (asset.id.clone(), asset.title.clone()))
                .collect::<Vec<_>>();
            let chosen = chosen(
                &options,
                worn.and_then(|worn| worn.get(category)),
            );
            Choice {
                category,
                options,
                chosen,
            }
        })
        .collect::<Vec<_>>();
    let emote = choices.iter().any(|choice| {
        choice.category == Category::Emote && choice.chosen > 0
    });
    Now {
        choices,
        emote,
        said: wardrobe.said.clone(),
    }
}

/// What `name` of `target` wears, to be changed.
pub(crate) fn looks(target: Target, name: String) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        // Drawn without the core's plugins in the gallery.
        cx.world.init_resource::<Wardrobe>();
        let read = name.clone();
        let state =
            derived(move |world: &World| now(world, target, &read))
                .when(moved::<Wardrobe>());
        let name = name.clone();
        let view = keyed(state, move |now| part(target, &name, now))
            .within(bare());
        cx.build(view)
    })
}

fn part(target: Target, name: &str, now: &Now) -> AnyView {
    let stall = action(Glyph::Hanger, "ASSET STALL", |world| {
        crate::bazaar::open_stall(world, Stall::Assets)
    });
    let mut parts = Vec::new();
    if now.choices.iter().all(|choice| choice.options.is_empty()) {
        let who = match target {
            Target::Agent => "this agent",
            Target::Hamlet => "this hamlet",
        };
        parts.push(
            note(format!(
                "Nothing for {who} to wear yet. Skins, emotes and \
                 meshes are sold at the bazaar's Asset stall."
            ))
            .boxed(),
        );
        parts.push(buttons(vec![stall]).boxed());
    } else {
        for choice in &now.choices {
            parts.push(choose(target, name, choice));
        }
        let mut more = Vec::new();
        if target == Target::Agent && now.emote {
            let name = name.to_string();
            more.push(action(
                Glyph::Play,
                "PLAY EMOTE",
                move |world| world.trigger(PlayEmote(name.clone())),
            ));
        }
        more.push(stall);
        parts.push(buttons(more).boxed());
    }
    if let Some((words, wrong)) = &now.said {
        let tone = if *wrong { Tone::Critical } else { Tone::Accent };
        parts
            .push(label(words.clone()).size(12.0).tone(tone).boxed());
    }
    column(parts).gap(10.0).boxed()
}

/// The choice of what is worn of one kind.
fn choose(target: Target, name: &str, choice: &Choice) -> AnyView {
    let names = std::iter::once("None".to_string())
        .chain(choice.options.iter().map(|(_, title)| title.clone()))
        .collect::<Vec<_>>();
    let ids = choice
        .options
        .iter()
        .map(|(id, _)| id.clone())
        .collect::<Vec<_>>();
    let (name, category) = (name.to_string(), choice.category);
    row((
        row((label(category.one()).size(12.0).tone(Tone::Faint),))
            .min_width(px(KIND_WIDTH)),
        dropdown(
            names,
            choice.chosen,
            Glyph::Chevron.image(ICON),
            move |world, at| {
                world.trigger(Wear {
                    target,
                    name: name.clone(),
                    category,
                    id: at
                        .checked_sub(1)
                        .and_then(|at| ids.get(at).cloned()),
                });
            },
        )
        .min_width(px(200.0))
        .boxed(),
    ))
    .gap(10.0)
    .align(AlignItems::Center)
    .boxed()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_choice_shows_what_is_worn_or_none() {
        let options = [
            ("sakura".to_string(), "Sakura".to_string()),
            ("ember".to_string(), "Ember".to_string()),
        ];
        assert_eq!(chosen(&options, Some("ember")), 2);
        assert_eq!(chosen(&options, None), 0);
        // One worn and no longer owned shows as none.
        assert_eq!(chosen(&options, Some("gone")), 0);
    }
}
