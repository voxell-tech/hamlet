//! The field a message is typed in, with chips for attachments and
//! agents.

use std::collections::HashMap;
use std::ops::Range;

use bevy::ecs::change_detection::Mut;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::query::Changed;
use bevy::ecs::system::{Commands, Query, ResMut};
use bevy::ecs::world::World;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy_fynix::views::{
    Atom, Caret, Piece, Plain, PlainAtom, RichField,
};
use hamlet_ui::markdown::{agent_addresses, is_address_in};

use super::{ChatField, Draft, attach};
use crate::convo::Convo;
use crate::reference::chip_of;

/// What the key of an agent's chip in the field opens with.
const REFERENCE: &str = "reference:";

/// What stands for an atom in [`flat`] text.
const ATOM: &str = "\u{FFFC}";

/// A Hub's field's content as last seen, in [`flat`] text, and its
/// agent chips.
#[derive(Component, Default)]
pub(super) struct Typed {
    flat: String,
    chips: u64,
}

/// A Hub's unsent line of each chat, kept while another is shown.
#[derive(Component, Default)]
pub(super) struct Unsent(HashMap<Entity, Vec<Piece>>);

impl Unsent {
    /// Forgets the draft of `chat`, which is gone.
    pub(super) fn forget(&mut self, chat: Entity) {
        self.0.remove(&chat);
    }
}

/// The chat whose draft a field holds.
#[derive(Component)]
pub(super) struct Holds(Entity);

/// Puts each Hub's chat's draft in its field.
pub(super) fn swap_drafts(
    convos: Query<&Convo>,
    drafts: Query<&Unsent>,
    mut typed: Query<&mut Typed>,
    mut fields: Query<(
        Entity,
        &ChatField,
        &mut RichField,
        Option<&Holds>,
    )>,
    mut commands: Commands,
) {
    for (node, of, mut field, holds) in &mut fields {
        let Ok(convo) = convos.get(of.0) else {
            continue;
        };
        if holds.is_some_and(|holds| holds.0 == convo.chat) {
            continue;
        }
        let draft = drafts
            .get(of.0)
            .ok()
            .and_then(|unsent| unsent.0.get(&convo.chat).cloned())
            .unwrap_or_default();
        // Not an edit to read addresses in.
        if let Ok(mut typed) = typed.get_mut(of.0) {
            typed.flat = flat(&draft);
        }
        if !draft.is_empty() || !field.is_empty() {
            field.set_content(draft);
        }
        commands.entity(node).insert(Holds(convo.chat));
    }
}

/// Keeps `pieces` as `hub`'s draft of the chat in its field.
pub(super) fn keep_draft(
    world: &mut World,
    hub: Entity,
    pieces: &[Piece],
) {
    let mut fields = world.query::<(&ChatField, &Holds)>();
    let Some(held) = fields
        .iter(world)
        .find(|(field, _)| field.0 == hub)
        .map(|(_, holds)| holds.0)
    else {
        return;
    };
    put_draft(world, hub, held, pieces.to_vec());
}

/// Makes `pieces` `hub`'s draft of `chat`.
fn put_draft(
    world: &mut World,
    hub: Entity,
    chat: Entity,
    pieces: Vec<Piece>,
) {
    let Some(mut unsent) = world.get_mut::<Unsent>(hub) else {
        return;
    };
    if pieces.is_empty() {
        unsent.0.remove(&chat);
    } else {
        unsent.0.insert(chat, pieces);
    }
}

/// What `hub` has typed for `chat` and not sent.
pub(super) fn draft_of(
    world: &World,
    hub: Entity,
    chat: Entity,
) -> Vec<Piece> {
    world
        .get::<Unsent>(hub)
        .and_then(|unsent| unsent.0.get(&chat))
        .cloned()
        .unwrap_or_default()
}

/// Makes `pieces` `hub`'s draft of `chat` and has its field show it
/// once it shows that chat, mounted or not. The field takes the keys
/// only while it shows that chat and no other Hub has them.
pub(super) fn give_draft(
    world: &mut World,
    hub: Entity,
    chat: Entity,
    pieces: Vec<Piece>,
) {
    put_draft(world, hub, chat, pieces);
    let mut fields = world.query::<(Entity, &ChatField)>();
    let fields = fields
        .iter(world)
        .filter(|(_, field)| field.0 == hub)
        .map(|(node, _)| node)
        .collect::<Vec<_>>();
    for node in fields {
        world.entity_mut(node).remove::<Holds>();
    }
    let shows = crate::convo::chat_of(world, hub) == chat;
    let elsewhere = world
        .get_resource::<InputFocus>()
        .and_then(InputFocus::get)
        .is_some_and(|node| {
            crate::hub::holding(world, node) != Some(hub)
        });
    if shows && !elsewhere {
        super::focus(world, hub);
    }
}

/// Runs `edit` on `hub`'s field.
pub(super) fn with_field<R>(
    world: &mut World,
    hub: Entity,
    edit: impl FnOnce(Mut<RichField>) -> R,
) -> Option<R> {
    let mut fields = world.query::<(&ChatField, &mut RichField)>();
    fields
        .iter_mut(world)
        .find(|(field, _)| field.0 == hub)
        .map(|(_, field)| edit(field))
}

/// The text of `hub`'s field, other chips as their words, and
/// attachment places.
pub(super) fn typed(world: &mut World, hub: Entity) -> Option<Plain> {
    with_field(world, hub, |field| {
        let mut plain = Plain::default();
        for piece in field.pieces() {
            match piece {
                Piece::Text(text) => plain.text.push_str(&text),
                Piece::Atom(atom) if attach::is_attachment(&atom) => {
                    plain.atoms.push(PlainAtom {
                        key: atom.key().into(),
                        at: plain.text.len(),
                    });
                }
                Piece::Atom(atom) => plain.text.push_str(atom.text()),
            }
        }
        plain
    })
}

/// The chip of what `address` names for `hub`'s field, which is sent
/// as `words`.
pub(super) fn reference(
    world: &mut World,
    hub: Entity,
    address: &str,
    words: &str,
) -> Option<Atom> {
    let key = {
        let mut typed = world.get_mut::<Typed>(hub)?;
        typed.chips += 1;
        format!("{REFERENCE}{}", typed.chips)
    };
    chip_of(world, address, words, key)
}

/// Puts `with` in place of the bytes `range` of the text of `hub`'s
/// field, with the caret after it.
pub(super) fn replace(
    world: &mut World,
    hub: Entity,
    range: Range<usize>,
    with: Vec<Piece>,
) {
    with_field(world, hub, |mut field| {
        let after = with
            .iter()
            .map(|piece| match piece {
                Piece::Text(text) => text.len(),
                Piece::Atom(_) => 0,
            })
            .sum::<usize>();
        let mut pieces = Vec::new();
        let mut with = Some(with);
        let mut at = 0;
        for piece in field.pieces() {
            match piece {
                Piece::Atom(atom) => {
                    if at > range.start {
                        pieces.extend(
                            with.take().into_iter().flatten(),
                        );
                    }
                    pieces.push(Piece::Atom(atom));
                }
                Piece::Text(text) => {
                    let end = at + text.len();
                    let cut = range.start.clamp(at, end) - at
                        ..range.end.clamp(at, end) - at;
                    pieces
                        .push(Piece::Text(text[..cut.start].into()));
                    if end > range.start {
                        pieces.extend(
                            with.take().into_iter().flatten(),
                        );
                    }
                    pieces.push(Piece::Text(text[cut.end..].into()));
                    at = end;
                }
            }
        }
        pieces.extend(with.into_iter().flatten());
        field.set_content(pieces);
        field.set_caret(Caret::Offset(range.start + after));
    });
}

/// Puts `pieces` in `hub`'s field and gives it the focus.
pub(super) fn set(
    world: &mut World,
    hub: Entity,
    pieces: Vec<Piece>,
) {
    with_field(world, hub, |mut field| field.set_content(pieces));
    super::focus(world, hub);
}

/// Gives a Hub's field the focus when asked, once it is mounted.
pub fn refocus(
    mut drafts: Query<&mut Draft>,
    fields: Query<(Entity, &ChatField)>,
    mut focus: ResMut<InputFocus>,
) {
    for (node, field) in &fields {
        let Ok(mut draft) = drafts.get_mut(field.0) else {
            continue;
        };
        if draft.focus {
            draft.focus = false;
            focus.set(node, FocusCause::Pressed);
        }
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

/// Chips what was typed or pasted at the end of the frame the field
/// took it in, so it is never drawn as its words first: the field's
/// change handler runs only after its next frame's text is laid out.
pub(super) fn chip_early(
    fields: Query<(&ChatField, &RichField), Changed<RichField>>,
    mut commands: Commands,
) {
    for (of, field) in &fields {
        let (hub, pieces) = (of.0, field.pieces());
        commands.queue(move |world: &mut World| {
            chip_references(world, hub, &pieces);
        });
    }
}

/// Turns each address the last edit of `hub`'s field wrote whole
/// into its chip.
pub(super) fn chip_references(
    world: &mut World,
    hub: Entity,
    pieces: &[Piece],
) {
    let now = flat(pieces);
    let Some(mut typed) = world.get_mut::<Typed>(hub) else {
        return;
    };
    let was = std::mem::replace(&mut typed.flat, now.clone());
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
                let Some(mut typed) = world.get_mut::<Typed>(hub)
                else {
                    return;
                };
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
    if let Some(mut typed) = world.get_mut::<Typed>(hub) {
        typed.flat = flat(&out);
    }
    with_field(world, hub, |mut field| {
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
