//! The command registry: the one action. A menu entry, a key and a
//! plugin all point at a [`Command`], and [`RunCommand`] runs it.

mod handler;
mod list;
mod notice;
mod run;
mod spec;
mod subject;

use bevy::app::{App, Update};
use bevy::ecs::schedule::IntoScheduleConfigs;

pub use self::handler::{Handler, HandlerWith, Opens};
pub use self::list::{Commands, Listed};
pub use self::notice::Notice;
pub use self::run::{Forward, RunCommand};
pub use self::spec::{Command, full};
pub use self::subject::{Subject, kind};

/// Runs the commands that are asked for and keeps [`Commands`] in
/// step with the ones there are.
pub fn plugin(app: &mut App) {
    app.init_resource::<Commands>()
        .add_observer(run::run)
        .add_observer(notice::notice)
        .add_systems(Update, list::keep.run_if(list::moved));
}

#[cfg(test)]
mod tests {
    use bevy::app::App;
    use bevy::ecs::observer::On;
    use bevy::ecs::resource::Resource;
    use bevy::ecs::system::ResMut;

    use super::*;
    use crate::plugin::{
        self as plugins, AppPluginExt, Disable, Entry, HamletPlugin,
        Manifest,
    };

    #[derive(Resource)]
    struct Waved;

    #[derive(Resource)]
    struct Greeted(String);

    #[derive(Resource, Default)]
    struct Seen(Vec<String>);

    #[derive(Resource, Default)]
    struct Passed(Vec<u8>);

    struct Probe;

    impl HamletPlugin for Probe {
        fn manifest() -> Manifest {
            Manifest::built_in("probe").named("Probe")
        }

        fn entry(entry: &mut Entry) {
            entry.command("wave", "Wave", |commands| {
                commands.insert_resource(Waved);
            });
            entry.command_with(
                "greet",
                "Greet",
                |commands, subject| {
                    commands.insert_resource(Greeted(
                        subject.name.clone(),
                    ));
                },
            );
            entry.forwarded("far", "Far", None);
        }
    }

    fn app() -> App {
        let mut app = App::new();
        app.init_resource::<Seen>()
            .init_resource::<Passed>()
            .add_plugins((plugins::plugin, plugin))
            .add_observer(
                |forward: On<Forward>,
                 mut seen: ResMut<Seen>,
                 mut passed: ResMut<Passed>| {
                    seen.0.push(forward.command.clone());
                    passed.0 = forward.args.clone();
                },
            )
            .add_observer(
                |notice: On<Notice>, mut seen: ResMut<Seen>| {
                    seen.0.push(notice.0.clone())
                },
            )
            .add_hamlet_plugin::<Probe>();
        app
    }

    fn run(app: &mut App, id: &str) {
        app.world_mut().trigger(RunCommand::of(id));
        app.world_mut().flush();
    }

    #[test]
    fn a_command_runs_its_handler_or_is_passed_to_its_plugin() {
        let mut app = app();

        run(&mut app, "probe/wave");
        assert!(app.world().contains_resource::<Waved>());
        run(&mut app, "probe/far");
        assert_eq!(app.world().resource::<Seen>().0, ["far"]);
        app.update();
        let listed = &app.world().resource::<Commands>().0;
        assert_eq!(
            listed.iter().map(|c| &c.id[..]).collect::<Vec<_>>(),
            ["probe/far", "probe/greet", "probe/wave"]
        );
    }

    #[test]
    fn a_command_is_given_the_subject_it_is_run_on() {
        let mut app = app();
        let subject = Subject::new(kind::AGENT, "7", "Juniper");

        run(&mut app, "probe/greet");
        assert!(!app.world().contains_resource::<Greeted>());
        assert_eq!(
            app.world().resource::<Seen>().0,
            ["`probe/greet` needs something to act on"]
        );

        let world = app.world_mut();
        world.trigger(RunCommand::on("probe/greet", subject.clone()));
        world.trigger(RunCommand::on("probe/far", subject.clone()));
        world.flush();
        assert_eq!(app.world().resource::<Greeted>().0, "Juniper");
        let passed = &app.world().resource::<Passed>().0;
        assert_eq!(Subject::decode(passed), Ok(subject));
    }

    #[test]
    fn a_command_of_a_plugin_that_is_off_is_reported_and_not_run() {
        let mut app = app();
        app.world_mut().trigger(Disable("probe".into()));
        app.world_mut().flush();

        run(&mut app, "probe/wave");

        assert!(!app.world().contains_resource::<Waved>());
        assert_eq!(
            app.world().resource::<Seen>().0,
            ["`probe/wave` did not run: Probe is switched off"]
        );
    }
}
