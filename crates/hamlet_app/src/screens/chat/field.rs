//! The field a message is typed in: text with the chips of what is
//! attached to it and of the agents named in it.

use std::collections::HashMap;
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

/// The field's content as it was last seen, in [`flat`] text, and the
/// chips of agents put in it so far.
#[derive(Resource, Default)]
pub(super) struct Typed {
    flat: String,
    chips: u64,
}

/// The unsent line of each conversation's chat, kept while another
/// chat is shown.
#[derive(Resource, Default)]
pub(super) struct Drafts(HashMap<Entity, Vec<Piece>>);

impl Drafts {
    fn keep(&mut self, session: Entity, pieces: Vec<Piece>) {
        if pieces.is_empty() {
            self.0.remove(&session);
        } else {
            self.0.insert(session, pieces);
        }
    }
}

/// The conversation whose draft a field holds.
#[derive(Component)]
pub(super) struct Holds(Entity);

/// Puts the draft of the shown conversation in the field, when the
/// field is new or another conversation is shown, and keeps the draft
/// it held.
pub(super) fn swap_drafts(
    convo: Res<Convo>,
    mut drafts: ResMut<Drafts>,
    mut typed: ResMut<Typed>,
    mut attached: ResMut<Attached>,
    mut fields: Query<
        (Entity, &mut RichField, Option<&Holds>),
        With<ChatField>,
    >,
    mut commands: Commands,
) {
    for (node, mut field, holds) in &mut fields {
        if holds.is_some_and(|holds| holds.0 == convo.session) {
            continue;
        }
        if let Some(Holds(was)) = holds {
            drafts.keep(*was, field.pieces());
        }
        let draft =
            drafts.0.get(&convo.session).cloned().unwrap_or_default();
        // Its words are its own, not an edit to read addresses in.
        typed.flat = flat(&draft);
        if !draft.is_empty() || !field.is_empty() {
            field.set_content(draft);
        }
        attached.show(convo.session);
        commands.entity(node).insert(Holds(convo.session));
    }
}

/// Keeps `pieces`, the content of the field, as the draft of the
/// conversation it holds.
pub(super) fn keep_draft(world: &mut World, pieces: &[Piece]) {
    let mut fields =
        world.query_filtered::<&Holds, With<ChatField>>();
    let Some(session) =
        fields.iter(world).next().map(|holds| holds.0)
    else {
        return;
    };
    world
        .resource_mut::<Drafts>()
        .keep(session, pieces.to_vec());
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

/// The text of the field, with each agent's chip as its words and a
/// place for each chip of an attachment.
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

/// Puts `pieces` in the field, with the caret at their end, and gives
/// it the focus.
pub(super) fn set(world: &mut World, pieces: Vec<Piece>) {
    with_field(world, |mut field| field.set_content(pieces));
    world.resource_mut::<Draft>().focus = true;
}

/// Gives the field the focus when it was asked for. The ask waits for
/// a field that is not mounted yet, as when the thread is just shown.
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

/// The part of `now` an edit of `was` put there, found by what the
/// two start and end with.
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

/// Whether the address at `part` of `text` is written whole: a space,
/// a mark or a chip follows it, or the edit `edit` put all of it
/// there at once, as a paste does. Typed a letter at a time, an
/// address is whole only once something follows it.
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

/// Turns each agent's or scholar's address the last edit wrote whole
/// in the field into its chip, as the chat draws it, with the caret
/// kept where the edit left it.
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
    // The caret stands at the end of what was typed or pasted: in the
    // words it ends, or after the chip it ends in.
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
