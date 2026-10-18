//! The read-only repository review panel. Git work runs off the UI
//! thread.

use std::path::PathBuf;

use bevy::prelude::*;
use bevy::tasks::{
    AsyncComputeTaskPool, Task, TaskPool, block_on, poll_once,
};
use bevy_fynix::dock::{DockTree, DockWindowKind};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, button, column, label, row,
    scroll, segmented, text_field,
};
use bevy_fynix::{ScopedExt, ViewExt, component, keyed, resource};
use hamlet_core::agents::{LivesIn, Works};
use hamlet_rules::review::highlight::Highlighting;
use hamlet_rules::review::{Comparison, Diff, File, Mark, Review};
use hamlet_world::pick::{Selected, Thing};
use tokio::sync::mpsc::{
    UnboundedReceiver, UnboundedSender, unbounded_channel,
};

use crate::dock::{PanelAppExt, panel};
use crate::icons::Glyph;
use crate::looks::{Fills, ICON, action, bare, brass, note, well};
use crate::widgets::diff::{self, Layout};
use crate::{AnyView, Cx};

pub fn plugin(app: &mut App) {
    app.add_panel(
        "review",
        DockWindowKind::new("DIFF", || panel(screen()))
            .icon(Glyph::Notes.image(ICON)),
    )
    .init_resource::<State>()
    .add_systems(
        Update,
        (
            receive.run_if(|s: Res<State>| !s.rx.is_empty()),
            finish_probes,
        ),
    );
}

#[derive(Component)]
struct RepositoryProbe {
    task: Option<Task<Result<bool, String>>>,
    found: Result<bool, String>,
}

fn finish_probes(
    mut probes: Query<(&mut RepositoryProbe, &mut Node)>,
) {
    for (mut probe, mut node) in &mut probes {
        if probe.task.is_none() {
            continue;
        }
        if let Some(task) = &mut probe.task
            && let Some(found) = block_on(poll_once(task))
        {
            probe.task = None;
            node.display = if matches!(found, Ok(false)) {
                Display::None
            } else {
                Display::Flex
            };
            probe.found = found;
        }
    }
}

/// A Diff action for a Git-backed work folder.
pub(crate) fn folder_action(folder: PathBuf) -> impl crate::View {
    AnyView::new(move |cx: &mut Cx| {
        let path = folder.clone();
        let task =
            AsyncComputeTaskPool::get_or_init(TaskPool::default)
                .spawn(async move {
                    Review::is_repository(&path)
                        .map_err(|e| format!("{e:#}"))
                });
        let root = cx.build(bare().display(Display::None));
        cx.world.entity_mut(root).insert(RepositoryProbe {
            task: Some(task),
            found: Ok(false),
        });
        cx.under(root, |cx| {
            cx.build(keyed(
                component::<RepositoryProbe, _>(root, |p| {
                    p.map(|p| p.found.clone()).unwrap_or(Ok(false))
                }),
                move |found| {
                    match found {
                        Ok(false) => return bare().boxed(),
                        Err(problem) => {
                            let problem = problem.clone();
                            return note("DIFF UNAVAILABLE")
                                .tooltip(move || {
                                    column((note(problem.clone()),))
                                        .max_width(px(320.0))
                                })
                                .boxed();
                        }
                        Ok(true) => {}
                    }
                    let folder = folder.clone();
                    action(Glyph::Notes, "DIFF", move |world| {
                        inspect(
                            world,
                            folder.clone(),
                            Comparison::Unstaged,
                        );
                        open(world);
                    })
                },
            ));
        });
        root
    })
}

type Loaded = Result<(Review, Option<(Diff, Highlighting)>), String>;

#[derive(Resource)]
struct State {
    folder: String,
    source: usize,
    base: String,
    target: String,
    merge_base: bool,
    layout: Layout,
    review: Option<Review>,
    scope: String,
    selected: usize,
    diff: Option<(Diff, Highlighting)>,
    problem: String,
    pending: bool,
    generation: u64,
    displayed: u64,
    tx: UnboundedSender<(u64, Loaded)>,
    rx: UnboundedReceiver<(u64, Loaded)>,
}

impl Default for State {
    fn default() -> Self {
        let (tx, rx) = unbounded_channel();
        Self {
            folder: std::env::current_dir()
                .unwrap_or_default()
                .display()
                .to_string(),
            source: 0,
            base: "HEAD~1".into(),
            target: "HEAD".into(),
            merge_base: true,
            layout: Layout::Unified,
            review: None,
            scope: String::new(),
            selected: 0,
            diff: None,
            problem: String::new(),
            pending: false,
            generation: 0,
            displayed: 0,
            tx,
            rx,
        }
    }
}

impl State {
    fn comparison(&self) -> Comparison {
        match self.source {
            0 => Comparison::Unstaged,
            1 => Comparison::Staged,
            _ => Comparison::Revisions {
                base: self.base.clone(),
                target: self.target.clone(),
                merge_base: self.merge_base,
            },
        }
    }

    fn read(&mut self, selected: Option<usize>) {
        self.generation += 1;
        self.pending = true;
        self.problem.clear();
        self.diff = None;
        self.selected = selected.unwrap_or(0);
        self.displayed += 1;
        let prior = selected.and(self.review.clone());
        if selected.is_none() {
            self.review = None;
            self.scope = match self.comparison() {
                Comparison::Unstaged => "Index → working tree \
                                         (including untracked files)"
                    .into(),
                Comparison::Staged => "HEAD → index".into(),
                Comparison::Revisions {
                    base,
                    target,
                    merge_base,
                } => {
                    if merge_base {
                        format!(
                            "Common ancestor of {base} and {target} \
                             → {target}"
                        )
                    } else {
                        format!("{base} → {target} (direct)")
                    }
                }
            };
        }
        let (folder, comparison, at) = (
            PathBuf::from(&self.folder),
            self.comparison(),
            self.selected,
        );
        let (tx, generation) = (self.tx.clone(), self.generation);
        AsyncComputeTaskPool::get_or_init(TaskPool::default)
            .spawn(async move {
                let result = (|| {
                    let review = match prior {
                        Some(review) => review,
                        None => Review::read(&folder, comparison)
                            .map_err(|e| format!("{e:#}"))?,
                    };
                    let diff = review.files.get(at).map(|f| {
                        let diff =
                            review.diff(f).unwrap_or_else(|error| {
                                Diff {
                                    description: format!(
                                        "Unable to preview this \
                                         file: {error:#}"
                                    ),
                                    ..Diff::default()
                                }
                            });
                        let highlighting = Highlighting::new(
                            &diff,
                            f.before.as_deref().unwrap_or(&f.path),
                            &f.path,
                        );
                        (diff, highlighting)
                    });
                    Ok((review, diff))
                })();
                let _ = tx.send((generation, result));
            })
            .detach();
    }
}

fn receive(mut state: ResMut<State>) {
    while let Ok((generation, result)) = state.rx.try_recv() {
        if generation != state.generation {
            continue;
        }
        state.pending = false;
        state.displayed += 1;
        match result {
            Ok((review, diff)) => {
                state.review = Some(review);
                state.diff = diff;
            }
            Err(problem) => {
                state.problem = problem;
            }
        }
    }
}

/// True while the panel is reading Git data.
pub fn waiting(world: &World) -> bool {
    world.get_resource::<State>().is_some_and(|s| s.pending)
}

/// Loads a comparison for a repository into the panel.
pub fn inspect(
    world: &mut World,
    folder: PathBuf,
    comparison: Comparison,
) {
    let mut state = world.resource_mut::<State>();
    state.folder = folder.display().to_string();
    state.source = match comparison {
        Comparison::Unstaged => 0,
        Comparison::Staged => 1,
        Comparison::Revisions {
            base,
            target,
            merge_base,
        } => {
            state.base = base;
            state.target = target;
            state.merge_base = merge_base;
            2
        }
    };
    state.read(None);
}

/// Changes the panel's line layout.
pub fn set_layout(world: &mut World, layout: Layout) {
    world.resource_mut::<State>().layout = layout;
}

/// Opens Diff in an available tab group, or brings its existing tab
/// forward.
pub(crate) fn open(world: &mut World) {
    let mut tree = world.resource_mut::<DockTree>();
    if tree.find_leaf_with_window("review").is_none() {
        let leaf = tree
            .find_leaf_with_window("world")
            .or_else(|| tree.find_leaf_with_window("agent"))
            .or_else(|| {
                tree.nodes.iter().find_map(|(id, node)| {
                    node.as_leaf().map(|_| *id)
                })
            });
        if let Some(leaf) = leaf {
            tree.add_tab(leaf, "review");
        }
    }
    crate::dock::show_tab(world, "review");
}

fn refresh(world: &mut World) {
    world.resource_mut::<State>().read(None);
}

fn selected_folder(world: &World) -> Option<PathBuf> {
    let selected = world.get_resource::<Selected>().and_then(|s| s.0);
    let hamlet = match selected {
        Some(
            Thing::Common(h)
            | Thing::Clockwork(h)
            | Thing::Workbench(h),
        ) => h,
        Some(
            Thing::Resident(a)
            | Thing::ResidentHouse(a)
            | Thing::Ruin(a),
        ) => world.get::<LivesIn>(a)?.0,
        _ => {
            world
                .get::<LivesIn>(
                    world.get_resource::<crate::Me>()?.agent,
                )?
                .0
        }
    };
    world.get::<Works>(hamlet)?.0.folder.clone()
}

fn controls() -> impl crate::View {
    let revisions =
        keyed(resource::<State, _>(|s| s.source == 2), |revisions| {
            if !revisions {
                return bare().boxed();
            }
            column((
                row((
                    note("BASE"),
                    text_field(
                        resource::<State, _>(|s| s.base.clone()),
                        |w, text| {
                            w.resource_mut::<State>().base = text
                        },
                    )
                    .grow(1.0)
                    .min_width(px(60.0))
                    .rules(well),
                    note("TARGET"),
                    text_field(
                        resource::<State, _>(|s| s.target.clone()),
                        |w, text| {
                            w.resource_mut::<State>().target = text
                        },
                    )
                    .grow(1.0)
                    .min_width(px(60.0))
                    .rules(well),
                ))
                .align(AlignItems::Center)
                .gap(8.0),
                segmented(
                    ["SINCE COMMON ANCESTOR", "DIRECT"],
                    resource::<State, _>(|s| {
                        usize::from(!s.merge_base)
                    }),
                    |w, at| {
                        w.resource_mut::<State>().merge_base =
                            at == 0;
                        refresh(w);
                    },
                ),
            ))
            .gap(8.0)
            .boxed()
        })
        .within(bare());
    column((
        row((
            note("REPOSITORY"),
            text_field(
                resource::<State, _>(|s| s.folder.clone()),
                |w, text| w.resource_mut::<State>().folder = text,
            )
            .grow(1.0)
            .min_width(px(60.0))
            .rules(well),
            action(Glyph::Restore, "REFRESH", refresh),
        ))
        .gap(8.0)
        .align(AlignItems::Center),
        row((
            segmented(
                ["UNSTAGED", "STAGED", "REVISIONS"],
                resource::<State, _>(|s| s.source),
                |w, at| {
                    w.resource_mut::<State>().source = at;
                    refresh(w);
                },
            ),
            segmented(
                ["UNIFIED", "SPLIT"],
                resource::<State, _>(|s| {
                    usize::from(s.layout == Layout::Split)
                }),
                |w, at| {
                    set_layout(
                        w,
                        if at == 0 {
                            Layout::Unified
                        } else {
                            Layout::Split
                        },
                    )
                },
            ),
        ))
        .gap(12.0)
        .tagged(crate::platform::Wraps),
        revisions,
        note(resource::<State, _>(|s| {
            if s.source == 2 {
                "Base and target accept branches, tags or commits. \
                 Refresh applies edited fields."
            } else {
                "Read-only. Refresh to pick up changes on disk. \
                 Unstaged includes untracked files."
            }
            .to_owned()
        })),
        note(resource::<State, _>(|s| match &s.review {
            Some(review) => format!(
                "Loaded repository: {}\nComparison: \
                 {}\nRepository-wide changes; not attributed to an \
                 agent or task.",
                review.root.display(),
                s.scope
            ),
            None => "Repository-wide changes; not attributed to an \
                     agent or task."
                .into(),
        })),
    ))
    .gap(8.0)
}

fn file_name(file: &File) -> String {
    let path = file
        .path
        .to_string_lossy()
        .replace('\n', "\\n")
        .replace('\t', "\\t");
    match &file.before {
        Some(before) => {
            format!("{} → {path}", before.to_string_lossy())
        }
        None => path,
    }
}

fn files() -> AnyView {
    keyed(resource::<State, _>(|s| (s.displayed, s.selected)), |_| {
        AnyView::new(|cx: &mut Cx| {
            let state = cx.world.resource::<State>();
            let files = state
                .review
                .as_ref()
                .map(|r| r.files.as_slice())
                .unwrap_or_default();
            let count = if state.pending || !state.problem.is_empty()
            {
                String::new()
            } else {
                format!(
                    "{} {}",
                    files.len(),
                    if files.len() == 1 { "FILE" } else { "FILES" }
                )
            };
            let mut views = vec![note(count).boxed()];
            for (at, file) in files.iter().enumerate() {
                let chosen = at == state.selected;
                let tone =
                    if chosen { Tone::Accent } else { Tone::Dim };
                views.push(
                    button(
                        label(format!(
                            "{}  {}",
                            file.status,
                            file_name(file)
                        ))
                        .size(12.0)
                        .tone(tone)
                        .wrap(false),
                    )
                    .min_height(px(28.0))
                    .shrink(0.0)
                    .rules(brass)
                    .on_activate(move |w| {
                        w.resource_mut::<State>().read(Some(at))
                    })
                    .boxed(),
                );
            }
            cx.build(scroll(views).gap(4.0).fills_height())
        })
    })
    .within(
        bare()
            .width(px(230.0))
            .min_width(px(100.0))
            .min_height(px(0.0)),
    )
    .boxed()
}

/// The Diff panel's controls, file list and diff viewport.
pub fn screen() -> impl crate::View {
    AnyView::new(|cx: &mut Cx| {
        if cx.world.resource::<State>().generation == 0 {
            if let Some(folder) = selected_folder(cx.world) {
                cx.world.resource_mut::<State>().folder =
                    folder.display().to_string();
            }
            refresh(cx.world);
        }
        let body = keyed(
            resource::<State, _>(|s| (s.displayed, s.layout)),
            |_| {
                AnyView::new(|cx: &mut Cx| {
                    let state = cx.world.resource::<State>();
                    if state.pending {
                        return cx
                            .build(note("Reading Git changes..."));
                    }
                    if !state.problem.is_empty() {
                        return cx.build(
                            label(state.problem.clone())
                                .size(12.0)
                                .tone(Tone::Critical),
                        );
                    }
                    let Some((diff, highlighting)) = &state.diff
                    else {
                        return cx.build(note(
                            "No changes in this comparison.",
                        ));
                    };
                    let file = state
                        .review
                        .as_ref()
                        .and_then(|r| r.files.get(state.selected))
                        .map(file_name)
                        .unwrap_or_default();
                    let added = diff
                        .lines
                        .iter()
                        .filter(|l| l.mark == Mark::Add)
                        .count();
                    let deleted = diff
                        .lines
                        .iter()
                        .filter(|l| l.mark == Mark::Delete)
                        .count();
                    let view = column((
                        label(format!(
                            "{file}    +{added}  −{deleted}"
                        ))
                        .size(13.0)
                        .tone(Tone::Body),
                        note(format!(
                            "{}{}",
                            if state.layout == Layout::Split {
                                "Old on the left, new on the right"
                            } else {
                                "Line numbers: old / new"
                            },
                            if highlighting.limited {
                                ". Some highlighting omitted \
                                 (size/time limit)."
                            } else {
                                ""
                            },
                        )),
                        diff::view(
                            diff.clone(),
                            highlighting.clone(),
                            state.layout,
                        ),
                    ))
                    .gap(6.0)
                    .fills_height()
                    .min_width(px(0.0));
                    cx.build(view)
                })
            },
        )
        .within(bare().width(px(0.0)).fills_width().fills_height());
        cx.build(
            column((
                controls(),
                row((files(), body))
                    .gap(12.0)
                    .fills_height()
                    .min_width(px(0.0)),
            ))
            .fills_height(),
        )
    })
}

#[cfg(test)]
mod tests {
    use bevy::asset::AssetPlugin;
    use bevy::camera::{ComputedCameraValues, RenderTargetInfo};
    use bevy::input::InputPlugin;
    use bevy::text::TextPlugin;
    use bevy::time::TimePlugin;
    use bevy_fynix::dock::{DockAreaStyle, DockLeaf};
    use bevy_fynix::{FynixPlugin, mount};
    use hamlet_assets::Tuning;
    use hamlet_rules::review::Line;

    use super::*;
    use crate::theme::{self, HamletTheme};

    #[test]
    fn changing_files_keeps_the_review_columns_and_controls_in_place()
    {
        let mut app = App::new();
        app.add_plugins((
            bevy::app::TaskPoolPlugin::default(),
            TimePlugin,
            AssetPlugin::default(),
            InputPlugin,
            bevy::picking::PickingPlugin,
            bevy::picking::InteractionPlugin,
            bevy::input_focus::InputDispatchPlugin,
            bevy::image::ImagePlugin::default(),
            TextPlugin,
            bevy::ui::UiPlugin,
            FynixPlugin::<HamletTheme>::default(),
            diff::plugin,
        ));
        app.init_asset::<TextureAtlasLayout>();
        theme::insert_theme(app.world_mut(), HamletTheme::designed());
        app.insert_resource(State {
            generation: 1,
            source: 2,
            ..default()
        });
        app.world_mut().spawn((
            Camera2d,
            Camera {
                computed: ComputedCameraValues {
                    target_info: Some(RenderTargetInfo {
                        physical_size: UVec2::new(900, 700),
                        scale_factor: 1.0,
                    }),
                    ..default()
                },
                ..default()
            },
        ));
        mount::<HamletTheme>(
            app.world_mut(),
            column((screen(),)).width(px(900.0)).height(px(700.0)),
        );
        let mut positions = Vec::new();
        for (layout, width, lines) in [
            (Layout::Unified, 20, 5),
            (Layout::Unified, 200, 5),
            (Layout::Unified, 20, 1_000),
            (Layout::Split, 200, 5),
            (Layout::Split, 20, 5),
        ] {
            let diff = Diff {
                lines: (1..=lines)
                    .map(|new| Line {
                        old: None,
                        new: Some(new),
                        mark: Mark::Add,
                        text: "x".repeat(width),
                        no_newline: false,
                    })
                    .collect(),
                ..default()
            };
            let mut state = app.world_mut().resource_mut::<State>();
            state.diff = Some((diff, Highlighting::default()));
            state.layout = layout;
            state.displayed += 1;
            for _ in 0..10 {
                app.update();
            }
            let world = app.world_mut();
            let mut texts = world.query::<(
                &Text,
                &ComputedNode,
                &UiGlobalTransform,
            )>();
            let mut position = |text: &str| {
                let (_, node, transform) = texts
                    .iter(world)
                    .find(|(t, _, _)| t.0 == text)
                    .unwrap();
                transform.translation - node.size() / 2.0
            };
            let content = position(if layout == Layout::Unified {
                "Line numbers: old / new"
            } else {
                "Old on the left, new on the right"
            });
            assert_eq!(
                content.x - position("0 FILES").x,
                230.0 + 12.0,
                "only the diff should grow into the available width"
            );
            positions.push((
                position("BASE"),
                position("TARGET"),
                content,
            ));
        }
        for position in &positions[1..] {
            assert_eq!(
                positions[0], *position,
                "source must scroll without moving the file list or \
                 squeezing controls"
            );
        }
    }

    #[test]
    fn diff_opens_once_when_world_and_agent_tabs_are_closed() {
        let mut world = World::new();
        let mut tree = DockTree::default();
        let leaf = tree.set_root_leaf(
            DockLeaf::new("side", DockAreaStyle::TabBar)
                .with_windows(vec!["settings".into()]),
        );
        world.insert_resource(tree);
        open(&mut world);
        open(&mut world);
        let tree = world.resource::<DockTree>();
        let tabs =
            tree.leaf(leaf).unwrap().tabs().collect::<Vec<_>>();
        assert_eq!(tabs.len(), 2);
        let diff =
            tabs.iter().find(|(id, _)| *id == "review").unwrap();
        assert_eq!(tree.active(leaf), Some(diff.1));
    }
}
