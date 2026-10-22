//! The elements a plugin's panels are made of, each a node of the
//! engine's of one kind, and what is built out of them.

use core::any::TypeId;
use std::sync::Arc;

use fynix::{Cx, Patch, Prop, Styled, View, ViewSeq, element};

use crate::bindings::hamlet::engine::ui::{
    Align, ButtonStyle, Direction, Face, Fill, Fit, Kind, Menu,
    MenuItem as Item, Motion as Movement, NodePointer, PointerMode,
    Property, Size, Space, Tone,
};
use crate::ui::world::{Handler, World};
use crate::ui::{Hamlet, Key, States};

/// A run of text.
#[element(backend = Hamlet)]
pub struct Label {
    #[elem(patch = SetText)]
    pub text: Prop<World, String>,
    #[elem(optional, patch = SetTone, blend = jump)]
    pub tone: Prop<World, Tone>,
    #[elem(optional, patch = SetSize, blend = jump)]
    pub size: Prop<World, Size>,
    #[elem(optional, patch = SetBold)]
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
    /// The title of the system's dialog a press opens to choose a
    /// folder in ([`on_folder`](UiExt::on_folder)), none if empty.
    #[elem(patch = SetFolder)]
    pub folder: Prop<World, String>,
    #[elem(default = 1.0, patch = SetOpacity, blend = blend)]
    pub opacity: Prop<World, f32>,
    #[elem(default = (0.0, 0.0), patch = SetOffset, blend = shift)]
    pub offset: Prop<World, (f32, f32)>,
}

/// A column or a row, holding no views of its own: its children at
/// its start or its end, drawn on its `fill`.
#[element(backend = Hamlet)]
pub struct Frame {
    #[elem(default = Direction::Column, patch = SetDirection)]
    pub direction: Prop<World, Direction>,
    #[elem(default = Space::Gap, patch = SetSpace, blend = jump)]
    pub space: Prop<World, Space>,
    #[elem(default = Align::Start, patch = SetAlign)]
    pub align: Prop<World, Align>,
    #[elem(default = Fill::None, patch = SetFill)]
    pub fill: Prop<World, Fill>,
    /// Room around its children, past what its fill pads it by.
    #[elem(default = Space::None, patch = SetPadding)]
    pub padding: Prop<World, Space>,
    /// Its share of the room left along what holds it, none at 0.
    #[elem(default = 0.0, patch = SetGrow)]
    pub grow: Prop<World, f32>,
    /// Its height in pixels, what it holds laid out in it; at 0 it
    /// is as tall as what it holds and its share of the room make
    /// it.
    #[elem(default = 0.0, patch = SetHeight)]
    pub height: Prop<World, f32>,
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
    #[elem(optional, patch = SetTone, blend = jump)]
    pub tone: Prop<World, Tone>,
    #[elem(default = 1.0, patch = SetOpacity, blend = blend)]
    pub opacity: Prop<World, f32>,
    #[elem(default = (0.0, 0.0), patch = SetOffset, blend = shift)]
    pub offset: Prop<World, (f32, f32)>,
}

/// A switch showing `on`, and `text` beside it unless it is empty.
#[element(backend = Hamlet)]
pub struct Toggle {
    #[elem(patch = SetOn)]
    pub on: Prop<World, bool>,
    #[elem(patch = SetText)]
    pub text: Prop<World, String>,
    #[elem(default = 1.0, patch = SetOpacity, blend = blend)]
    pub opacity: Prop<World, f32>,
    #[elem(default = (0.0, 0.0), patch = SetOffset, blend = shift)]
    pub offset: Prop<World, (f32, f32)>,
}

/// Text to edit, showing `placeholder` while it is empty. With more
/// than one of `lines`, it grows to them as it is typed in, Enter
/// sends what is typed ([`on_submit`](UiExt::on_submit)) and
/// Shift+Enter adds a line.
#[element(backend = Hamlet)]
pub struct Field {
    #[elem(patch = SetText)]
    pub text: Prop<World, String>,
    #[elem(patch = SetPlaceholder)]
    pub placeholder: Prop<World, String>,
    /// 1 when unset.
    #[elem(default = 1, patch = SetLines)]
    pub lines: Prop<World, u32>,
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

/// A column that scrolls, holding no views of its own. One that
/// `stick`s keeps to its end as it grows, until the person scrolls
/// up, and tells when it reaches its top
/// ([`on_top`](UiExt::on_top)).
#[element(backend = Hamlet)]
pub struct Scroll {
    #[elem(patch = SetStick)]
    pub stick: Prop<World, bool>,
    #[elem(default = 1.0, patch = SetOpacity, blend = blend)]
    pub opacity: Prop<World, f32>,
    #[elem(default = (0.0, 0.0), patch = SetOffset, blend = shift)]
    pub offset: Prop<World, (f32, f32)>,
}

/// One of `options` shown, the one at `selected`, which opens a list
/// of them to pick another from.
#[element(backend = Hamlet)]
pub struct Dropdown {
    #[elem(patch = SetOptions)]
    pub options: Prop<World, Vec<String>>,
    #[elem(patch = SetSelected)]
    pub selected: Prop<World, u32>,
    #[elem(default = 1.0, patch = SetOpacity, blend = blend)]
    pub opacity: Prop<World, f32>,
    #[elem(default = (0.0, 0.0), patch = SetOffset, blend = shift)]
    pub offset: Prop<World, (f32, f32)>,
}

/// A picture, a PNG, JPEG or WebP, laid in its `extent` by its
/// `fit`. Pictures that compare equal are sent once.
#[element(backend = Hamlet)]
pub struct Image {
    #[elem(patch = SetPicture)]
    pub picture: Prop<World, Arc<[u8]>>,
    /// Its width and height in the interface's pixels. One at 0 is
    /// taken from the other by the picture's proportion; both at 0,
    /// the picture's own size in pixels.
    #[elem(default = (0.0, 0.0), patch = SetExtent)]
    pub extent: Prop<World, (f32, f32)>,
    #[elem(default = Fit::Contain, patch = SetFit)]
    pub fit: Prop<World, Fit>,
    /// Its corners rounded by the theme's radius.
    #[elem(patch = SetRounded)]
    pub rounded: Prop<World, bool>,
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

/// A button holding `children`, such as a card that leads somewhere.
pub fn pressable<C>(children: C) -> Stack<Button, C> {
    Stack {
        element: Button::unset(),
        children,
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

/// A switch showing `on`; [`on_toggle`](UiExt::on_toggle) runs as the
/// person switches it.
pub fn toggle(on: impl Into<Prop<World, bool>>) -> Toggle {
    Toggle {
        on: on.into(),
        ..Toggle::unset()
    }
}

/// A [`toggle`] with `text` beside it, pressed as one with it.
pub fn switch(
    text: impl Into<Prop<World, String>>,
    on: impl Into<Prop<World, bool>>,
) -> Toggle {
    toggle(on).text(text)
}

/// A field showing `text`; [`on_commit`](UiExt::on_commit) runs with
/// what the person leaves in it.
pub fn field(text: impl Into<Prop<World, String>>) -> Field {
    Field {
        text: text.into(),
        ..Field::unset()
    }
}

/// The picture `picture`, a PNG, JPEG or WebP, or none if empty.
pub fn image(picture: impl Into<Prop<World, Arc<[u8]>>>) -> Image {
    Image {
        picture: picture.into(),
        ..Image::unset()
    }
}

pub fn divider() -> Divider {
    Divider::unset()
}

/// A dropdown offering `options`, showing the one at `selected`;
/// [`on_pick`](UiExt::on_pick) runs with the place of the one the
/// person picks.
pub fn dropdown(
    options: impl Into<Prop<World, Vec<String>>>,
    selected: impl Into<Prop<World, u32>>,
) -> Dropdown {
    Dropdown {
        options: options.into(),
        selected: selected.into(),
        ..Dropdown::unset()
    }
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
        cx.world
            .handlers
            .entry(node)
            .or_default()
            .push(self.handler);
        node
    }
}

/// A view whose root node leads to an address, told to the desktop
/// service while the pointer is over it.
pub struct Leads<V> {
    inner: V,
    address: String,
}

impl<T, V: View<Hamlet, T>> View<Hamlet, T> for Leads<V> {
    fn build(self, cx: &mut Cx<'_, Hamlet, T>) -> Key {
        let node = self.inner.build(cx);
        cx.world.addresses.insert(node, self.address);
        cx.world.nodes.watch(node, States::HOVER);
        node
    }
}

/// A view whose root node shows words near the pointer while it
/// rests on it.
pub struct Tipped<V> {
    inner: V,
    tip: Prop<World, String>,
}

impl<T: 'static, V: View<Hamlet, T>> View<Hamlet, T> for Tipped<V> {
    fn build(self, cx: &mut Cx<'_, Hamlet, T>) -> Key {
        let node = self.inner.build(cx);
        cx.effect(node, self.tip, |world, node, tip: &String| {
            world.nodes.set(node, Property::Tip(tip.clone()), None);
        });
        node
    }
}

/// A view whose root node asks for a pointer of the engine's set
/// while the pointer is over it.
pub struct Pointed<V> {
    inner: V,
    pointer: Prop<World, Option<NodePointer>>,
}

impl<T: 'static, V: View<Hamlet, T>> View<Hamlet, T> for Pointed<V> {
    fn build(self, cx: &mut Cx<'_, Hamlet, T>) -> Key {
        let node = self.inner.build(cx);
        cx.effect(node, self.pointer, |world, node, pointer| {
            world.nodes.set(node, Property::Pointer(*pointer), None);
        });
        node
    }
}

impl NodePointer {
    /// The pointer `mode`, drawn whole.
    pub fn new(mode: PointerMode) -> Self {
        Self { mode, faint: false }
    }

    /// It drawn faint, so what is under it is seen through it.
    pub fn faint(self) -> Self {
        Self {
            faint: true,
            ..self
        }
    }
}

impl PartialEq for NodePointer {
    fn eq(&self, other: &Self) -> bool {
        self.mode == other.mode && self.faint == other.faint
    }
}

/// One item of a node's menu, and what choosing it does.
pub struct MenuItem {
    text: String,
    group: String,
    critical: bool,
    run: Box<dyn Fn(&mut World) + Send + Sync>,
}

/// An item of a menu showing `text`, running `run` when it is
/// chosen.
pub fn menu_item(
    text: impl Into<String>,
    run: impl Fn(&mut World) + Send + Sync + 'static,
) -> MenuItem {
    MenuItem {
        text: text.into(),
        group: String::new(),
        critical: false,
        run: Box::new(run),
    }
}

impl MenuItem {
    /// This item, under the item `group`, which opens the items
    /// under it beside the menu.
    pub fn under(mut self, group: impl Into<String>) -> Self {
        self.group = group.into();
        self
    }

    /// This item, as one that refuses, stops or throws away.
    pub fn critical(mut self) -> Self {
        self.critical = true;
        self
    }
}

/// A view whose root node opens a menu.
pub struct Menued<V> {
    inner: V,
    items: Vec<MenuItem>,
    click: bool,
}

impl<T, V: View<Hamlet, T>> View<Hamlet, T> for Menued<V> {
    fn build(self, cx: &mut Cx<'_, Hamlet, T>) -> Key {
        let node = self.inner.build(cx);
        let mut runs = Vec::with_capacity(self.items.len());
        let items = self
            .items
            .into_iter()
            .map(|item| {
                runs.push(item.run);
                Item {
                    text: item.text,
                    group: item.group,
                    critical: item.critical,
                }
            })
            .collect();
        let menu = Menu {
            items,
            click: self.click,
        };
        cx.world.nodes.set(node, Property::Menu(menu), None);
        let chose = move |world: &mut World, at: u32| {
            if let Some(run) = runs.get(at as usize) {
                run(world);
            }
        };
        cx.world
            .handlers
            .entry(node)
            .or_default()
            .push(Handler::Chose(Box::new(chose)));
        node
    }
}

/// What any view of a panel can be given.
pub trait UiExt: Sized {
    /// This view, opening a menu of `items` at the pointer on a
    /// right-click on its root node.
    fn menu(self, items: Vec<MenuItem>) -> Menued<Self> {
        Menued {
            inner: self,
            items,
            click: false,
        }
    }

    /// This view, opening a menu of `items` at the pointer on a
    /// click or a right-click on its root node.
    fn menu_on_click(self, items: Vec<MenuItem>) -> Menued<Self> {
        Menued {
            inner: self,
            items,
            click: true,
        }
    }

    /// This view, showing `tip` near the pointer while it rests on
    /// its root node.
    fn tip(
        self,
        tip: impl Into<Prop<World, String>>,
    ) -> Tipped<Self> {
        Tipped {
            inner: self,
            tip: tip.into(),
        }
    }

    /// This view, showing `pointer` while the pointer is over its
    /// root node, unless what the node holds asks for another; none
    /// leaves it to what holds the node.
    fn pointer(
        self,
        pointer: impl Into<Prop<World, Option<NodePointer>>>,
    ) -> Pointed<Self> {
        Pointed {
            inner: self,
            pointer: pointer.into(),
        }
    }

    /// This view, its root node leading to `address`: the status
    /// line shows it while the pointer is over the node.
    fn leads_to(self, address: impl Into<String>) -> Leads<Self> {
        Leads {
            inner: self,
            address: address.into(),
        }
    }

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

    /// This view, running `handler` with what Enter sent from its
    /// root node, a field of several lines.
    fn on_submit(
        self,
        handler: impl Fn(&mut World, String) + Send + Sync + 'static,
    ) -> OnInput<Self> {
        OnInput {
            inner: self,
            handler: Handler::Submit(Box::new(handler)),
        }
    }

    /// This view, running `handler` with the path of the folder the
    /// person chose in the dialog its root node, a button with a
    /// `folder`, opened.
    fn on_folder(
        self,
        handler: impl Fn(&mut World, String) + Send + Sync + 'static,
    ) -> OnInput<Self> {
        OnInput {
            inner: self,
            handler: Handler::Folder(Box::new(handler)),
        }
    }

    /// This view, running `handler` when its root node, a scroll that
    /// sticks, reaches its top.
    fn on_top(
        self,
        handler: impl Fn(&mut World) + Send + Sync + 'static,
    ) -> OnInput<Self> {
        OnInput {
            inner: self,
            handler: Handler::Top(Box::new(handler)),
        }
    }

    /// This view, running `handler` with the place of the option the
    /// person picks in its root node, a dropdown.
    fn on_pick(
        self,
        handler: impl Fn(&mut World, u32) + Send + Sync + 'static,
    ) -> OnInput<Self> {
        OnInput {
            inner: self,
            handler: Handler::Pick(Box::new(handler)),
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
        (TypeId::of::<Dropdown>(), Kind::Dropdown),
        (TypeId::of::<Image>(), Kind::Image),
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

/// The motion of fynix's `motion` as the engine names it.
fn movement(motion: Option<fynix::Motion>) -> Option<Movement> {
    motion.map(|motion| match motion {
        fynix::Motion::Interact => Movement::Interact,
        fynix::Motion::Expand => Movement::Expand,
    })
}

/// A [`Patch`] sending one property of a node, over the motion of a
/// transition where the engine moves it. One of an `optional` prop,
/// which the engine starts by where the node is, such as bold in a
/// button, is sent whenever the plugin sets it, and `$unset` while it
/// is unset once it was sent.
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
                world.nodes.set(node, $make(value), movement(motion));
            }
        }
    };
    ($name:ident, optional $value:ty, $unset:expr, $make:expr) => {
        pub struct $name;

        impl Patch<Hamlet, Option<$value>> for $name {
            fn patch(
                world: &mut World,
                node: Key,
                value: &Option<$value>,
            ) {
                Self::patch_over(world, node, value, None);
            }

            fn patch_over(
                world: &mut World,
                node: Key,
                value: &Option<$value>,
                motion: Option<fynix::Motion>,
            ) {
                let nodes = &mut world.nodes;
                match value {
                    Some(value) => nodes.send(
                        node,
                        $make(value),
                        movement(motion),
                    ),
                    None => nodes.set(
                        node,
                        $make(&$unset),
                        movement(motion),
                    ),
                }
            }
        }
    };
}

patch!(SetText, String, |text: &String| Property::Text(
    text.clone()
));
patch!(SetTone, optional Tone, Tone::Body, |tone: &Tone| {
    Property::Tone(*tone)
});
patch!(SetSize, optional Size, Size::Body, |size: &Size| {
    Property::Size(*size)
});
patch!(SetBold, optional bool, false, |bold: &bool| {
    Property::Bold(*bold)
});
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
patch!(SetOptions, Vec<String>, |options: &Vec<String>| {
    Property::Options(options.clone())
});
patch!(SetSelected, u32, |at: &u32| Property::Selected(*at));
patch!(SetLines, u32, |lines: &u32| Property::Lines(*lines));
patch!(SetStick, bool, |stick: &bool| Property::Stick(*stick));
patch!(SetAlign, Align, |align: &Align| Property::Align(*align));
patch!(SetFill, Fill, |fill: &Fill| Property::Fill(*fill));
patch!(SetPadding, Space, |space: &Space| Property::Padding(*space));
patch!(SetGrow, f32, |grow: &f32| Property::Grow(*grow));
patch!(SetHeight, f32, |height: &f32| Property::Height(*height));
patch!(SetFolder, String, |title: &String| {
    Property::Folder(title.clone())
});
patch!(SetPicture, Arc<[u8]>, |picture: &Arc<[u8]>| {
    Property::Picture(picture.to_vec())
});
patch!(SetExtent, (f32, f32), |extent: &(f32, f32)| {
    Property::Extent(*extent)
});
patch!(SetFit, Fit, |fit: &Fit| Property::Fit(*fit));
patch!(SetRounded, bool, |rounded: &bool| Property::Rounded(
    *rounded
));
