use bevy::ecs::message::{MessageCursor, Messages};
use bevy::prelude::*;
use hamlet_sdk::plugin::{Disable, Enable};

use crate::fixture;
use crate::fixture::home::{self, Plugin, Scratch};
use crate::reload::Polling;
use crate::run::Said;

/// The lengths of the lines the plugin said in the next frame.
fn said(
    app: &mut App,
    cursor: &mut MessageCursor<Said>,
) -> Vec<usize> {
    app.update();
    let messages = app.world().resource::<Messages<Said>>();
    cursor.read(messages).map(|said| said.text.len()).collect()
}

fn component(keep: u32) -> Vec<u8> {
    fixture::component(&fixture::remembering(keep))
}

#[test]
fn the_state_a_plugin_holds_is_kept_across_a_reload_and_a_switch_off()
{
    let scratch = Scratch::new("state");
    let file = scratch.plugins().join("probe/plugin.wasm");
    let mut app =
        home::app(&scratch, &[Plugin::new("probe", component(6))]);
    app.world_mut().resource_mut::<Polling>().poll_ms = 0;
    let mut cursor = app
        .world()
        .resource::<Messages<Said>>()
        .get_cursor_current();

    assert_eq!(said(&mut app, &mut cursor), [0]);
    assert_eq!(said(&mut app, &mut cursor), [6]);

    std::fs::write(&file, component(3)).unwrap();
    let lines = said(&mut app, &mut cursor);
    assert_eq!(lines, [6], "the new version starts with the state");
    assert_eq!(said(&mut app, &mut cursor), [3]);

    app.world_mut().trigger(Disable("probe".into()));
    app.world_mut().flush();
    assert!(said(&mut app, &mut cursor).is_empty());
    app.world_mut().trigger(Enable("probe".into()));
    app.world_mut().flush();
    assert_eq!(said(&mut app, &mut cursor), [3]);
}
