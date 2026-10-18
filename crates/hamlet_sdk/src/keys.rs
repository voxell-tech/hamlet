//! Keyboard shortcuts: a chord pressed runs a command.

pub mod chord;
mod key;
mod model;
mod press;

use bevy::app::{App, Update};
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::Res;
use bevy::input::ButtonInput;
use bevy::input::keyboard::KeyCode;

pub use self::key::Key;
pub use self::model::{Bound, Keys, Standing};

/// Keeps [`Keys`] in step with the [`Key`] entities there are, and
/// runs the command of a chord that is pressed.
pub fn plugin(app: &mut App) {
    app.init_resource::<Keys>().add_systems(
        Update,
        (
            model::keep.run_if(model::moved),
            press::press.run_if(
                |keys: Option<Res<ButtonInput<KeyCode>>>| {
                    keys.is_some_and(|keys| {
                        keys.get_just_pressed().len() > 0
                    })
                },
            ),
        )
            .chain(),
    );
}

#[cfg(test)]
mod tests {
    use bevy::app::App;
    use bevy::ecs::observer::On;
    use bevy::ecs::resource::Resource;
    use bevy::ecs::system::ResMut;
    use bevy::input_focus::{FocusCause, InputFocus};
    use bevy::text::EditableText;
    use bevy_fynix::shortcut::{Chord, Mods};

    use super::*;
    use crate::capability::{Capability, Granted};
    use crate::command::{Command, RunCommand};
    use crate::plugin::{ContributedBy, PluginInfo};

    #[derive(Resource, Default)]
    struct Ran(Vec<String>);

    fn app() -> App {
        let mut app = App::new();
        app.init_resource::<Ran>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<InputFocus>()
            .add_plugins(plugin)
            .add_observer(
                |run: On<RunCommand>, mut ran: ResMut<Ran>| {
                    ran.0.push(run.id.clone())
                },
            );
        app
    }

    /// A plugin with a command `go` and a key for it.
    fn bind(app: &mut App, id: &str, builtin: bool, chord: &str) {
        let world = app.world_mut();
        let info = PluginInfo {
            id: id.into(),
            name: id.into(),
            version: "1".into(),
            builtin,
            essential: false,
        };
        let plugin = world
            .spawn((info, Granted::allowing(&[Capability::UiKeys])))
            .id();
        world
            .spawn((Command::new("go", "Go"), ContributedBy(plugin)));
        world.spawn((
            Key {
                chord: chord::parse(chord).unwrap(),
                command: "go".into(),
            },
            ContributedBy(plugin),
        ));
    }

    /// Presses `chord` for one frame and returns what ran.
    fn press(app: &mut App, chord: &str) -> Vec<String> {
        let input = chord::pressed(&chord::parse(chord).unwrap());
        *app.world_mut().resource_mut::<ButtonInput<KeyCode>>() =
            input;
        app.world_mut().resource_mut::<Ran>().0.clear();
        app.update();
        std::mem::take(&mut app.world_mut().resource_mut::<Ran>().0)
    }

    #[test]
    fn a_chord_reads_and_writes_the_same_text() {
        for text in
            ["Cmd+Shift+N", "Ctrl+K", "F5", "Alt+ArrowUp", "7"]
        {
            let chord = chord::parse(text).unwrap();
            assert_eq!(chord::parse(&chord.to_string()), Ok(chord));
        }
        assert_eq!(
            chord::parse("cmd + shift + n"),
            chord::parse("Cmd+Shift+N")
        );
        assert_eq!(
            chord::parse("Cmd+N").unwrap().mods,
            Mods::PRIMARY
        );
        for bad in
            ["", "Cmd+", "Escape", "Cmd+Enter", "Hyper+K", "Cmd+Nope"]
        {
            assert!(chord::parse(bad).is_err(), "{bad}");
        }
        assert_eq!(
            chord::parse("F5").unwrap(),
            Chord::key(KeyCode::F5)
        );
    }

    #[test]
    fn a_pressed_chord_runs_its_command_unless_it_is_contested() {
        let mut app = app();
        bind(&mut app, "one", false, "Cmd+K");
        assert_eq!(press(&mut app, "Cmd+K"), ["one/go"]);
        assert!(press(&mut app, "Cmd+J").is_empty());

        bind(&mut app, "two", false, "Cmd+K");
        assert!(press(&mut app, "Cmd+K").is_empty());

        bind(&mut app, "own", true, "Cmd+K");
        assert_eq!(press(&mut app, "Cmd+K"), ["own/go"]);
    }

    #[test]
    fn typing_in_a_field_is_not_taken_for_a_shortcut() {
        let mut app = app();
        bind(&mut app, "one", false, "F5");
        bind(&mut app, "two", false, "Shift+N");
        bind(&mut app, "three", false, "Cmd+K");
        let field =
            app.world_mut().spawn(EditableText::default()).id();
        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(field, FocusCause::Navigated);

        assert!(press(&mut app, "F5").is_empty());
        assert!(press(&mut app, "Shift+N").is_empty());
        assert_eq!(press(&mut app, "Cmd+K"), ["three/go"]);

        app.world_mut().resource_mut::<InputFocus>().clear();
        assert_eq!(press(&mut app, "F5"), ["one/go"]);
    }
}
