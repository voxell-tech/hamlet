//! The field a message is typed in, with chips for attachments and
//! agents.

use std::ops::Range;

use bevy::ecs::change_detection::Mut;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy_fynix::views::{Caret, Piece, Plain, PlainAtom, RichField};
use hamlet_ui::markdown::{agent_addresses, is_address_in};

use super::attach::Attached;
use super::{ChatField, Draft};
use crate::convo::Convo;
use crate::reference::chip_of;

/// What the key of an agent's chip in the field opens with.
const REFERENCE: &str = "reference:";

/// What stands for an atom in [`flat`] text.
const ATOM: &str = "\u{FFFC}";

/// The field's content as last seen, in [`flat`] text, and its agent
/// chips.
#[derive(Resource, Default)]
pub(super) struct Typed {
    flat: String,
    chips: u64,
}

/// The unsent line of a chat, kept while another chat is shown.
#[derive(Component)]
pub(super) struct Unsent(Vec<Piece>);

/// The chat whose draft a field holds.
#[derive(Component)]
pub(super) struct Holds(Entity);

/// Puts the shown chat's draft in the field.
pub(super) fn swap_drafts(
    convo: Res<Convo>,
    drafts: Query<&Unsent>,
    mut typed: ResMut<Typed>,
    mut attached: ResMut<Attached>,
    mut fields: Query<
        (Entity, &mut RichField, Option<&Holds>),
        With<ChatField>,
    >,
    mut commands: Commands,
) {
    for (node, mut field, holds) in &mut fields {
        if holds.is_some_and(|holds| holds.0 == convo.chat) {
            continue;
        }
        let draft = drafts
            .get(convo.chat)
            .map(|unsent| unsent.0.clone())
            .unwrap_or_default();
        // Not an edit to read addresses in.
        typed.flat = flat(&draft);
        if !draft.is_empty() || !field.is_empty() {
            field.set_content(draft);
        }
        attached.show(convo.chat);
        commands.entity(node).insert(Holds(convo.chat));
    }
}

/// Keeps `pieces` as the draft of the field's chat.
pub(super) fn keep_draft(world: &mut World, pieces: &[Piece]) {
    let mut fields =
        world.query_filtered::<&Holds, With<ChatField>>();
    let Some(held) = fields.iter(world).next().map(|holds| holds.0)
    else {
        return;
    };
    let Ok(mut chat) = world.get_entity_mut(held) else {
        return;
    };
    if pieces.is_empty() {
        chat.remove::<Unsent>();
    } else {
        chat.insert(Unsent(pieces.to_vec()));
    }
}

/// Runs `edit` on the field of the chat.
pub(super) fn with_field<R>(
    world: &mut World,
    edit: impl FnOnce(Mut<RichField>) -> R,
) -> Option<R> {
    let mut fields =
        world.query_filtered::<&mut RichField, With<ChatField>>();
    fields.iter_mut(world).next().map(edit)
}

/// The field's text, agent chips as words, and attachment places.
pub(super) fn typed(world: &mut World) -> Option<Plain> {
    with_field(world, |field| {
        let mut plain = Plain::default();
        for piece in field.pieces() {
            match piece {
                Piece::Text(text) => plain.text.push_str(&text),
                Piece::Atom(atom)
                    if atom.key().starts_with(REFERENCE) =>
                {
                    plain.text.push_str(atom.text());
                }
                Piece::Atom(atom) => plain.atoms.push(PlainAtom {
                    key: atom.key().into(),
                    at: plain.text.len(),
                }),
            }
        }
        plain
    })
}

/// Puts `pieces` in the field and gives it the focus.
pub(super) fn set(world: &mut World, pieces: Vec<Piece>) {
    with_field(world, |mut field| field.set_content(pieces));
    world.resource_mut::<Draft>().focus = true;
}

/// Gives the field the focus when asked, once it is mounted.
pub fn refocus(
    mut draft: ResMut<Draft>,
    fields: Query<Entity, With<ChatField>>,
    mut focus: ResMut<InputFocus>,
) {
    if !draft.focus {
        return;
    }
    if let Some(field) = fields.iter().next() {
        draft.focus = false;
        focus.set(field, FocusCause::Pressed);
    }
}

/// `pieces` as one text, each atom one [`ATOM`].
fn flat(pieces: &[Piece]) -> String {
    pieces
        .iter()
        .map(|piece| match piece {
            Piece::Text(text) => text.as_str(),
            Piece::Atom(_) => ATOM,
        })
        .collect()
}

/// The part of `now` an edit of `was` put there.
fn edited(was: &str, now: &str) -> Range<usize> {
    let head = was
        .char_indices()
        .zip(now.chars())
        .find(|((_, a), b)| a != b)
        .map_or(was.len().min(now.len()), |((at, _), _)| at);
    let tail = was[head..]
        .chars()
        .rev()
        .zip(now[head..].chars().rev())
        .take_while(|(a, b)| a == b)
        .map(|(a, _)| a.len_utf8())
        .sum::<usize>();
    head..now.len() - tail
}

/// Whether the address at `part` is whole: followed, or pasted at
/// once.
fn is_whole(
    text: &str,
    part: &Range<usize>,
    edit: &Range<usize>,
) -> bool {
    let after = &text[part.end..];
    let ended = after.starts_with(ATOM)
        || after.chars().next().is_some_and(|c| {
            c.is_whitespace() || c.is_ascii_punctuation()
        });
    ended || (edit.start <= part.start && part.end <= edit.end)
}

/// Turns each address the last edit wrote whole into its chip.
pub(super) fn chip_references(world: &mut World, pieces: &[Piece]) {
    let now = flat(pieces);
    let was = std::mem::replace(
        &mut world.resource_mut::<Typed>().flat,
        now.clone(),
    );
    let edit = edited(&was, &now);
    if edit.is_empty() {
        return;
    }
    let mut out = Vec::new();
    // Each part of `out` with the part of `now` it was.
    let mut parts = Vec::<(Range<usize>, Part)>::new();
    let mut chipped = false;
    let mut at = 0;
    let mut plain = 0;
    let text_part = |out: &mut Vec<Piece>,
                     parts: &mut Vec<(Range<usize>, Part)>,
                     words: &str,
                     at: usize,
                     plain: &mut usize| {
        if !words.is_empty() {
            out.push(Piece::Text(words.into()));
            parts.push((at..at + words.len(), Part::Text(*plain)));
            *plain += words.len();
        }
    };
    for piece in pieces {
        let text = match piece {
            Piece::Text(text) => text,
            Piece::Atom(atom) => {
                out.push(piece.clone());
                let key = atom.key().to_string();
                parts.push((at..at + ATOM.len(), Part::Atom(key)));
                at += ATOM.len();
                continue;
            }
        };
        let mut from = 0;
        let is_address = is_address_in(world);
        for (part, address) in agent_addresses(text, is_address) {
            let whole = at + part.start..at + part.end;
            let touched =
                whole.start <= edit.end && edit.start <= whole.end;
            if !touched || !is_whole(&now, &whole, &edit) {
                continue;
            }
            let key = {
                let mut typed = world.resource_mut::<Typed>();
                typed.chips += 1;
                format!("{REFERENCE}{}", typed.chips)
            };
            let words = &text[part.clone()];
            let Some(atom) =
                chip_of(world, &address, words, key.clone())
            else {
                continue;
            };
            text_part(
                &mut out,
                &mut parts,
                &text[from..part.start],
                at + from,
                &mut plain,
            );
            out.push(Piece::Atom(atom));
            parts.push((whole, Part::Atom(key)));
            chipped = true;
            from = part.end;
        }
        text_part(
            &mut out,
            &mut parts,
            &text[from..],
            at + from,
            &mut plain,
        );
        at += text.len();
    }
    if !chipped {
        return;
    }
    // The caret stays at the end of what was typed or pasted.
    let caret = edit.end;
    let place = parts.iter().find_map(|(part, kind)| match kind {
        Part::Text(plain)
            if part.contains(&caret) || part.end == caret =>
        {
            Some(Caret::Offset(plain + caret - part.start))
        }
        Part::Atom(key)
            if part.start < caret && caret <= part.end =>
        {
            Some(Caret::After(key.clone()))
        }
        Part::Text(_) | Part::Atom(_) => None,
    });
    world.resource_mut::<Typed>().flat = flat(&out);
    with_field(world, |mut field| {
        field.set_content(out);
        if let Some(place) = place {
            field.set_caret(place);
        }
    });
}

/// A part of the field's content.
enum Part {
    /// Words, from this byte of the text without its chips.
    Text(usize),
    /// A chip, by its key.
    Atom(String),
}
