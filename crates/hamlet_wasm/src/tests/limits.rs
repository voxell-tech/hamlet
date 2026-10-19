use std::time::Duration;

use bevy::ecs::entity::Entity;
use wasmtime::ResourceLimiter;

use crate::budget::Spent;
use crate::inbox::{Inbox, Ticks};
use crate::interface::exports::hamlet::plugin::plugin::{
    Canvas, CircleShape, Incoming, Node, Paint, RectShape, Shape,
    Tree, Widget,
};
use crate::interface::hamlet::plugin::host::Level;
use crate::limits::{Limiter, Limits};
use crate::manifest::Manifest;
use crate::rate::Rate;
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
fn a_canvas_is_held_to_its_size_its_shapes_and_the_theme_colours() {
    let rect = |paint| {
        Shape::Rect(RectShape {
            x: 0.0,
            y: 0.0,
            w: 4.0,
            h: 4.0,
            paint,
        })
    };
    let ink = || Paint::Palette("ink".into());
    let drawn = |width, shapes| Tree {
        nodes: vec![Node {
            id: None,
            widget: Widget::Canvas(Canvas {
                width,
                height: 10,
                shapes,
            }),
            children: 0,
        }],
    };
    let limits = Limits {
        canvas_shapes: 2,
        canvas_px: 100,
        ..Limits::default()
    };

    assert!(
        wire::node(
            drawn(100, vec![rect(ink()), rect(ink())]),
            &limits
        )
        .is_ok()
    );
    for (width, shapes, said) in [
        (101, vec![], "100"),
        (10, vec![rect(ink()); 3], "more than 2 shapes"),
        (10, vec![rect(Paint::Palette("neon".into()))], "neon"),
        (
            10,
            vec![Shape::Circle(CircleShape {
                x: f32::NAN,
                y: 0.0,
                r: 1.0,
                paint: ink(),
            })],
            "not a number",
        ),
    ] {
        let why =
            wire::node(drawn(width, shapes), &limits).unwrap_err();
        assert!(why.contains(said), "{why}");
    }
    let mut root =
        wire::node(drawn(10, vec![rect(ink())]), &limits).unwrap();
    assert!(wire::without_canvases(&mut root));
    assert!(root.text().contains("ui.canvas"));
}

#[test]
fn intents_over_the_limit_are_dropped_with_a_warning() {
    let limits = Limits {
        intents_per_call: 0,
        ..Limits::default().patient()
    };
    let mut plugin = super::load(&fixture::echo()).unwrap();

    let out = plugin.step(&limits, &[Incoming::Tick(1)]).unwrap();

    assert!(out.intents.is_empty());
    assert!(out.logs.iter().any(|line| {
        line.level == Level::Warn && line.message.contains("dropped")
    }));
}

#[test]
fn the_memories_of_a_plugin_share_one_limit() {
    let mut limiter = Limiter::new(&Limits {
        memory_mib: 1,
        ..Limits::default()
    });
    assert!(limiter.memory_growing(0, 600 << 10, None).is_ok());
    assert!(limiter.memory_growing(0, 600 << 10, None).is_err());
    assert!(limiter.memory_growing(0, 400 << 10, None).is_ok());
}

#[test]
fn a_plugin_that_spent_its_second_waits_for_the_next() {
    let limits = Limits::default();
    let mut spent = Spent::default();
    assert!(!spent.over(&limits));
    spent.charge(
        Duration::from_millis(limits.plugin_ms_per_second / 2),
        0,
    );
    assert!(!spent.over(&limits));
    spent.charge(Duration::ZERO, limits.plugin_fuel_per_second);
    assert!(spent.over(&limits));
}

#[test]
fn ticks_come_at_the_rate_to_the_plugins_that_want_them() {
    let manifest = |extra: &str| {
        let text = format!(
            "id = \"a\"\nname = \"A\"\nversion = \"0.1.0\"\n{extra}"
        );
        Ticks::of(&Manifest::parse(&text).unwrap())
    };
    let mut default = manifest("");
    assert!(default.due(Some(0.0), 30));
    assert!(!default.due(Some(0.01), 30));
    assert!(default.due(Some(0.04), 30));
    assert!(default.due(None, 30));
    assert!(!manifest("ticks = false").due(Some(0.0), 30));
}

#[test]
fn events_published_over_the_rate_are_counted_out() {
    let mut rate = Rate::default();
    let plugin = Entity::PLACEHOLDER;

    assert!((0..2).all(|_| rate.within(plugin, 0.0, 1.0, 2)));
    assert!(!rate.within(plugin, 0.5, 1.0, 2));
    assert!(rate.within(plugin, 1.0, 1.0, 2));
}

#[test]
fn events_over_the_rate_are_not_given_to_the_plugin() {
    let mut inbox = Inbox::default();

    assert!((0..3).all(|_| inbox.admit(0.0, 3)));
    assert!(!inbox.admit(0.5, 3));
    assert!(inbox.admit(1.0, 3));
}
