//! The keys in force: each command's own, unless the person bound it
//! to others. The person's are kept in `shortcuts.toml` in the home,
//! written whole as they change. A file that cannot be read is never
//! written over.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use bevy::app::{App, Last, PreUpdate, Startup};
use bevy::ecs::change_detection::DetectChanges;
use bevy::ecs::lifecycle::RemovedComponents;
use bevy::ecs::message::MessageWriter;
use bevy::ecs::query::{Changed, Or};
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Query, Res, ResMut};
use bevy_fynix::shortcut::Chord;
use hamlet_engine_api::CommandId;
use hamlet_engine_core::{Home, LogLine, write_whole};
use serde::{Deserialize, Serialize};

use crate::command::{ChordExt, Command, Shortcut, parse_chord};

/// The file's name in the home.
pub const FILE: &str = "shortcuts.toml";

pub(crate) fn plugin(app: &mut App) {
    let home = app.world().resource::<Home>().clone();
    let (file, keymap) = KeymapFile::open(&home);
    app.insert_resource(file)
        .insert_resource(keymap)
        .init_resource::<Bindings>()
        .add_systems(Startup, tell_held)
        .add_systems(PreUpdate, bind)
        .add_systems(Last, save);
}

/// The person's own keys: for each command they bound, the chords it
/// answers to in place of its own, none for one they cleared.
#[derive(Resource, Clone, PartialEq, Debug, Default)]
pub struct Keymap {
    rebound: BTreeMap<CommandId, Vec<Chord>>,
}

impl Keymap {
    /// The person's chords for `command`, if they bound it.
    pub fn rebound(&self, command: &CommandId) -> Option<&[Chord]> {
        self.rebound.get(command).map(Vec::as_slice)
    }

    /// Binds `command` to `chords` alone. Its own chords are no
    /// binding of the person's.
    pub fn bind(
        &mut self,
        bindings: &Bindings,
        command: &CommandId,
        chords: Vec<Chord>,
    ) {
        let own =
            bindings.of(command).map_or(&[][..], |bound| &bound.own);
        if chords == own {
            self.rebound.remove(command);
        } else {
            self.rebound.insert(command.clone(), chords);
        }
    }

    /// Binds `command` to `chord`, taking it from `other`, which has
    /// it. A swap gives `other` the chord `command` had.
    pub fn take(
        &mut self,
        bindings: &Bindings,
        command: &CommandId,
        chord: Chord,
        other: &CommandId,
        swap: bool,
    ) {
        let mut left = bindings
            .chords(other)
            .iter()
            .copied()
            .filter(|&had| had != chord)
            .collect::<Vec<_>>();
        if swap
            && let Some(&had) = bindings.chords(command).first()
            && had != chord
            && !left.contains(&had)
        {
            left.push(had);
        }
        self.bind(bindings, other, left);
        self.bind(bindings, command, vec![chord]);
    }

    /// Puts `command` back on its own chords.
    pub fn reset(&mut self, command: &CommandId) {
        self.rebound.remove(command);
    }

    /// Puts every command back on its own chords.
    pub fn reset_all(&mut self) {
        self.rebound.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.rebound.is_empty()
    }
}

/// [`Keymap`] as the file holds it, each chord as [`parse_chord`]
/// reads it.
#[derive(Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct Saved {
    rebound: BTreeMap<CommandId, Vec<String>>,
}

/// Where the [`Keymap`] is kept.
#[derive(Resource, Debug)]
pub struct KeymapFile {
    path: PathBuf,
    /// Why the file could not be read. It is left as it is, and
    /// nothing is written.
    held: Option<String>,
}

impl KeymapFile {
    /// The file of `home`, and the keymap it holds. A missing file
    /// holds none.
    pub fn open(home: &Home) -> (Self, Keymap) {
        let path = home.path(FILE);
        let read = match fs::read_to_string(&path) {
            Ok(text) => read(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Ok(Keymap::default())
            }
            Err(e) => Err(e.to_string()),
        };
        let (keymap, held) = match read {
            Ok(keymap) => (keymap, None),
            Err(why) => (Keymap::default(), Some(why)),
        };
        (Self { path, held }, keymap)
    }

    /// Writes `keymap`, unless the file could not be read. The error
    /// says why it cannot be written.
    pub fn write(&self, keymap: &Keymap) -> Result<(), String> {
        if self.held.is_some() {
            return Ok(());
        }
        let saved = Saved {
            rebound: keymap
                .rebound
                .iter()
                .map(|(command, chords)| {
                    let keys = chords
                        .iter()
                        .filter_map(ChordExt::keys)
                        .collect();
                    (command.clone(), keys)
                })
                .collect(),
        };
        let unwritten =
            |why: String| format!("{FILE} cannot be written: {why}");
        let text = toml::to_string(&saved)
            .map_err(|e| unwritten(e.to_string()))?;
        write_whole(&self.path, text)
            .map_err(|e| unwritten(e.to_string()))
    }
}

fn read(text: &str) -> Result<Keymap, String> {
    let saved = toml::from_str::<Saved>(text)
        .map_err(|e| e.message().to_owned())?;
    let mut rebound = BTreeMap::new();
    for (command, keys) in saved.rebound {
        let chords = keys
            .iter()
            .map(|keys| {
                parse_chord(keys)
                    .ok_or_else(|| format!("{keys:?} are not keys"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        rebound.insert(command, chords);
    }
    Ok(Keymap { rebound })
}

/// The keys in force for each command there is, the one that answers
/// a chord first first.
#[derive(Resource, Clone, PartialEq, Debug, Default)]
pub struct Bindings(Vec<Bound>);

/// A command's keys.
#[derive(Clone, PartialEq, Debug)]
pub struct Bound {
    pub command: CommandId,
    /// What a menu calls it.
    pub label: String,
    /// The chords it came with.
    pub own: Vec<Chord>,
    /// The chords it answers to.
    pub chords: Vec<Chord>,
}

impl Bindings {
    /// The bindings of `commands`, with their ids and labels, given
    /// the shortcuts they came with and the person's `keymap`. The
    /// commands with shortcuts answer first, in the order of their
    /// shortcuts.
    pub fn build<'a>(
        commands: impl IntoIterator<Item = (&'a CommandId, &'a str)>,
        shortcuts: impl IntoIterator<Item = &'a Shortcut>,
        keymap: &Keymap,
    ) -> Self {
        let commands = commands.into_iter().collect::<Vec<_>>();
        let mut all = Vec::<Bound>::new();
        let add = |all: &mut Vec<Bound>, id: &CommandId| {
            if let Some(at) =
                all.iter().position(|b| b.command == *id)
            {
                return Some(at);
            }
            let (_, label) =
                commands.iter().find(|(c, _)| *c == id)?;
            all.push(Bound {
                command: id.clone(),
                label: (*label).to_owned(),
                own: Vec::new(),
                chords: Vec::new(),
            });
            Some(all.len() - 1)
        };
        for shortcut in shortcuts {
            if let Some(at) = add(&mut all, &shortcut.command)
                && !all[at].own.contains(&shortcut.chord)
            {
                all[at].own.push(shortcut.chord);
            }
        }
        for (id, _) in &commands {
            add(&mut all, id);
        }
        for bound in &mut all {
            bound.chords = match keymap.rebound(&bound.command) {
                Some(chords) => chords.to_vec(),
                None => bound.own.clone(),
            };
        }
        Self(all)
    }

    pub fn all(&self) -> &[Bound] {
        &self.0
    }

    pub fn of(&self, command: &CommandId) -> Option<&Bound> {
        self.0.iter().find(|bound| bound.command == *command)
    }

    /// The chords `command` answers to.
    pub fn chords(&self, command: &CommandId) -> &[Chord] {
        self.of(command).map_or(&[], |bound| &bound.chords)
    }

    /// The first chord of `command`, as the platform writes it.
    pub fn label(&self, command: &CommandId) -> Option<String> {
        self.chords(command).first().map(ToString::to_string)
    }

    /// The commands bound to `chord`, the one that answers first
    /// first.
    pub fn holders(
        &self,
        chord: Chord,
    ) -> impl Iterator<Item = &CommandId> + '_ {
        self.0
            .iter()
            .filter(move |bound| bound.chords.contains(&chord))
            .map(|bound| &bound.command)
    }
}

/// Builds the bindings again when a command, a shortcut or the
/// person's keymap comes, changes or goes.
fn bind(
    changed: Query<(), Or<(Changed<Command>, Changed<Shortcut>)>>,
    mut gone: RemovedComponents<Command>,
    mut gone_keys: RemovedComponents<Shortcut>,
    commands: Query<&Command>,
    shortcuts: Query<&Shortcut>,
    keymap: Res<Keymap>,
    mut bindings: ResMut<Bindings>,
) {
    let went = gone.read().count() + gone_keys.read().count() > 0;
    if changed.is_empty() && !went && !keymap.is_changed() {
        return;
    }
    let now = Bindings::build(
        commands
            .iter()
            .map(|command| (&command.id, command.label.as_str())),
        shortcuts,
        &keymap,
    );
    if *bindings != now {
        *bindings = now;
    }
}

/// Writes the keymap as it changes.
fn save(
    keymap: Res<Keymap>,
    file: Res<KeymapFile>,
    mut log: MessageWriter<LogLine>,
) {
    if !keymap.is_changed() || keymap.is_added() {
        return;
    }
    if let Err(why) = file.write(&keymap) {
        log.write(LogLine::error("shortcuts", why));
    }
}

fn tell_held(file: Res<KeymapFile>, mut log: MessageWriter<LogLine>) {
    if let Some(why) = &file.held {
        log.write(LogLine::error(
            "shortcuts",
            format!(
                "{FILE} cannot be read ({why}). It is left as it \
                 is, and the shortcuts are kept only until the \
                 engine closes."
            ),
        ));
    }
}

#[cfg(test)]
mod tests {
    use bevy::input::keyboard::KeyCode;
    use bevy_fynix::shortcut::Mods;

    use super::*;
    use crate::command::engine_id;

    #[test]
    fn a_rebinding_outlives_a_restart_and_a_shared_chord_is_found() {
        let root = std::env::temp_dir().join(format!(
            "hamlet_engine_test_keymap_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let home = Home::at(&root);
        let (quit, new) = (engine_id("quit"), engine_id("new"));
        let q = Chord {
            key: KeyCode::KeyQ,
            mods: Mods::PRIMARY,
        };
        let n = Chord {
            key: KeyCode::KeyN,
            mods: Mods::PRIMARY.with(Mods::SHIFT),
        };
        let own = [Shortcut {
            chord: q,
            command: quit.clone(),
        }];
        let commands = [(&quit, "Quit"), (&new, "New")];
        let bindings =
            |keymap: &Keymap| Bindings::build(commands, &own, keymap);

        let (file, mut keymap) = KeymapFile::open(&home);
        keymap.bind(&bindings(&keymap), &new, vec![n]);
        file.write(&keymap).unwrap();
        let (_, mut read) = KeymapFile::open(&home);
        assert_eq!(read, keymap);
        assert_eq!(bindings(&read).chords(&new), [n]);

        let held =
            bindings(&read).holders(q).cloned().collect::<Vec<_>>();
        assert_eq!(
            held,
            std::slice::from_ref(&quit),
            "the chord is taken"
        );
        read.take(&bindings(&read), &new, q, &quit, true);
        assert_eq!(bindings(&read).chords(&quit), [n]);
        assert_eq!(bindings(&read).chords(&new), [q]);

        fs::write(root.join(FILE), "rebound = 1").unwrap();
        let (unread, _) = KeymapFile::open(&home);
        unread.write(&read).unwrap();
        let text = fs::read_to_string(root.join(FILE)).unwrap();
        assert_eq!(text, "rebound = 1", "an unread file is kept");
        let _ = fs::remove_dir_all(root);
    }
}
