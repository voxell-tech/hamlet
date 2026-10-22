//! Stills of the engine's widgets and chrome, drawn into an image
//! with no window and saved as PNGs.
//!
//! `cargo run -p hamlet_engine --example stills -- --out <dir>`, and
//! `--scene <name>` for one of `SHOTS`. The engine's home is
//! `<dir>/home`, made empty first.

use std::path::{Path, PathBuf};
use std::process::Command as Process;
use std::time::Duration;

use bevy::app::ScheduleRunnerPlugin;
use bevy::camera::{ImageRenderTarget, RenderTarget};
use bevy::picking::PickingSettings;
use bevy::picking::hover::Hovered as PickingHovered;
use bevy::prelude::*;
use bevy::render::RenderPlugin;
use bevy::render::render_resource::TextureFormat;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy::text::EditableText;
use bevy::time::TimeUpdateStrategy;
use bevy::ui_widgets::{Activate, Button as ButtonBehavior};
use bevy::window::{ExitCondition, WindowResolution};
use bevy::winit::WinitPlugin;
use bevy_fynix::dock::DockTree;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, label, row,
};
use bevy_fynix::{Focused, Pressed, ViewExt};
use hamlet_engine_api::{Ask, Capability, Manifest, PluginId};
use hamlet_engine_core::Home;
use hamlet_engine_plugins::{
    Entry, EntryPoint, PluginCommandsExt, PluginInfo, PluginKind,
};
use hamlet_engine_shell::fonts::DisplayText;
use hamlet_engine_shell::icons::{Glyph, glyph, side};
use hamlet_engine_shell::style::{
    BottomGradient, ButtonStyle, Wraps, action, card, choices,
    divider, field, heading, icon_button, labelled_switch, link_row,
    link_rows, list, note, page_head, section, switch, title_button,
};
use hamlet_engine_shell::{
    AnyView, Cx, DockTreeExt, EngineTheme, StyleExt,
};

/// One still: its name, its size in the interface's pixels and the
/// pixels to one of them.
struct Shot {
    name: &'static str,
    size: UVec2,
    scale: f32,
}

const SHOTS: [Shot; 5] = [
    Shot {
        name: "controls",
        size: UVec2::new(900, 1100),
        scale: 2.0,
    },
    Shot {
        name: "icons",
        size: UVec2::new(1200, 560),
        scale: 1.0,
    },
    Shot {
        name: "shell",
        size: UVec2::new(1440, 860),
        scale: 1.0,
    },
    Shot {
        name: "menu",
        size: UVec2::new(1440, 860),
        scale: 1.0,
    },
    Shot {
        name: "settings",
        size: UVec2::new(1440, 860),
        scale: 1.0,
    },
];

/// The update the buttons shown as done are pressed on, the updates
/// after it the still is taken, and the updates its file is given to
/// be written.
const STRIKE: u32 = 240;
const AFTER: u32 = 7;
const WRITE: u32 = 60;

/// The update the panels of a scene of the shell are set out on, and
/// the one its menu is opened on.
const ARRANGE: u32 = 30;
const MENU: u32 = 120;

fn main() {
    let out = word("--out").map(PathBuf::from).unwrap_or_else(|| {
        std::env::temp_dir().join("hamlet_engine_stills")
    });
    std::fs::create_dir_all(&out).expect("output folder");
    let Some(scene) = word("--scene") else {
        // A scene to a process: each starts from nothing.
        let me = std::env::current_exe().expect("this program");
        for shot in &SHOTS {
            let status = Process::new(&me)
                .args(["--scene", shot.name, "--out"])
                .arg(&out)
                .status()
                .expect("run a scene");
            assert!(status.success(), "{} failed", shot.name);
        }
        return;
    };
    let shot = SHOTS
        .iter()
        .find(|shot| shot.name == scene)
        .expect("a scene named in SHOTS");
    shot.draw(&out);
}

/// The argument after `name` on the command line.
fn word(name: &str) -> Option<String> {
    let args = std::env::args().collect::<Vec<_>>();
    let at = args.iter().position(|a| a == name)?;
    args.get(at + 1).cloned()
}

impl Shot {
    /// Draws the still into `out`.
    fn draw(&self, out: &Path) {
        let home = out.join("home");
        let _ = std::fs::remove_dir_all(&home);
        let mut app = App::new();
        app.insert_resource(Home::at(&home))
            .insert_resource(hamlet_engine_net::Offline)
            .add_plugins(
                DefaultPlugins
                    // A window entity with no winit behind it.
                    .set(WindowPlugin {
                        primary_window: Some(Window {
                            resolution: WindowResolution::new(
                                self.size.x,
                                self.size.y,
                            )
                            .with_scale_factor_override(1.0),
                            ..default()
                        }),
                        exit_condition: ExitCondition::DontExit,
                        ..default()
                    })
                    .set(RenderPlugin {
                        synchronous_pipeline_compilation: true,
                        ..default()
                    })
                    .disable::<WinitPlugin>()
                    .add(ScheduleRunnerPlugin::run_loop(
                        Duration::ZERO,
                    )),
            )
            .add_plugins((
                hamlet_engine_core::plugin,
                hamlet_engine_shell::plugin,
                hamlet_engine_plugins::plugin,
                hamlet_engine_grants::plugin,
                hamlet_engine_assets::plugin,
                hamlet_engine_bus::plugin,
                hamlet_engine_net::plugin,
                hamlet_engine_render::plugin,
                hamlet_engine_wasm::plugin,
            ));
        let pixels = (self.size.as_vec2() * self.scale).as_uvec2();
        let image = Image::new_target_texture(
            pixels.x,
            pixels.y,
            TextureFormat::Rgba8UnormSrgb,
            None,
        );
        let image = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(image);
        let target = RenderTarget::Image(ImageRenderTarget {
            handle: image,
            scale_factor: self.scale,
        });
        app.insert_resource(Still {
            target,
            path: out.join(format!("{}.png", self.name)),
            update: 0,
        })
        .insert_resource(TimeUpdateStrategy::ManualDuration(
            Duration::from_secs_f64(1.0 / 60.0),
        ))
        // No pointer moves here, so a held hover stays held.
        .insert_resource(PickingSettings {
            is_hover_enabled: false,
            ..default()
        })
        .add_systems(PostStartup, aim)
        .add_systems(Update, (reach_in, hold, capture).chain());
        match self.name {
            "controls" => {
                app.add_systems(PostStartup, |world: &mut World| {
                    bevy_fynix::mount::<EngineTheme>(
                        world,
                        controls(),
                    );
                });
            }
            "icons" => {
                app.add_systems(PostStartup, |world: &mut World| {
                    bevy_fynix::mount::<EngineTheme>(
                        world,
                        icon_sheet(),
                    );
                });
            }
            name => {
                let scene = match name {
                    "menu" => Shell::Menu,
                    "settings" => Shell::Settings,
                    _ => Shell::Panels,
                };
                app.insert_resource(scene)
                    .add_systems(Startup, add_plugins)
                    .add_systems(Update, (arrange, open_menu));
            }
        }
        app.run();
    }
}

/// The image drawn into, where its still goes, and the updates so
/// far.
#[derive(Resource)]
struct Still {
    target: RenderTarget,
    path: PathBuf,
    update: u32,
}

/// What a still of the whole shell shows besides its panels.
#[derive(Resource, Clone, Copy, PartialEq, Eq)]
enum Shell {
    Panels,
    /// The first menu of the bar, open.
    Menu,
    /// The Settings panel showing its link rows.
    Settings,
}

/// Points the window's camera at the image.
fn aim(
    cameras: Query<Entity, With<Camera2d>>,
    still: Res<Still>,
    mut commands: Commands,
) {
    for camera in &cameras {
        commands.entity(camera).insert(still.target.clone());
    }
}

/// Takes the still a moment after the strike, and leaves once it is
/// written.
fn capture(
    mut still: ResMut<Still>,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
) {
    still.update += 1;
    if still.update == STRIKE + AFTER {
        commands
            .spawn(Screenshot(still.target.clone()))
            .observe(save_to_disk(still.path.clone()));
    }
    if still.update > STRIKE + AFTER + WRITE {
        exit.write(AppExit::Success);
    }
}

/// A control held in one state.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Held {
    Hover,
    Press,
    Done,
    Focus,
}

/// Keeps every held control in its state, and presses the ones shown
/// as done on the strike.
fn hold(
    held: Query<(Entity, &Held, Option<&PickingHovered>)>,
    still: Res<Still>,
    mut commands: Commands,
) {
    for (control, state, hovered) in &held {
        let mut entity = commands.entity(control);
        if *state == Held::Focus {
            entity.insert(Focused);
            continue;
        }
        if hovered.is_none_or(|hovered| !hovered.0) {
            entity.insert(PickingHovered(true));
        }
        if *state == Held::Press {
            entity.insert((Pressed, bevy::ui::Pressed));
        }
        if *state == Held::Done && still.update == STRIKE {
            commands.trigger(Activate { entity: control });
        }
    }
}

/// Moves each [`HoldInside`] to the field or the button inside it,
/// once that is built.
fn reach_in(
    frames: Query<(Entity, &HoldInside)>,
    children: Query<&Children>,
    texts: Query<&ChildOf, With<EditableText>>,
    buttons: Query<(), With<ButtonBehavior>>,
    mut commands: Commands,
) {
    for (frame, held) in &frames {
        let inside =
            children.iter_descendants(frame).find_map(|node| {
                if let Ok(text) = texts.get(node) {
                    return Some(text.parent());
                }
                buttons.contains(node).then_some(node)
            });
        if let Some(inside) = inside {
            commands.entity(inside).insert(held.0);
            commands.entity(frame).remove::<HoldInside>();
        }
    }
}

/// On a frame whose first field or button is held in a state.
#[derive(Component, Clone, Copy)]
struct HoldInside(Held);

/// The stills' methods on [`AnyView`].
trait StillViewExt {
    /// This view with its first field or button held as `held`.
    fn held_inside(self, held: Held) -> AnyView;
}

impl StillViewExt for AnyView {
    fn held_inside(self, held: Held) -> AnyView {
        row((self,)).tagged(HoldInside(held)).boxed()
    }
}

/// A caption over what it names.
fn cell(name: &'static str, view: AnyView) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let theme = cx.theme();
        let (size, gap) = (theme.text.caption, theme.space.gap);
        cx.build(
            column((
                label(name).size(size).tone(Tone::Dim).wrap(false),
                view,
            ))
            .gap(gap)
            .width(px(168.0))
            .align(AlignItems::FlexStart),
        )
    })
}

/// Cells side by side.
fn cells(cells: Vec<AnyView>) -> AnyView {
    row(cells).gap(16.0).boxed()
}

/// A button of the style `kind` in each of its states.
fn states(
    kind: ButtonStyle,
    which: Glyph,
    text: &'static str,
) -> AnyView {
    let one =
        move || icon_button(kind, which, text).on_activate(|_| {});
    cells(vec![
        cell("AT REST", one().boxed()),
        cell("UNDER THE POINTER", one().tagged(Held::Hover).boxed()),
        cell("HELD", one().tagged(Held::Press).boxed()),
        cell("DONE", one().tagged(Held::Done).boxed()),
    ])
}

fn fields() -> AnyView {
    let one = |text: &str| {
        column(
            (field(text.to_owned(), "A reason".into(), |_, _| {}),),
        )
        .width(px(160.0))
        .boxed()
    };
    cells(vec![
        cell("AT REST", one("A reason")),
        cell("UNDER THE POINTER", one("").held_inside(Held::Hover)),
        cell("TYPED IN", one("A reason").held_inside(Held::Focus)),
    ])
}

fn titles() -> AnyView {
    let bar = |view: AnyView| row((view,)).height(px(34.0)).boxed();
    cells(vec![
        cell("AT REST", bar(title_button("HAMLET", false).boxed())),
        cell(
            "UNDER THE POINTER",
            bar(title_button("VIEW", false)
                .tagged(Held::Hover)
                .boxed()),
        ),
        cell(
            "ITS MENU OPEN",
            bar(title_button("VIEW", true).boxed()),
        ),
    ])
}

fn tab(which: Glyph, name: &'static str, shown: bool) -> AnyView {
    label(name)
        .wrap(false)
        .tagged(DisplayText)
        .with_icon(which)
        .in_tab(shown)
}

fn tabs() -> AnyView {
    cells(vec![
        cell("AT REST", tab(Glyph::Page, "NOTES", false)),
        cell(
            "UNDER THE POINTER",
            tab(Glyph::Page, "NOTES", false).held_inside(Held::Hover),
        ),
        cell("THE ONE SHOWN", tab(Glyph::Speech, "TALK", true)),
    ])
}

fn choosing() -> AnyView {
    let answers = || {
        vec![
            "Allow".to_owned(),
            "Ask next time".into(),
            "Don't allow".into(),
        ]
    };
    row((
        column((
            label("THE MIDDLE ONE PICKED").size(11.0).tone(Tone::Dim),
            choices(answers(), 1, |_, _| {}),
        ))
        .gap(8.0),
        column((
            label("THE FIRST UNDER THE POINTER")
                .size(11.0)
                .tone(Tone::Dim),
            choices(answers(), 1, |_, _| {}).held_inside(Held::Hover),
        ))
        .gap(8.0),
    ))
    .gap(48.0)
    .boxed()
}

fn the_rest() -> AnyView {
    row((
        column((
            label("A CARD").size(11.0).tone(Tone::Dim),
            card((
                section("ORCHARD"),
                note("Prune, graft and keep the press."),
            )),
            label("LINK ROWS").size(11.0).tone(Tone::Dim),
            link_rows(vec![
                link_row(
                    Glyph::Sliders,
                    "PERMISSIONS",
                    Some("What each plugin may do.".into()),
                ),
                link_row(Glyph::Globe, "NETWORK", None)
                    .tagged(Held::Hover)
                    .boxed(),
            ]),
            page_head(
                "SETTINGS",
                |_| {},
                Glyph::Sliders,
                "PERMISSIONS",
            ),
        ))
        .gap(8.0)
        .width(px(300.0)),
        column((
            label("A LIST").size(11.0).tone(Tone::Dim),
            list(vec![
                label("Pears").boxed(),
                label("Apples").boxed(),
                label("Quinces").boxed(),
            ]),
        ))
        .gap(8.0)
        .width(px(160.0)),
        column((
            label("SWITCHES").size(11.0).tone(Tone::Dim),
            row((switch(false, |_, _| {}), switch(true, |_, _| {})))
                .gap(8.0),
            label("WORDS").size(11.0).tone(Tone::Dim),
            heading("A heading"),
            note("A note"),
            divider(),
        ))
        .gap(8.0)
        .width(px(200.0)),
    ))
    .gap(28.0)
    .align(AlignItems::FlexStart)
    .boxed()
}

fn controls() -> AnyView {
    let part = |name: &'static str, view: AnyView| {
        column((section(name), view)).gap(10.0).boxed()
    };
    AnyView::new(move |cx: &mut Cx| {
        let fill = *cx.theme().colors.background;
        cx.build(
            column((
                part(
                    "A PLAIN BUTTON",
                    states(
                        ButtonStyle::Plain,
                        Glyph::SpeechPlus,
                        "NEW SESSION",
                    ),
                ),
                part(
                    "THE PRIMARY ACTION",
                    states(
                        ButtonStyle::Primary,
                        Glyph::Check,
                        "ALLOW ONCE",
                    ),
                ),
                part(
                    "WHAT REFUSES OR STOPS",
                    states(
                        ButtonStyle::Critical,
                        Glyph::Cross,
                        "DENY",
                    ),
                ),
                part(
                    "A WAY BACK",
                    states(
                        ButtonStyle::Quiet,
                        Glyph::Back,
                        "ALL PLUGINS",
                    ),
                ),
                part("A TEXT FIELD", fields()),
                part("MENU TITLES", titles()),
                part("TABS", tabs()),
                part("CHOICES", choosing()),
                part("CARDS, LINK ROWS, LISTS AND WORDS", the_rest()),
            ))
            .position(PositionType::Absolute)
            .inset(UiRect::all(px(0.0)))
            .z(Some(1))
            .padding(UiRect::all(px(28.0)))
            .gap(20.0)
            .fill(fill)
            .overflow(Overflow::clip())
            .tagged(BottomGradient),
        )
    })
}

/// Every icon at each size, in the tones it is tinted with, and
/// large.
fn icon_sheet() -> AnyView {
    let cells = Glyph::ALL
        .into_iter()
        .map(|which| {
            let small = [12.0, 14.0, 16.0, 20.0]
                .into_iter()
                .map(|size| {
                    glyph(which, size).tone(Tone::Faint).boxed()
                })
                .chain([
                    glyph(which, 14.0).tone(Tone::Accent).boxed(),
                    glyph(which, 14.0).tone(Tone::Dim).boxed(),
                ])
                .collect::<Vec<_>>();
            column((
                label(which.name()).size(11.0).tone(Tone::Dim),
                row((
                    glyph(which, 40.0).tone(Tone::Body),
                    row(small).gap(6.0).align(AlignItems::Center),
                ))
                .gap(10.0)
                .align(AlignItems::Center),
            ))
            .gap(6.0)
            .width(px(176.0))
            .boxed()
        })
        .collect::<Vec<_>>();
    AnyView::new(move |cx: &mut Cx| {
        let fill = *cx.theme().colors.background;
        cx.build(
            row(cells)
                .gap(18.0)
                .position(PositionType::Absolute)
                .inset(UiRect::all(px(0.0)))
                .z(Some(1))
                .padding(UiRect::all(px(24.0)))
                .fill(fill)
                .tagged(Wraps),
        )
    })
}

/// A plugin built into the stills, with a panel of the shell's
/// widgets, and one installed that asks for what it needs.
fn add_plugins(mut commands: Commands) {
    let manifest = Manifest::new(
        PluginId::try_from("hamlet.orchard").expect("a valid id"),
        "Orchard",
        "0.1.0",
    );
    commands.add_builtin(manifest, entry);
    let mut manifest = Manifest::new(
        PluginId::try_from("hamlet.almanac").expect("a valid id"),
        "Almanac",
        "0.2.0",
    );
    manifest.asks = vec![Ask {
        capability: Capability::NetFetch,
        target: "almanac.example".into(),
        reason: "Reads the frost dates for the orchard.".into(),
    }];
    commands.spawn((
        PluginInfo::of(manifest, PluginKind::Installed),
        EntryPoint(|_: &mut Entry| {}),
    ));
}

fn entry(entry: &mut Entry) {
    entry.panel("orchard", "Orchard", orchard);
}

fn orchard(_: Entity) -> AnyView {
    AnyView::new(|cx: &mut Cx| {
        let gap = cx.theme().space.gap;
        cx.build(
            column((
                heading("The orchard"),
                label("Three rows, pruned in spring."),
                list(vec![
                    label("Pears").boxed(),
                    label("Apples").boxed(),
                ]),
                field(String::new(), "A new row".into(), |_, _| {}),
                labelled_switch("Warn of frost", true, |_, _| {}),
                row((
                    side(Glyph::Bell, Tone::Faint),
                    action(ButtonStyle::Plain, "Add a row"),
                ))
                .gap(gap)
                .align(AlignItems::Center),
                divider(),
            ))
            .gap(gap),
        )
    })
    .in_panel()
}

/// Sets out the panels of a still of the shell: the plugin's panel
/// beside the console, or Settings there and the plugin list in
/// front.
fn arrange(
    still: Res<Still>,
    scene: Res<Shell>,
    mut tree: ResMut<DockTree>,
) {
    if still.update != ARRANGE {
        return;
    }
    let Some(leaf) = tree.find_leaf_with_window("engine/console")
    else {
        return;
    };
    tree.add_tab(leaf, "hamlet.orchard/orchard");
    if *scene == Shell::Settings {
        tree.add_tab(leaf, "engine/settings");
        tree.open_panel("engine/plugins");
    }
}

/// Opens the first menu of the bar, as a press on its title would.
fn open_menu(
    still: Res<Still>,
    scene: Res<Shell>,
    labels: Query<(Entity, &Text)>,
    parents: Query<&ChildOf>,
    buttons: Query<(), With<ButtonBehavior>>,
    mut commands: Commands,
) {
    if *scene != Shell::Menu || still.update != MENU {
        return;
    }
    let title = labels
        .iter()
        .find(|(_, text)| text.0 == "HAMLET")
        .and_then(|(label, _)| {
            parents
                .iter_ancestors(label)
                .find(|above| buttons.contains(*above))
        });
    if let Some(title) = title {
        commands.trigger(Activate { entity: title });
    }
}
