use crate::inbox::Inbox;
use crate::interface::exports::hamlet::plugin::plugin::{
    Incoming, Node, Tree, Widget,
};
use crate::interface::hamlet::plugin::host::Level;
use crate::limits::Limits;
use crate::{fixture, wire};

fn column(texts: &[&str]) -> Tree {
    let node = |widget, children| Node {
        id: None,
        widget,
        children,
    };
    let mut nodes = vec![node(Widget::Column, texts.len() as u32)];
    nodes.extend(
        texts
            .iter()
            .map(|text| node(Widget::Label(text.to_string()), 0)),
    );
    Tree { nodes }
}

#[test]
fn a_tree_with_too_much_text_fails() {
    let limits = Limits {
        text_bytes: 8,
        tree_text_bytes: 12,
        ..Limits::default()
    };

    assert!(wire::node(column(&["12345", "12345"]), &limits).is_ok());
    let long = wire::node(column(&["123456789"]), &limits);
    assert!(long.unwrap_err().contains("a text has 9 bytes"));
    let many = wire::node(column(&["1234567", "1234567"]), &limits);
    assert!(many.unwrap_err().contains("12 bytes of text"));
}

#[test]
fn intents_over_the_limit_are_dropped_with_a_warning() {
    let limits = Limits {
        intents_per_call: 0,
        ..Limits::default()
    };
    let mut plugin = super::load(&fixture::echo()).unwrap();

    let out = plugin.step(&limits, &[Incoming::Tick(1)]).unwrap();

    assert!(out.intents.is_empty());
    assert!(out.logs.iter().any(|line| {
        line.level == Level::Warn && line.message.contains("dropped")
    }));
}

#[test]
fn events_over_the_rate_are_not_given_to_the_plugin() {
    let mut inbox = Inbox::default();

    assert!((0..3).all(|_| inbox.admit(0.0, 3)));
    assert!(!inbox.admit(0.5, 3));
    assert!(inbox.admit(1.0, 3));
}
