//! Temporary documents opened in the system editor and watched for
//! saves.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::{fs, io};

use bevy_app::{App, AppExit, Last, Update};
use bevy_ecs::event::Event;
use bevy_ecs::message::MessageReader;
use bevy_ecs::observer::On;
use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::{Commands, In, ResMut, SystemId};
use bevy_ecs::world::{FromWorld, World};
use bevy_log::warn;
use bevy_time::common_conditions::on_real_timer;

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Opener>()
        .init_resource::<Files>()
        .add_observer(start)
        .add_observer(end)
        .add_systems(
            Update,
            watch.run_if(on_real_timer(Duration::from_secs(1))),
        )
        .add_systems(Last, (final_watch, exit).chain());
}

/// Reads the last editor saves before a close is considered.
pub fn poll(world: &mut World) {
    if world.contains_resource::<Files>() {
        let _ = world.run_system_cached(watch);
        world.flush();
    }
}

/// Local recovery copies still watched by the editor.
pub fn copies(world: &World) -> Vec<PathBuf> {
    world
        .get_resource::<Files>()
        .map(|files| {
            files
                .watching
                .values()
                .map(|saved| saved.file.clone())
                .collect()
        })
        .unwrap_or_default()
}

/// A document copied to a temporary Markdown file for editing.
#[derive(Event, Clone, Debug)]
pub struct Edit {
    /// A nonempty file stem of ASCII letters, digits, hyphens or
    /// underscores.
    pub key: String,
    pub text: String,
}

/// A saved document whose text differs from the last seen text.
#[derive(Event, Clone, Debug, PartialEq, Eq)]
pub struct Edited {
    pub key: String,
    pub text: String,
}

/// The end of a document's edit, removing its temporary file.
#[derive(Event, Clone, Debug)]
pub struct EndEdit {
    pub key: String,
}

/// The system that opens a temporary file, returning whether it
/// opened.
#[derive(Resource)]
pub struct Opener(pub SystemId<In<PathBuf>, bool>);

impl FromWorld for Opener {
    fn from_world(world: &mut World) -> Self {
        Self(world.register_system(open))
    }
}

fn open(In(path): In<PathBuf>) -> bool {
    path.parent()
        .and_then(|folder| folder.canonicalize().ok())
        .is_some_and(|folder| crate::open::show(&path, &[folder]))
}

#[derive(Resource, Default)]
struct Files {
    folder: Option<PathBuf>,
    watching: HashMap<String, Saved>,
}

struct Saved {
    file: PathBuf,
    text: String,
    modified: SystemTime,
}

impl Files {
    fn write(&mut self, edit: &Edit) -> io::Result<PathBuf> {
        if edit.key.is_empty()
            || !edit.key.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || b"-_".contains(&byte)
            })
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "An editor key contains only ASCII letters, digits, \
                 hyphens and underscores.",
            ));
        }
        let folder = match &self.folder {
            Some(folder) => folder,
            None => self.folder.insert(folder()?),
        };
        let file = folder.join(format!("{}.md", edit.key));
        fs::write(&file, &edit.text)?;
        self.watching.insert(
            edit.key.clone(),
            Saved {
                file: file.clone(),
                text: edit.text.clone(),
                modified: fs::metadata(&file)?.modified()?,
            },
        );
        Ok(file)
    }

    fn end(&mut self, key: &str) {
        if let Some(saved) = self.watching.remove(key) {
            let _ = fs::remove_file(saved.file);
        }
        if self.watching.is_empty() {
            self.clear();
        }
    }

    fn clear(&mut self) {
        self.watching.clear();
        if let Some(folder) = self.folder.take() {
            let _ = fs::remove_dir_all(folder);
        }
    }
}

impl Drop for Files {
    fn drop(&mut self) {
        self.clear();
    }
}

fn folder() -> io::Result<PathBuf> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    for attempt in 0.. {
        let folder = std::env::temp_dir().join(format!(
            "hamlet-editor-{}-{stamp}-{attempt}",
            std::process::id()
        ));
        match builder.create(&folder) {
            Ok(()) => return Ok(folder),
            Err(error)
                if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    unreachable!()
}

fn start(edit: On<Edit>, mut commands: Commands) {
    let edit = edit.event().clone();
    commands.queue(move |world: &mut World| {
        let file = match world.resource_mut::<Files>().write(&edit) {
            Ok(file) => file,
            Err(error) => {
                warn!("Could not edit {}: {error}", edit.key);
                return;
            }
        };
        let opener = world.resource::<Opener>().0;
        if !world.run_system_with(opener, file).unwrap_or(false) {
            world.resource_mut::<Files>().end(&edit.key);
            warn!("Could not open the editor for {}", edit.key);
        }
    });
}

fn end(edit: On<EndEdit>, mut files: ResMut<Files>) {
    files.end(&edit.key);
}

fn watch(mut files: ResMut<Files>, mut commands: Commands) {
    for (key, saved) in &mut files.watching {
        let Ok(modified) = fs::metadata(&saved.file)
            .and_then(|file| file.modified())
        else {
            continue;
        };
        let Ok(text) = fs::read_to_string(&saved.file) else {
            continue;
        };
        if modified == saved.modified && text == saved.text {
            continue;
        }
        saved.modified = modified;
        if text != saved.text {
            saved.text.clone_from(&text);
            commands.trigger(Edited {
                key: key.clone(),
                text,
            });
        }
    }
}

fn final_watch(
    mut exits: MessageReader<AppExit>,
    mut commands: Commands,
) {
    if exits.read().next().is_some() {
        commands.run_system_cached(watch);
    }
}

fn exit(mut exits: MessageReader<AppExit>, mut files: ResMut<Files>) {
    if exits.read().next().is_some()
        && let Some(folder) = files.folder.take()
    {
        // A queued edit is not proof that the server kept it.
        warn!(
            "Editor copies kept in {}. Unconfirmed saves may need \
             to be applied again.",
            folder.display()
        );
        files.watching.clear();
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::Duration;

    use bevy_app::AppExit;
    use bevy_ecs::system::ResMut;
    use bevy_time::{TimePlugin, TimeUpdateStrategy};

    use super::*;
    use crate::DesktopPlugin;

    #[derive(Resource, Default)]
    struct Opened(Vec<PathBuf>);

    #[derive(Resource, Default)]
    struct Saves(Vec<Edited>);

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((
            TimePlugin,
            DesktopPlugin { window: false },
        ))
        .insert_resource(TimeUpdateStrategy::ManualDuration(
            Duration::ZERO,
        ))
        .init_resource::<Opened>()
        .init_resource::<Saves>()
        .add_observer(
            |saved: On<Edited>, mut saves: ResMut<Saves>| {
                saves.0.push(saved.event().clone());
            },
        );
        let open = app.world_mut().register_system(
            |In(path): In<PathBuf>, mut opened: ResMut<Opened>| {
                opened.0.push(path);
                true
            },
        );
        app.insert_resource(Opener(open));
        app
    }

    fn tick(app: &mut App, elapsed: Duration) {
        app.insert_resource(TimeUpdateStrategy::ManualDuration(
            elapsed,
        ));
        app.update();
    }

    fn save(path: &Path, text: &str) {
        let modified =
            fs::metadata(path).unwrap().modified().unwrap();
        fs::write(path, text).unwrap();
        fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(modified + Duration::from_secs(1))
            .unwrap();
    }

    #[test]
    fn closing_before_the_poll_reports_the_save_and_keeps_its_copy() {
        let mut app = app();
        app.world_mut().trigger(Edit {
            key: "charter".into(),
            text: "old".into(),
        });
        app.update();
        let file = app.world().resource::<Opened>().0[0].clone();
        save(&file, "last save");
        app.world_mut().write_message(AppExit::Success);
        app.update();
        assert_eq!(
            app.world().resource::<Saves>().0,
            [Edited {
                key: "charter".into(),
                text: "last save".into()
            }]
        );
        drop(app);
        assert_eq!(fs::read_to_string(&file).unwrap(), "last save");
        fs::remove_dir_all(file.parent().unwrap()).unwrap();
    }

    #[test]
    fn closing_keeps_an_already_polled_save_that_may_still_be_in_flight()
     {
        let mut app = app();
        app.world_mut().trigger(Edit {
            key: "charter".into(),
            text: "old".into(),
        });
        app.update();
        let file = app.world().resource::<Opened>().0[0].clone();
        save(&file, "offline save");
        tick(&mut app, Duration::from_secs(1));
        app.world_mut().write_message(AppExit::Success);
        app.update();
        drop(app);
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            "offline save"
        );
        fs::remove_dir_all(file.parent().unwrap()).unwrap();
    }

    #[test]
    fn an_edit_opens_a_temporary_markdown_copy() {
        let mut app = app();
        app.world_mut().trigger(Edit {
            key: "charter".into(),
            text: "abc".into(),
        });
        app.update();

        let opened = &app.world().resource::<Opened>().0;
        assert_eq!(opened.len(), 1);
        let file = &opened[0];
        assert_eq!(file.file_name().unwrap(), "charter.md");
        assert!(file.starts_with(std::env::temp_dir()));
        assert_ne!(file.parent().unwrap(), std::env::temp_dir());
        assert_eq!(fs::read_to_string(file).unwrap(), "abc");
    }

    #[test]
    fn a_changed_save_is_reported_once_at_the_next_second() {
        let mut app = app();
        app.world_mut().trigger(Edit {
            key: "charter".into(),
            text: "abc".into(),
        });
        app.update();
        let file = app.world().resource::<Opened>().0[0].clone();
        save(&file, "new words");

        tick(&mut app, Duration::from_millis(999));
        assert!(app.world().resource::<Saves>().0.is_empty());
        tick(&mut app, Duration::from_millis(1));
        assert_eq!(
            app.world().resource::<Saves>().0,
            [Edited {
                key: "charter".into(),
                text: "new words".into(),
            }]
        );
        tick(&mut app, Duration::from_secs(1));
        save(&file, "new words");
        tick(&mut app, Duration::from_secs(1));
        assert_eq!(app.world().resource::<Saves>().0.len(), 1);
    }

    #[test]
    fn reopening_replaces_the_copy_and_watches_until_app_exit() {
        let mut app = app();
        app.world_mut().trigger(Edit {
            key: "founder-character".into(),
            text: "old character".into(),
        });
        app.update();
        let file = app.world().resource::<Opened>().0[0].clone();
        app.world_mut().trigger(Edit {
            key: "founder-character".into(),
            text: "from the server".into(),
        });
        app.update();
        assert_eq!(
            app.world().resource::<Opened>().0,
            [file.clone(), file.clone()]
        );
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            "from the server"
        );
        tick(&mut app, Duration::from_secs(1));
        assert!(app.world().resource::<Saves>().0.is_empty());
        save(&file, "saved again");
        tick(&mut app, Duration::from_secs(1));
        assert_eq!(
            app.world().resource::<Saves>().0,
            [Edited {
                key: "founder-character".into(),
                text: "saved again".into(),
            }]
        );

        app.world_mut().write_message(AppExit::Success);
        app.update();
        assert_eq!(fs::read_to_string(&file).unwrap(), "saved again");
        fs::remove_dir_all(file.parent().unwrap()).unwrap();
    }

    #[test]
    fn an_editor_that_cannot_open_leaves_no_temporary_file() {
        let mut app = app();
        let failed = app.world_mut().register_system(
            |In(path): In<PathBuf>, mut opened: ResMut<Opened>| {
                opened.0.push(path);
                false
            },
        );
        app.insert_resource(Opener(failed));
        app.world_mut().trigger(Edit {
            key: "charter".into(),
            text: "abc".into(),
        });
        app.update();
        let file = &app.world().resource::<Opened>().0[0];
        assert!(!file.exists());
        assert!(!file.parent().unwrap().exists());
        tick(&mut app, Duration::from_secs(1));
        assert!(app.world().resource::<Saves>().0.is_empty());
    }

    #[test]
    fn a_key_cannot_write_outside_the_editors_folder() {
        let scratch = hamlet_testing::Scratch::new("editor-key");
        let outside = scratch.join("outside");
        let mut app = app();
        app.world_mut().trigger(Edit {
            key: outside.to_str().unwrap().into(),
            text: "abc".into(),
        });
        app.update();
        assert!(!outside.with_extension("md").exists());
        assert!(app.world().resource::<Opened>().0.is_empty());
    }

    #[test]
    fn a_replacement_save_is_read_even_when_its_timestamp_is_preserved()
     {
        let mut app = app();
        app.world_mut().trigger(Edit {
            key: "charter".into(),
            text: "abc".into(),
        });
        app.update();
        let file = app.world().resource::<Opened>().0[0].clone();
        let modified =
            fs::metadata(&file).unwrap().modified().unwrap();
        let replacement = file.with_extension("saving");
        fs::write(&replacement, "def").unwrap();
        fs::File::options()
            .write(true)
            .open(&replacement)
            .unwrap()
            .set_modified(modified)
            .unwrap();
        fs::rename(replacement, &file).unwrap();
        tick(&mut app, Duration::from_secs(1));
        assert_eq!(
            app.world().resource::<Saves>().0,
            [Edited {
                key: "charter".into(),
                text: "def".into(),
            }]
        );
    }

    #[test]
    fn ending_one_edit_keeps_the_other_watched_until_the_app_is_dropped()
     {
        let mut app = app();
        for key in ["charter", "founder-character"] {
            app.world_mut().trigger(Edit {
                key: key.into(),
                text: "abc".into(),
            });
        }
        app.update();
        let opened = app.world().resource::<Opened>().0.clone();
        app.world_mut().trigger(EndEdit {
            key: "charter".into(),
        });
        app.update();
        assert!(!opened[0].exists());
        save(&opened[1], "still watching");
        tick(&mut app, Duration::from_secs(1));
        assert_eq!(
            app.world().resource::<Saves>().0,
            [Edited {
                key: "founder-character".into(),
                text: "still watching".into(),
            }]
        );
        drop(app);
        assert!(!opened[1].exists());
        assert!(!opened[1].parent().unwrap().exists());
    }
}
