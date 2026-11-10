//! The kinds of box the panels are built of. A view names a kind and
//! what goes in it; how the kind is drawn, its room, corner, fill and
//! edge, is left unset here and set by the rules in `rules.rs`, as a
//! Typst template sets what a document only names.

use std::marker::PhantomData;

use bevy::ecs::entity::Entity;
use bevy::ui::FlexDirection;
use bevy_fynix::views::{Frame, FrameProps};
use bevy_fynix::{Bevy, Styled, ViewSeq};

use crate::Cx;
use crate::theme::{HamletTheme, StatusKind};

/// A card in a panel, edged: what sits apart from the panel's words.
pub struct Card;

/// A card with no edge, on the panel like a tile.
pub struct Tile;

/// A small plate around a word or two: code, a tag, a key.
pub struct Chip;

/// A kind of box, and how a status changes its look.
pub trait Kind: Send + Sync + 'static {
    /// The way its children are laid out.
    const DIRECTION: FlexDirection;

    /// What `kind` changes in a box of this kind, over its look.
    fn status(kind: StatusKind, theme: &HamletTheme) -> Frame;
}

/// The look of every box of the kind `K` in a scope: a frame whose
/// props the rules set. A call site's own props beat it.
pub struct Look<K>(pub Frame, PhantomData<K>);

impl<K> Look<K> {
    /// This, with `change` made to its frame.
    pub fn frame(self, change: impl FnOnce(Frame) -> Frame) -> Self {
        Look(change(self.0), PhantomData)
    }
}

impl<K: Kind> Styled for Look<K> {
    fn unset() -> Self {
        Look(Frame::unset(), PhantomData)
    }

    fn over(self, below: Self) -> Self {
        Look(self.0.over(below.0), PhantomData)
    }
}

/// A box of the kind `K` with `children` in it.
pub struct Boxed<K, C> {
    frame: Frame,
    children: C,
    status: Option<StatusKind>,
    kind: PhantomData<K>,
}

impl<K: Kind, C> Boxed<K, C> {
    fn new(children: C) -> Self {
        Self {
            frame: Frame::unset().direction(K::DIRECTION),
            children,
            status: None,
            kind: PhantomData,
        }
    }

    /// This, marked as being about `kind`.
    pub fn status(mut self, kind: StatusKind) -> Self {
        self.status = Some(kind);
        self
    }
}

impl<K, C> FrameProps for Boxed<K, C> {
    fn frame_mut(&mut self) -> &mut Frame {
        &mut self.frame
    }
}

impl<K: Kind, C: ViewSeq<Bevy, HamletTheme>>
    bevy_fynix::View<Bevy, HamletTheme> for Boxed<K, C>
{
    fn build(self, cx: &mut Cx<'_>) -> Entity {
        let mut look = cx.resolve(Look::<K>::unset()).0;
        if let Some(kind) = self.status {
            look = K::status(kind, cx.theme()).over(look);
        }
        let node = cx.build(self.frame.over(look));
        cx.under(node, |cx| self.children.build_each(cx));
        node
    }
}

/// A [`Card`] of `children`, one under another.
pub fn card<C>(children: C) -> Boxed<Card, C> {
    Boxed::new(children)
}

/// A [`Tile`] of `children`, one under another.
pub fn tile<C>(children: C) -> Boxed<Tile, C> {
    Boxed::new(children)
}

/// A [`Chip`] of `children`, side by side.
pub fn chip<C>(children: C) -> Boxed<Chip, C> {
    Boxed::new(children)
}
