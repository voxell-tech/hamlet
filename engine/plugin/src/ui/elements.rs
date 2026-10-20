//! The elements a plugin's panels are made of, each a node of the
//! engine's of one kind, and what is built out of them.

use fynix::{Cx, Patch, Prop, Styled, View, ViewSeq, element};

use super::world::{Handler, World};
use super::{Hamlet, Key};
use crate::bindings::hamlet::engine::ui::{
    ButtonStyle, Direction, Kind, Motion as Movement, Property, Size,
    Space, Tone,
};

/// A run of text.
#[element(backend = Hamlet, prepare = prepare_label)]
pub struct Label {
    #[elem(patch = SetText)]
    pub text: Prop<World, String>,
    #[elem(default = Tone::Body, patch = SetTone)]
    pub tone: Prop<World, Tone>,
    #[elem(default = Size::Body, patch = SetSize)]
    pub size: Prop<World, Size>,
    #[elem(patch = SetBold)]
    pub bold: Prop<World, bool>,
    /// 1.0, fully opaque, when unset.
    #[elem(default = 1.0, patch = SetOpacity, blend = blend)]
    pub opacity: Prop<World, f32>,
}

/// A button lettered with `text`, in the engine's style `style`.
#[element(backend = Hamlet, prepare = prepare_button)]
pub struct Button {
    #[elem(patch = SetText)]
    pub text: Prop<World, String>,
    #[elem(default = ButtonStyle::Plain, patch = SetStyle)]
    pub style: Prop<World, ButtonStyle>,
}

/// A column or a row, holding no views of its own.
#[element(backend = Hamlet, prepare = prepare_frame)]
pub struct Frame {
    #[elem(default = Direction::Column, patch = SetDirection)]
    pub direction: Prop<World, Direction>,
    #[elem(default = Space::Gap, patch = SetSpace)]
    pub space: Prop<World, Space>,
}

pub fn label(text: impl Into<Prop<World, String>>) -> Label {
    Label {
        text: text.into(),
        ..Label::unset()
    }
}

/// The name at the head of a panel.
pub fn heading(text: impl Into<Prop<World, String>>) -> Label {
    label(text).size(Size::Title).bold(true)
}

pub fn button(text: impl Into<Prop<World, String>>) -> Button {
    Button {
        text: text.into(),
        ..Button::unset()
    }
}

/// A [`Frame`] with `children` built under it, in order.
pub struct Stack<C> {
    pub frame: Frame,
    pub children: C,
}

pub fn column<C>(children: C) -> Stack<C> {
    Stack {
        frame: Frame::unset().direction(Direction::Column),
        children,
    }
}

pub fn row<C>(children: C) -> Stack<C> {
    Stack {
        frame: Frame::unset().direction(Direction::Row),
        children,
    }
}

impl<C> FrameProps for Stack<C> {
    fn frame_mut(&mut self) -> &mut Frame {
        &mut self.frame
    }
}

impl<T: 'static, C: ViewSeq<Hamlet, T>> View<Hamlet, T> for Stack<C> {
    fn build(self, cx: &mut Cx<'_, Hamlet, T>) -> Key {
        let node = cx.build(self.frame);
        cx.under(node, |cx| self.children.build_each(cx));
        node
    }
}

/// A view whose root node runs a handler when it is clicked.
pub struct OnClick<V> {
    inner: V,
    handler: Handler,
}

impl<T, V: View<Hamlet, T>> View<Hamlet, T> for OnClick<V> {
    fn build(self, cx: &mut Cx<'_, Hamlet, T>) -> Key {
        let node = self.inner.build(cx);
        cx.world.clicks.insert(node, self.handler);
        node
    }
}

/// A view whose values the engine animates when they change, over a
/// movement of its theme.
pub struct EngineTransition<V> {
    inner: V,
    motion: Movement,
}

impl<T, V: View<Hamlet, T>> View<Hamlet, T> for EngineTransition<V> {
    fn build(self, cx: &mut Cx<'_, Hamlet, T>) -> Key {
        let first = cx.world.nodes.next_key();
        let node = self.inner.build(cx);
        // After the build, so the first values are shown at once.
        cx.world.nodes.animate_from(first, self.motion);
        node
    }
}

/// What any view of a panel can be given.
pub trait UiExt: Sized {
    /// This view, running `handler` when its root node is clicked.
    fn on_click(
        self,
        handler: impl Fn(&mut World) + Send + Sync + 'static,
    ) -> OnClick<Self> {
        OnClick {
            inner: self,
            handler: Box::new(handler),
        }
    }

    /// This view, with each of its values animated by the engine
    /// over `motion` when it changes.
    fn engine_transition(
        self,
        motion: fynix::Motion,
    ) -> EngineTransition<Self> {
        let motion = match motion {
            fynix::Motion::Interact => Movement::Interact,
            fynix::Motion::Expand => Movement::Expand,
        };
        EngineTransition {
            inner: self,
            motion,
        }
    }
}

impl<V> UiExt for V {}

fn prepare_label(world: &mut World, node: Key) {
    world.nodes.prepare(node, Kind::Label);
}

fn prepare_button(world: &mut World, node: Key) {
    world.nodes.prepare(node, Kind::Button);
}

fn prepare_frame(world: &mut World, node: Key) {
    world.nodes.prepare(node, Kind::Frame);
}

fn blend(from: &f32, to: &f32, t: f32) -> f32 {
    from + (to - from) * t
}

/// A [`Patch`] sending one property of a node.
macro_rules! patch {
    ($name:ident, $value:ty, $make:expr) => {
        pub struct $name;

        impl Patch<Hamlet, $value> for $name {
            fn patch(world: &mut World, node: Key, value: &$value) {
                world.nodes.set(node, $make(value));
            }
        }
    };
}

patch!(SetText, String, |text: &String| Property::Text(
    text.clone()
));
patch!(SetTone, Tone, |tone: &Tone| Property::Tone(*tone));
patch!(SetSize, Size, |size: &Size| Property::Size(*size));
patch!(SetBold, bool, |bold: &bool| Property::Bold(*bold));
patch!(SetOpacity, f32, |opacity: &f32| Property::Opacity(*opacity));
patch!(SetStyle, ButtonStyle, |style: &ButtonStyle| {
    Property::Style(*style)
});
patch!(SetDirection, Direction, |direction: &Direction| {
    Property::Direction(*direction)
});
patch!(SetSpace, Space, |space: &Space| Property::Space(*space));
