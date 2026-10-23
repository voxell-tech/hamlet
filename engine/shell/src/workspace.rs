//! Workspaces: named sets of panels and their layout in the dock,
//! kept per person in the home. One is shown at a time, and what is
//! changed in the dock by hand is kept in it.

mod arrange;
mod file;
pub(crate) mod icon;

use bevy::app::{App, AppExit, Last, Startup, Update};
use bevy::ecs::change_detection::DetectChanges;
use bevy::ecs::component::Component;
use bevy::ecs::hierarchy::ChildOf;
use bevy::ecs::lifecycle::Add;
use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::common_conditions::{
    on_message, resource_changed,
};
use bevy::ecs::schedule::{IntoScheduleConfigs, SystemCondition};
use bevy::ecs::system::{Commands, Query, Res, ResMut};
use bevy_fynix::dock::{DockRegistry, DockTree};
use hamlet_engine_api::id::ENGINE as ENGINE_OWNER;
use hamlet_engine_api::{Axis, CommandId, Layout, PanelId};
use hamlet_engine_core::TuningPlugin;
use hamlet_engine_core::tuning::{ENGINE, Tuning, TuningFile};
use serde::{Deserialize, Serialize};

use crate::command::{RunCommand, ShellCommandsExt};
use crate::dock::PLUGIN_LIST;
use crate::icons::Glyph;
pub use crate::workspace::arrange::DockTreeExt;
use crate::{EngineTheme, console};

/// The name of the first workspace of a new home.
const FIRST: &str = "Main";

/// Panels the first workspace holds besides the plugin list and the
/// console.
const REALM: &str = "hamlet.realm/realm";
const PETITIONS: &str = "engine/petitions";
const CHAT: &str = "hamlet.chat/chat";
const INSPECTOR: &str = "hamlet.inspector/inspector";

/// The most characters a workspace's name may have.
const LONGEST_NAME: usize = 40;

pub(crate) fn plugin(app: &mut App) {
    app.add_plugins(TuningPlugin::<WorkspaceTuning>::default())
        .init_resource::<Workspaces>()
        .init_resource::<Shown>()
        .init_resource::<file::Unsaved>()
        .add_observer(suggest)
        .add_systems(Startup, file::restore)
        .add_systems(
            Update,
            (
                show.run_if(resource_changed::<Workspaces>.or_eager(
                    resource_changed::<DockRegistry<EngineTheme>>,
                )),
                capture.run_if(resource_changed::<DockTree>),
                file::note.run_if(resource_changed::<Workspaces>),
                file::save.run_if(|unsaved: Res<file::Unsaved>| {
                    unsaved.pending()
                }),
            )
                .chain(),
        )
        .add_systems(
            Last,
            file::save_now.run_if(on_message::<AppExit>),
        );
}

/// When the workspaces are saved.
#[derive(Resource, Clone, PartialEq, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceTuning {
    /// Seconds the workspaces stay unchanged before they are saved.
    pub save_after: f32,
    /// The most seconds between the two presses of a double-click on
    /// a workspace's tab.
    pub double_click: f32,
    /// How far, in logical pixels, a tab is dragged before it moves.
    pub drag_start: f32,
}

impl Tuning for WorkspaceTuning {
    const FILE: TuningFile = ENGINE;
    const SECTION: Option<&'static str> = Some("workspaces");
}

/// A named set of panels and their layout.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Workspace {
    pub name: String,
    /// The icon before its name. A file without one is given one as
    /// it is read.
    #[serde(default, with = "icon")]
    pub icon: Option<Glyph>,
    pub layout: Layout,
}

/// A workspace a plugin suggests. The person takes a copy of it with
/// the command `take`, offered by the switcher's "+", and the copy is
/// theirs.
#[derive(Component, Clone, Debug)]
#[component(immutable)]
pub struct SuggestedWorkspace {
    pub take: CommandId,
    pub name: String,
    pub layout: Layout,
}

/// The person's workspaces, and the one shown. There is always at
/// least one.
#[derive(
    Resource, Clone, PartialEq, Debug, Serialize, Deserialize,
)]
pub struct Workspaces {
    shown: usize,
    list: Vec<Workspace>,
}

impl Default for Workspaces {
    /// One workspace: the realm in the middle, the petitions and the
    /// plugin list on its left, and the chat and the Inspector over
    /// the console on its right. The panel of a plugin that is not on
    /// is left out of the dock until it is.
    fn default() -> Self {
        let tabs =
            |panels: Vec<PanelId>| Layout::Tabs { panels, shown: 0 };
        let bundled =
            |id: &str| PanelId::try_from(id).expect("a valid id");
        let split = |axis, share, first, second| Layout::Split {
            axis,
            share,
            first: Box::new(first),
            second: Box::new(second),
        };
        let left = tabs(vec![
            bundled(PETITIONS),
            PanelId::new(ENGINE_OWNER, PLUGIN_LIST)
                .expect("a valid id"),
        ]);
        let right = split(
            Axis::Vertical,
            0.65,
            tabs(vec![bundled(CHAT), bundled(INSPECTOR)]),
            tabs(vec![console::id()]),
        );
        Self {
            shown: 0,
            list: vec![Workspace {
                name: FIRST.into(),
                icon: Some(Glyph::House),
                layout: split(
                    Axis::Horizontal,
                    0.2,
                    left,
                    split(
                        Axis::Horizontal,
                        0.7,
                        tabs(vec![bundled(REALM)]),
                        right,
                    ),
                ),
            }],
        }
    }
}

impl Workspaces {
    pub fn new(list: Vec<Workspace>, shown: usize) -> Self {
        Self { shown, list }.cleaned()
    }

    pub fn list(&self) -> &[Workspace] {
        &self.list
    }

    /// The index of the workspace shown.
    pub fn shown(&self) -> usize {
        self.shown
    }

    /// Shows the workspace at `at`, if there is one.
    pub fn show(&mut self, at: usize) {
        if at < self.list.len() && at != self.shown {
            self.shown = at;
        }
    }

    /// Shows the next workspace, or the previous with `back`, going
    /// round.
    pub fn cycle(&mut self, back: bool) {
        let count = self.list.len();
        let step = if back { count - 1 } else { 1 };
        self.show((self.shown + step) % count);
    }

    /// Adds an empty workspace with a name of its own, shows it and
    /// gives its index.
    pub fn add(&mut self) -> usize {
        let name = (self.list.len() + 1..)
            .map(|n| format!("Workspace {n}"))
            .find(|name| !self.named(name, None))
            .unwrap_or_default();
        self.insert(self.list.len(), name, Layout::empty())
    }

    /// Adds a copy of `layout` named after `name`, numbered when the
    /// name is taken, and shows it.
    pub fn take(&mut self, name: &str, layout: &Layout) {
        let name = self.free_name(name);
        self.insert(self.list.len(), name, layout.clone());
    }

    /// Adds a copy of the workspace at `at` after it, numbered, and
    /// shows it.
    pub fn duplicate(&mut self, at: usize) {
        let Some(workspace) = self.list.get(at).cloned() else {
            return;
        };
        let name = self.free_name(&workspace.name);
        self.list.insert(at + 1, Workspace { name, ..workspace });
        self.shown = at + 1;
    }

    /// Moves the workspace at `from` to `to`, the shown one still
    /// shown.
    pub fn move_to(&mut self, from: usize, to: usize) {
        let count = self.list.len();
        if from >= count || to >= count || from == to {
            return;
        }
        let shown = self.shown;
        let workspace = self.list.remove(from);
        self.list.insert(to, workspace);
        self.shown = match shown {
            _ if shown == from => to,
            _ if from < shown && shown <= to => shown - 1,
            _ if to <= shown && shown < from => shown + 1,
            _ => shown,
        };
    }

    /// Gives the workspace at `at` the icon `icon`.
    pub fn set_icon(&mut self, at: usize, icon: Glyph) {
        if let Some(workspace) = self.list.get_mut(at)
            && workspace.icon != Some(icon)
        {
            workspace.icon = Some(icon);
        }
    }

    /// Names the workspace at `at` `name`, trimmed. Whether it was: a
    /// name that is empty, too long or another's is refused. An icon
    /// that came with the old name follows the new one.
    pub fn rename(&mut self, at: usize, name: &str) -> bool {
        let name = name.trim();
        let fits = !name.is_empty()
            && name.chars().count() <= LONGEST_NAME
            && !self.named(name, Some(at));
        let taken = self.icons_besides(at);
        match self.list.get_mut(at) {
            Some(workspace) if fits => {
                if workspace.name != name {
                    let came = icon::suggest(&workspace.name, &taken);
                    if workspace.icon == Some(came) {
                        workspace.icon =
                            Some(icon::suggest(name, &taken));
                    }
                    workspace.name = name.to_owned();
                }
                true
            }
            _ => false,
        }
    }

    /// Deletes the workspace at `at`, unless it is the last one.
    pub fn delete(&mut self, at: usize) {
        if self.list.len() < 2 || at >= self.list.len() {
            return;
        }
        self.list.remove(at);
        if self.shown > at || self.shown == self.list.len() {
            self.shown -= 1;
        }
    }

    /// Puts a workspace named `name` holding `layout` at `at`, with
    /// the icon its name calls for, shows it and gives `at`.
    fn insert(
        &mut self,
        at: usize,
        name: String,
        layout: Layout,
    ) -> usize {
        let taken = self.icons_besides(usize::MAX);
        let icon = Some(icon::suggest(&name, &taken));
        self.list.insert(at, Workspace { name, icon, layout });
        self.shown = at;
        at
    }

    /// `name`, trimmed and numbered when another workspace has it.
    fn free_name(&self, name: &str) -> String {
        let name = name
            .trim()
            .chars()
            .take(LONGEST_NAME - 3)
            .collect::<String>();
        (1..)
            .map(|n| match n {
                1 => name.clone(),
                n => format!("{name} {n}"),
            })
            .find(|name| !name.is_empty() && !self.named(name, None))
            .unwrap_or_default()
    }

    /// The icons of the workspaces but the one at `besides`.
    fn icons_besides(&self, besides: usize) -> Vec<Glyph> {
        self.list
            .iter()
            .enumerate()
            .filter(|(at, _)| *at != besides)
            .filter_map(|(_, workspace)| workspace.icon)
            .collect()
    }

    /// Whether a workspace other than the one at `besides` is called
    /// `name`.
    fn named(&self, name: &str, besides: Option<usize>) -> bool {
        self.list.iter().enumerate().any(|(at, workspace)| {
            Some(at) != besides
                && workspace.name.eq_ignore_ascii_case(name)
        })
    }

    /// These, with at least one workspace, one of them shown, and
    /// every one named and with an icon.
    fn cleaned(mut self) -> Self {
        if self.list.is_empty() {
            return Self::default();
        }
        self.shown = self.shown.min(self.list.len() - 1);
        for at in 0..self.list.len() {
            let taken = self.icons_besides(at);
            let workspace = &mut self.list[at];
            if workspace.name.trim().is_empty() {
                workspace.name = format!("Workspace {}", at + 1);
            }
            if workspace.icon.is_none() {
                workspace.icon =
                    Some(icon::suggest(&workspace.name, &taken));
            }
        }
        self
    }
}

/// What the dock was last given, to tell a change made by hand from
/// one the engine made.
#[derive(Resource, Default)]
struct Shown {
    workspace: Option<usize>,
    /// The workspace's layout as it was laid out.
    saved: Option<Layout>,
    /// The layout as the dock had it then, without the panels it
    /// does not know.
    drawn: Option<Layout>,
}

/// Adds the command that takes a suggested workspace, for as long as
/// it is there.
fn suggest(
    added: On<Add, SuggestedWorkspace>,
    suggestions: Query<&SuggestedWorkspace>,
    mut commands: Commands,
) {
    let entity = added.entity;
    let Ok(suggestion) = suggestions.get(entity) else {
        return;
    };
    let (name, layout) =
        (suggestion.name.clone(), suggestion.layout.clone());
    commands
        .spawn_command(
            suggestion.take.clone(),
            &format!("Take workspace {name}"),
            None,
            &[],
        )
        .insert(ChildOf(entity))
        .observe(
            move |_: On<RunCommand>,
                  mut workspaces: ResMut<Workspaces>| {
                workspaces.take(&name, &layout);
            },
        );
}

/// Lays the shown workspace out in the dock when another is shown,
/// and again when panels come or go.
fn show(
    workspaces: Res<Workspaces>,
    registry: Res<DockRegistry<EngineTheme>>,
    mut tree: ResMut<DockTree>,
    mut shown: ResMut<Shown>,
) {
    let at = workspaces.shown;
    let Some(workspace) = workspaces.list.get(at) else {
        return;
    };
    let same = shown.workspace == Some(at)
        && shown.saved.as_ref() == Some(&workspace.layout);
    if same && !registry.is_changed() {
        return;
    }
    tree.lay_out(&workspace.layout, |id| registry.get(id).is_some());
    *shown = Shown {
        workspace: Some(at),
        saved: Some(workspace.layout.clone()),
        drawn: Some(tree.layout()),
    };
}

/// Keeps in the shown workspace what was changed in the dock by hand.
fn capture(
    tree: Res<DockTree>,
    mut shown: ResMut<Shown>,
    mut workspaces: ResMut<Workspaces>,
) {
    let now = tree.layout();
    if shown.drawn.as_ref() == Some(&now) {
        return;
    }
    let Some(at) = shown.workspace else {
        return;
    };
    shown.drawn = Some(now.clone());
    shown.saved = Some(now.clone());
    if let Some(workspace) = workspaces.list.get_mut(at) {
        workspace.layout = now;
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::{env, fs};

    use hamlet_engine_api::PanelId;
    use hamlet_engine_core::Home;

    use super::file::{FILE, Restored};
    use super::*;

    /// A fresh home of its own for the test `name`.
    fn home(name: &str) -> Home {
        let root = env::temp_dir().join(format!(
            "hamlet_engine_test_{name}_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        Home::at(root)
    }

    fn panel(id: &str) -> PanelId {
        PanelId::try_from(id).unwrap()
    }

    fn workspaces() -> Workspaces {
        let tabs = |panels: &[&str], shown| Layout::Tabs {
            panels: panels.iter().map(|id| panel(id)).collect(),
            shown,
        };
        Workspaces::new(
            vec![
                Workspace {
                    name: "Writing".into(),
                    icon: Some(Glyph::Moon),
                    layout: Layout::Split {
                        axis: Axis::Horizontal,
                        share: 0.25,
                        first: Box::new(tabs(&["engine/plugins"], 0)),
                        second: Box::new(Layout::Split {
                            axis: Axis::Vertical,
                            share: 0.6,
                            first: Box::new(tabs(
                                &[
                                    "hamlet.notes/page",
                                    "hamlet.notes/list",
                                ],
                                1,
                            )),
                            second: Box::new(tabs(
                                &["engine/console"],
                                0,
                            )),
                        }),
                    },
                },
                Workspace {
                    name: "Quiet".into(),
                    icon: Some(Glyph::Bell),
                    layout: Layout::empty(),
                },
            ],
            1,
        )
    }

    #[test]
    fn workspaces_come_back_from_the_file_and_the_dock_unchanged() {
        let home = home("round_trip");
        let saved = workspaces();
        saved.write(&home).unwrap();
        let Restored::Read(read) = Restored::read(&home) else {
            panic!("the file was not read");
        };
        assert_eq!(read, saved);

        let mut tree = DockTree::default();
        for workspace in saved.list() {
            tree.lay_out(&workspace.layout, |_| true);
            assert_eq!(tree.layout(), workspace.layout);
        }
        // A panel the dock does not know is left out, with its area.
        tree.lay_out(&saved.list()[0].layout, |id| {
            id != "engine/console"
        });
        let Layout::Split { second, .. } = tree.layout() else {
            panic!("the split went");
        };
        assert!(
            matches!(*second, Layout::Tabs { ref panels, shown: 1 }
            if panels.len() == 2)
        );
        let _ = fs::remove_dir_all(home.root());
    }

    #[test]
    fn a_file_without_icons_is_read_and_given_them() {
        let home = home("no_icons");
        let text = "(shown: 1, list: [(name: \"Main\", layout: \
                    Tabs(panels: [], shown: 0)), (name: \"Ledger\", \
                    layout: Tabs(panels: [], shown: 0))])";
        fs::write(home.path(FILE), text).unwrap();
        let Restored::Read(read) = Restored::read(&home) else {
            panic!("the file was not read");
        };
        let icons = read
            .list()
            .iter()
            .map(|workspace| workspace.icon)
            .collect::<Vec<_>>();
        assert_eq!(icons[0], Some(Glyph::House));
        assert!(icons[1].is_some_and(|icon| icon != Glyph::House));
        assert_eq!(read.shown(), 1);
        let _ = fs::remove_dir_all(home.root());
    }

    #[test]
    fn a_broken_file_is_kept_and_never_written_over() {
        let home = home("broken");
        let path = home.path(FILE);
        let kept = |name: &str| -> PathBuf { home.path(name) };
        for (attempt, aside) in
            ["workspaces_broken.ron", "workspaces_broken_2.ron"]
                .into_iter()
                .enumerate()
        {
            let text =
                format!("(shown: {attempt}, list: [(name: \"Mine\"");
            fs::write(&path, &text).unwrap();
            let Restored::KeptAside { kept: at, .. } =
                Restored::read(&home)
            else {
                panic!("the broken file was not kept");
            };
            assert_eq!(at, kept(aside));
            assert_eq!(fs::read_to_string(&at).unwrap(), text);
            assert!(!path.exists());
        }
        let _ = fs::remove_dir_all(home.root());
    }
}
