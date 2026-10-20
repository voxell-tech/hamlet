//! The elements a plugin's panels are made of, each a node of the
//! engine's of one kind, and what is built out of them.

use core::any::TypeId;

use fynix::{Cx, Patch, Prop, Styled, View, ViewSeq, element};

use super::world::{Handler, World};
use super::{Hamlet, Key};
use crate::bindings::hamlet::engine::ui::{
    ButtonStyle, Direction, Face, Kind, Motion as Movement, Property,
    Size, Space, Tone,
};

/// A run of text.
#[element(backend = Hamlet)]
pub struct Label {
    #[elem(patch = SetText)]
    pub text: Prop<World, String>,
    #[elem(default = Tone::Body, patch = SetTone, blend = jump)]
    pub tone: Prop<World, Tone>,
    #[elem(default = Size::Body, patch = SetSize, blend = jump)]
    pub size: Prop<World, Size>,
    #[elem(patch = SetBold)]
    pub bold: Prop<World, bool>,
    #[elem(default = Face::Body, patch = SetFace)]
    pub face: Prop<World, Face>,
    /// 1.0, fully opaque, when unset.
    #[elem(default = 1.0, patch = SetOpacity, blend = blend)]
    pub opacity: Prop<World, f32>,
    #[elem(default = (0.0, 0.0), patch = SetOffset, blend = shift)]
    pub offset: Prop<World, (f32, f32)>,
}

/// A button in the engine's style `style`: showing `text`, or holding
/// an icon and a label.
#[element(backend = Hamlet)]
pub struct Button {
    #[elem(patch = SetText)]
    pub text: Prop<World, String>,
    #[elem(default = ButtonStyle::Plain, patch = SetStyle)]
    pub style: Prop<World, ButtonStyle>,
    #[elem(default = 1.0, patch = SetOpacity, blend = blend)]
    pub opacity: Prop<World, f32>,
    #[elem(default = (0.0, 0.0), patch = SetOffset, blend = shift)]
    pub offset: Prop<World, (f32, f32)>,
}

/// A column or a row, holding no views of its own.
#[element(backend = Hamlet)]
pub struct Frame {
    #[elem(default = Direction::Column, patch = SetDirection)]
    pub direction: Prop<World, Direction>,
    #[elem(default = Space::Gap, patch = SetSpace, blend = jump)]
    pub space: Prop<World, Space>,
    #[elem(default = 1.0, patch = SetOpacity, blend = blend)]
    pub opacity: Prop<World, f32>,
    #[elem(default = (0.0, 0.0), patch = SetOffset, blend = shift)]
    pub offset: Prop<World, (f32, f32)>,
}

/// An icon of the engine's set, by its name.
#[element(backend = Hamlet)]
pub struct Icon {
    #[elem(patch = SetText)]
    pub name: Prop<World, String>,
    #[elem(default = Tone::Body, patch = SetTone, blend = jump)]
    pub tone: Prop<World, Tone>,
    #[elem(default = 1.0, patch = SetOpacity, blend = blend)]
    pub opacity: Prop<World, f32>,
    #[elem(default = (0.0, 0.0), patch = SetOffset, blend = shift)]
    pub offset: Prop<World, (f32, f32)>,
}

/// A switch showing `on`.
#[element(backend = Hamlet)]
pub struct Toggle {
    #[elem(patch = SetOn)]
    pub on: Prop<World, bool>,
    #[elem(default = 1.0, patch = SetOpacity, blend = blend)]
    pub opacity: Prop<World, f32>,
    #[elem(default = (0.0, 0.0), patch = SetOffset, blend = shift)]
    pub offset: Prop<World, (f32, f32)>,
}

/// A line of text to edit, showing `placeholder` while it is empty.
#[element(backend = Hamlet)]
pub struct Field {
    #[elem(patch = SetText)]
    pub text: Prop<World, String>,
    #[elem(patch = SetPlaceholder)]
    pub placeholder: Prop<World, String>,
    #[elem(default = 1.0, patch = SetOpacity, blend = blend)]
    pub opacity: Prop<World, f32>,
    #[elem(default = (0.0, 0.0), patch = SetOffset, blend = shift)]
    pub offset: Prop<World, (f32, f32)>,
}

/// The rows of a list, a divider between each, holding no views of
/// its own.
#[element(backend = Hamlet)]
pub struct List {
    #[elem(default = 1.0, patch = SetOpacity, blend = blend)]
    pub opacity: Prop<World, f32>,
    #[elem(default = (0.0, 0.0), patch = SetOffset, blend = shift)]
    pub offset: Prop<World, (f32, f32)>,
}

/// A column that scrolls, holding no views of its own.
#[element(backend = Hamlet)]
pub struct Scroll {
    #[elem(default = 1.0, patch = SetOpacity, blend = blend)]
    pub opacity: Prop<World, f32>,
    #[elem(default = (0.0, 0.0), patch = SetOffset, blend = shift)]
    pub offset: Prop<World, (f32, f32)>,
}

/// A line across.
#[element(backend = Hamlet)]
pub struct Divider {
    #[elem(default = 1.0, patch = SetOpacity, blend = blend)]
    pub opacity: Prop<World, f32>,
    #[elem(default = (0.0, 0.0), patch = SetOffset, blend = shift)]
    pub offset: Prop<World, (f32, f32)>,
}

pub fn label(text: impl Into<Prop<World, String>>) -> Label {
    Label {
        text: text.into(),
        ..Label::unset()
    }
}

/// The name at the head of a panel.
pub fn heading(text: impl Into<Prop<World, String>>) -> Label {
    label(text).size(Size::Title).bold(true).face(Face::Display)
}

pub fn button(text: impl Into<Prop<World, String>>) -> Button {
    Button {
        text: text.into(),
        ..Button::unset()
    }
}

/// A button with the icon `name` before `text`.
pub fn icon_button(
    name: impl Into<Prop<World, String>>,
    text: impl Into<Prop<World, String>>,
) -> Stack<Button, (Icon, Label)> {
    Stack {
        element: Button::unset(),
        children: (icon(name), label(text).face(Face::Display)),
    }
}

/// The icon of the engine's set named `name`, as `Glyph::name` has
/// it.
pub fn icon(name: impl Into<Prop<World, String>>) -> Icon {
    Icon {
        name: name.into(),
        ..Icon::unset()
    }
}

/// A toggle showing `on`; [`on_toggle`](UiExt::on_toggle) runs as the
/// person switches it.
pub fn toggle(on: impl Into<Prop<World, bool>>) -> Toggle {
    Toggle {
        on: on.into(),
        ..Toggle::unset()
    }
}

/// A field showing `text`; [`on_commit`](UiExt::on_commit) runs with
/// what the person leaves in it.
pub fn field(text: impl Into<Prop<World, String>>) -> Field {
    Field {
        text: text.into(),
        ..Field::unset()
    }
}

pub fn divider() -> Divider {
    Divider::unset()
}

/// An element with `children` built under it, in order.
pub struct Stack<E, C> {
    pub element: E,
    pub children: C,
}

pub fn column<C>(children: C) -> Stack<Frame, C> {
    Stack {
        element: Frame::unset().direction(Direction::Column),
        children,
    }
}

pub fn row<C>(children: C) -> Stack<Frame, C> {
    Stack {
        element: Frame::unset().direction(Direction::Row),
        children,
    }
}

/// `children` as the rows of a list.
pub fn list<C>(children: C) -> Stack<List, C> {
    Stack {
        element: List::unset(),
        children,
    }
}

/// `children` in a column that scrolls.
pub fn scroll<C>(children: C) -> Stack<Scroll, C> {
    Stack {
        element: Scroll::unset(),
        children,
    }
}

impl<C> FrameProps for Stack<Frame, C> {
    fn frame_mut(&mut self) -> &mut Frame {
        &mut self.element
    }
}

impl<C> ButtonProps for Stack<Button, C> {
    fn button_mut(&mut self) -> &mut Button {
        &mut self.element
    }
}

impl<C> ListProps for Stack<List, C> {
    fn list_mut(&mut self) -> &mut List {
        &mut self.element
    }
}

impl<C> ScrollProps for Stack<Scroll, C> {
    fn scroll_mut(&mut self) -> &mut Scroll {
        &mut self.element
    }
}

impl<T, E, C> View<Hamlet, T> for Stack<E, C>
where
    T: 'static,
    E: View<Hamlet, T>,
    C: ViewSeq<Hamlet, T>,
{
    fn build(self, cx: &mut Cx<'_, Hamlet, T>) -> Key {
        let node = cx.build(self.element);
        cx.under(node, |cx| self.children.build_each(cx));
        node
    }
}

/// A view whose root node runs a handler on what the person does to
/// it.
pub struct OnInput<V> {
    inner: V,
    handler: Handler,
}

impl<T, V: View<Hamlet, T>> View<Hamlet, T> for OnInput<V> {
    fn build(self, cx: &mut Cx<'_, Hamlet, T>) -> Key {
        let node = self.inner.build(cx);
        cx.world.handlers.insert(node, self.handler);
        node
    }
}

/// What any view of a panel can be given.
pub trait UiExt: Sized {
    /// This view, running `handler` when its root node, a button, is
    /// clicked.
    fn on_click(
        self,
        handler: impl Fn(&mut World) + Send + Sync + 'static,
    ) -> OnInput<Self> {
        OnInput {
            inner: self,
            handler: Handler::Click(Box::new(handler)),
        }
    }

    /// This view, running `handler` with the state asked for when its
    /// root node, a toggle, is switched.
    fn on_toggle(
        self,
        handler: impl Fn(&mut World, bool) + Send + Sync + 'static,
    ) -> OnInput<Self> {
        OnInput {
            inner: self,
            handler: Handler::Toggle(Box::new(handler)),
        }
    }

    /// This view, running `handler` with the text the person left in
    /// its root node, a field.
    fn on_commit(
        self,
        handler: impl Fn(&mut World, String) + Send + Sync + 'static,
    ) -> OnInput<Self> {
        OnInput {
            inner: self,
            handler: Handler::Commit(Box::new(handler)),
        }
    }
}

impl<V> UiExt for V {}

/// The kind of node the element of the type `element` is. A node no
/// element is written on is a frame.
pub(super) fn kind(element: Option<TypeId>) -> Kind {
    let Some(element) = element else {
        return Kind::Frame;
    };
    [
        (TypeId::of::<Label>(), Kind::Label),
        (TypeId::of::<Button>(), Kind::Button),
        (TypeId::of::<Icon>(), Kind::Icon),
        (TypeId::of::<Toggle>(), Kind::Toggle),
        (TypeId::of::<Field>(), Kind::Field),
        (TypeId::of::<List>(), Kind::List),
        (TypeId::of::<Scroll>(), Kind::Scroll),
        (TypeId::of::<Divider>(), Kind::Divider),
    ]
    .into_iter()
    .find_map(|(id, kind)| (id == element).then_some(kind))
    .unwrap_or(Kind::Frame)
}

fn blend(from: &f32, to: &f32, t: f32) -> f32 {
    from + (to - from) * t
}

fn shift(from: &(f32, f32), to: &(f32, f32), t: f32) -> (f32, f32) {
    (blend(&from.0, &to.0, t), blend(&from.1, &to.1, t))
}

/// A value the engine moves, and the plugin sets straight to its
/// target.
fn jump<T: Copy>(_: &T, to: &T, _: f32) -> T {
    *to
}

/// A [`Patch`] sending one property of a node, over the motion of a
/// transition where the engine moves it.
macro_rules! patch {
    ($name:ident, $value:ty, $make:expr) => {
        pub struct $name;

        impl Patch<Hamlet, $value> for $name {
            fn patch(world: &mut World, node: Key, value: &$value) {
                Self::patch_over(world, node, value, None);
            }

            fn patch_over(
                world: &mut World,
                node: Key,
                value: &$value,
                motion: Option<fynix::Motion>,
            ) {
                let motion = motion.map(|motion| match motion {
                    fynix::Motion::Interact => Movement::Interact,
                    fynix::Motion::Expand => Movement::Expand,
                });
                world.nodes.set(node, $make(value), motion);
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
patch!(SetFace, Face, |face: &Face| Property::Face(*face));
patch!(SetOpacity, f32, |opacity: &f32| Property::Opacity(*opacity));
patch!(SetStyle, ButtonStyle, |style: &ButtonStyle| {
    Property::Style(*style)
});
patch!(SetDirection, Direction, |direction: &Direction| {
    Property::Direction(*direction)
});
patch!(SetSpace, Space, |space: &Space| Property::Space(*space));
patch!(SetPlaceholder, String, |text: &String| {
    Property::Placeholder(text.clone())
});
patch!(SetOn, bool, |on: &bool| Property::On(*on));
patch!(SetOffset, (f32, f32), |offset: &(f32, f32)| {
    Property::Offset(*offset)
});
