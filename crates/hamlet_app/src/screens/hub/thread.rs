//! The Hub's thread: the chat with one agent, over a composer.

use bevy::app::{App, PostUpdate, Update};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::{ChildOf, Children};
use bevy::ecs::query::{Has, With, Without};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::ui::{
    AlignItems, ComputedNode, Node, Overflow, UiRect, UiSystems, Val,
    percent, px,
};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, button, column, frame,
    label, menu_button, row, scroll,
};
use bevy_fynix::{
    ScopedExt, Signal, ViewExt, component, derived, every_frame,
    keyed, resource, style,
};
use hamlet_client::messages::Compact;
use hamlet_client::realm::{
    Activity, Busy, Kind, Left, LivesIn, Parent, Profile, Works,
    id_of, name_of, shown_of,
};
use hamlet_desktop::choose::What;
use hamlet_protocol::client as protocol;
use hamlet_realm::pick::{Thing, select};
use hamlet_ui::kinds::card;
use hamlet_ui::styles::{
    Fills, ICON, Looks, Tipped, action, bare, card_name, choice,
    crumb, crumb_here, crumb_slash, crumbs, filling, hairline,
    icon_button, more_button, note, note_rules, page_head,
    scroll_area, status_icon, title, transparent,
};
use hamlet_ui::theme::{HamletTheme, Themed};
use hamlet_ui::widgets::edges::faded;
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::ellipsis::one_line;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::widgets::tabs::{self, Spread};
use hamlet_ui::{AnyView, Cx, View, built, with_cx};

use super::{Showing, details, show};
use crate::convo::Convo;
use crate::overview::tell;
use crate::profile_tag::{ProfileTag, learned_mark, learned_words};
use crate::roll::rolled;
use crate::screens::wait;
use crate::standing::Standing;
use crate::world_menu::ChipName;
use crate::{chat, words, world_menu};

/// The tabs between a thread's chat and its details.
const SHEETS: [(Sheet, Glyph, &str); 2] = [
    (Sheet::Chat, Glyph::Talk, "Chat"),
    (Sheet::Details, Glyph::Sliders, "Details"),
];

pub(super) fn plugin(app: &mut App) {
    app.add_systems(Update, mark_open_chips)
        .add_systems(PostUpdate, crowd.after(UiSystems::Layout));
}

/// What the thread's page shows.
#[derive(Resource, Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum Sheet {
    #[default]
    Chat,
    Details,
}

/// The system prompt of an agent, as it was last read for the chief.
#[derive(Resource, Default, PartialEq)]
pub struct Briefing {
    pub agent: Option<Entity>,
    pub prompt: Vec<protocol::PromptSection>,
    /// Why it could not be read.
    pub failed: Option<String>,
    /// Whether it went to the clipboard since it was read.
    pub copied: bool,
}

impl Briefing {
    /// The prompt as the model reads it, in one text.
    fn whole(&self) -> String {
        self.prompt
            .iter()
            .map(|section| section.text.as_str())
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

/// An agent's purpose cut short, whole under the pointer.
pub(super) fn said(purpose: String) -> AnyView {
    let shown =
        crate::hover::short(&purpose, crate::hover::PURPOSE_SHOWN);
    if shown == purpose {
        return dim(purpose).boxed();
    }
    label(shown)
        .tone(Tone::Dim)
        .tooltip(move || {
            column((note(purpose.clone()),)).max_width(px(320.0))
        })
        .boxed()
}

pub(super) fn dim(text: impl Into<String>) -> impl View {
    label(text.into()).tone(Tone::Dim)
}

/// What `agent` is called in the path.
fn called(world: &World, agent: Entity) -> String {
    if world.get::<Kind>(agent).map(|kind| kind.0)
        == Some(protocol::AgentKind::Hand)
    {
        return "The Hand".into();
    }
    shown_of(world, agent)
}

/// The steps above `agent` in the chain of command.
fn above(world: &World, agent: Entity) -> Vec<AnyView> {
    let hamlet = world.get::<LivesIn>(agent).map(|l| l.0);
    let mut parts = vec![
        crumb("Realm", |world| show(world, Showing::Realm)).boxed(),
        crumb_slash().boxed(),
    ];
    if let Some(hamlet) = hamlet {
        parts.push(
            crumb(name_of(world, hamlet), move |world| {
                select(world, Some(Thing::Common(hamlet)));
                show(world, Showing::Hamlet(hamlet));
            })
            .boxed(),
        );
        parts.push(crumb_slash().boxed());
    }
    let mut parents = Vec::new();
    let mut at = agent;
    while let Some(parent) =
        world.get::<Parent>(at).and_then(|parent| {
            hamlet_client::realm::agent(world, parent.0)
        })
    {
        let same = world.get::<LivesIn>(parent).map(|l| l.0);
        if parents.contains(&parent)
            || hamlet.is_none()
            || same != hamlet
        {
            break;
        }
        parents.insert(0, parent);
        at = parent;
    }
    for ancestor in parents {
        parts.push(
            crumb(called(world, ancestor), move |world| {
                crate::convo::talk_to(world, ancestor);
            })
            .boxed(),
        );
        parts.push(crumb_slash().boxed());
    }
    parts
}

/// The path to the thread, then the page `under` it, if any.
fn path(agent: Entity, under: Option<&'static str>) -> impl View {
    built(move |cx| {
        let world = &*cx.world;
        let mut parts = above(world, agent);
        let name = called(world, agent);
        parts.push(match under {
            None => crate::naming::name_crumb(agent, name),
            Some(_) => {
                crumb(name, |world| show(world, Showing::Thread))
                    .boxed()
            }
        });
        if under.is_none()
            && let Some(tag) = ProfileTag::of(world, agent)
        {
            let profile = tag.profile;
            parts.push(
                label("of").wrap(false).rules(note_rules).boxed(),
            );
            parts.push(
                crumb(
                    derived(move |world: &World| {
                        world
                            .get::<Profile>(profile)
                            .map(|s| s.0.name.clone())
                            .unwrap_or_default()
                    })
                    .when(every_frame()),
                    move |world| super::open_profile(world, profile),
                )
                .boxed(),
            );
        }
        if let Some(page) = under {
            parts.push(crumb_slash().boxed());
            parts.push(crumb_here(page).boxed());
        }
        cx.build(crumbs(parts))
    })
}

/// Its state, by its name, and what it works on while it works.
fn state(agent: Entity) -> impl View {
    let standing = move |world: &World| Standing::of(world, agent);
    let mark = keyed(rolled(standing), |standing| {
        status_icon(
            standing.glyph(),
            standing.word(),
            standing.kind(),
        )
        .boxed()
    })
    .within(row(()).gap(0.0));
    let doing = label(rolled(move |world| {
        if standing(world) != Standing::Works {
            return String::new();
        }
        world
            .get::<Activity>(agent)
            .map(|activity| activity.words.clone())
            .unwrap_or_default()
    }))
    .tone(Tone::Dim);
    // The mark of what it learned that its profile lacks.
    let learned = keyed(
        rolled(move |world| {
            ProfileTag::of(world, agent)
                .filter(|tag| tag.learned)
                .map(|tag| learned_words(&tag.name))
        }),
        |words| match words {
            Some(words) => {
                let words = words.clone();
                row((learned_mark(),))
                    .padding(UiRect::horizontal(px(4.0)))
                    .tooltip(move || note(words.clone()))
                    .boxed()
            }
            None => frame().boxed(),
        },
    )
    .within(row(()).gap(0.0));
    row((
        mark,
        learned,
        column((one_line(doing),)).min_width(px(0.0)),
    ))
    .gap(4.0)
    .align(AlignItems::Center)
    .min_width(px(0.0))
}

/// The common actions on `agent` as buttons, the rest in a menu.
fn quick(world: &World, agent: Entity) -> Vec<AnyView> {
    let kind = world.get::<Kind>(agent).map(|kind| kind.0);
    let living = world.get::<Left>(agent).is_none();
    let mut buttons = Vec::new();
    if living
        && kind == Some(protocol::AgentKind::Hamlet)
        && hamlet_client::realm::Actions::is_allowed(
            world,
            "add-agent",
        )
    {
        buttons.push(icon_button(
            Glyph::Plus,
            "Add Agent Under It",
            move |world| world_menu::ask_add_agent(world, agent),
        ));
    }
    buttons
}

fn header(agent: Entity) -> impl View {
    built(move |cx| {
        // Gone while empty.
        let thing = if cx.world.get::<Kind>(agent).map(|kind| kind.0)
            == Some(protocol::AgentKind::Hand)
        {
            Thing::Hand
        } else {
            Thing::Agent(agent)
        };
        let more = keyed(
            derived(move |world: &World| {
                world_menu::has_items(world, thing)
            })
            .when(every_frame()),
            move |&any| {
                if !any {
                    return frame().boxed();
                }
                more_button(
                    "Adding agents, channels, merging and ending it",
                    move |world| {
                        world_menu::open_for(world, thing);
                    },
                )
                .boxed()
            },
        )
        .within(bare());
        let quick = quick(cx.world, agent);
        let address = name_of(cx.world, agent);
        let space = &cx.theme().space;
        let (gap, section) = (space.gap, space.section);
        cx.build(
            column((
                row((
                    path(agent, None)
                        .tooltip(move || note(address.clone())),
                    state(agent),
                ))
                .gap(section)
                .align(AlignItems::Center)
                .min_width(px(0.0)),
                row((
                    column((tabs::strip(
                        &SHEETS,
                        Spread::Packed,
                        |world, sheet| {
                            *world.resource_mut::<Sheet>() = sheet;
                        },
                    ),))
                    .grow(1.0)
                    .min_width(px(0.0)),
                    row(quick)
                        .gap(gap / 2.0)
                        .align(AlignItems::Center),
                    more,
                ))
                .gap(gap)
                .align(AlignItems::Center),
            ))
            .gap(section),
        )
    })
}

/// On the chip of the model of the agent it holds.
#[derive(Component, Clone, Copy)]
struct ModelChip(Entity);

/// On the chip of the model while its popup is open.
#[derive(Component)]
struct ChipOpen;

/// The chip of the model, effort and tier, which opens their popup.
fn model_chip(agent: Entity) -> impl View {
    button(
        row((world_menu::chip(agent), glyph(Glyph::Chevron, 12.0)))
            .gap(6.0)
            .align(AlignItems::Center)
            .min_width(px(0.0)),
    )
    .min_width(px(0.0))
    .max_width(percent(100.0))
    .overflow(Overflow::clip_x())
    .padding(UiRect::axes(px(8.0), px(0.0)))
    .rules(chip_style)
    .tagged(Variant::Text)
    .tagged(world_menu::Hook::Chip)
    .tagged(ModelChip(agent))
    .on_activate(move |world| {
        world_menu::ask_model(world, agent);
    })
}

/// Marks the chip of the model whose popup is open.
fn mark_open_chips(
    open: Option<Res<world_menu::Open>>,
    chips: Query<(Entity, &ModelChip, Has<ChipOpen>)>,
    mut commands: Commands,
) {
    for (chip, of, marked) in &chips {
        let wanted = open.as_deref().is_some_and(|open| {
            *open == world_menu::Open::Model(of.0)
        });
        if wanted && !marked {
            commands.entity(chip).try_insert(ChipOpen);
        } else if !wanted && marked {
            commands.entity(chip).try_remove::<ChipOpen>();
        }
    }
}

/// The style of a chip of the composer, picked while [`ChipOpen`].
fn chip_style(cx: &mut Cx) {
    choice(false)(cx);
    style::<HamletTheme>()
        .frame(|f, t| {
            f.fill(t.palette.button)
                .border_color(transparent(t.palette.border))
        })
        .when::<ChipOpen>(|s| {
            s.frame(|f, t| {
                f.fill(t.palette.selected)
                    .border_color(t.palette.gold_4)
            })
            .tone(Tone::Body)
        })
        .apply(cx);
}

/// The hamlet `agent` lives in and how its work is overseen.
fn mode_of(
    world: &World,
    agent: Entity,
) -> Option<(Entity, hamlet_client::realm::Mode)> {
    let lives = world.get::<LivesIn>(agent)?.0;
    Some((lives, world.get::<Works>(lives)?.mode))
}

/// The words of the hamlet's mode `agent` works under.
fn mode_words(world: &World, agent: Entity) -> String {
    match mode_of(world, agent).map(|at| at.1) {
        Some(hamlet_client::realm::Mode::Auto) => words::NO_ASKING,
        _ => words::ASK_FIRST,
    }
    .to_string()
}

/// On the words of the mode's chip in the composer's toolbar.
#[derive(Component)]
struct ModeWords;

/// The chip of the hamlet's mode, which a press turns over.
fn mode_chip(agent: Entity, bar: Entity) -> impl View {
    keyed(
        derived(move |world: &World| {
            mode_of(world, agent).map(|at| at.0)
        })
        .when(every_frame()),
        move |hamlet| {
            let Some(hamlet) = *hamlet else {
                return frame().boxed();
            };
            let shown = keyed(
                component::<Toolbar, _>(bar, |toolbar| {
                    toolbar.is_none_or(|toolbar| {
                        toolbar.fold != Fold::Tight
                    })
                }),
                move |&shown| match shown {
                    true => row((label(
                        derived(move |world: &World| {
                            mode_words(world, agent)
                        })
                        .when(every_frame()),
                    )
                    .wrap(false),))
                    .padding(UiRect::left(px(6.0)))
                    .tagged(ModeWords)
                    .boxed(),
                    false => frame().boxed(),
                },
            )
            .within(row(()).gap(0.0));
            button(
                row((glyph(Glyph::Shield, 12.0), shown))
                    .gap(0.0)
                    .align(AlignItems::Center),
            )
            .padding(UiRect::axes(px(8.0), px(0.0)))
            .rules(chip_style)
            .tagged(Variant::Text)
            .tooltip(move || {
                note(
                    derived(move |world: &World| {
                        let tight = world
                            .get::<Toolbar>(bar)
                            .is_some_and(|t| t.fold == Fold::Tight);
                        let mode = match tight {
                            true => format!(
                                "{}. ",
                                mode_words(world, agent)
                            ),
                            false => String::new(),
                        };
                        format!(
                            "{mode}{} Press to switch.",
                            words::APPROVALS
                        )
                    })
                    .when(every_frame()),
                )
            })
            .on_activate(move |world| {
                let to = match mode_of(world, agent).map(|at| at.1) {
                    Some(hamlet_client::realm::Mode::Auto) => {
                        "manual"
                    }
                    _ => "auto",
                };
                tell(world, hamlet, format!("/mode {to}"));
            })
            .boxed()
        },
    )
    .within(bare())
}

/// The side of the round send and stop buttons.
const ROUND: f32 = 30.0;

/// On the round button that stops the agent.
#[derive(Component)]
struct Stopping;

/// Send, or Stop while the agent works.
fn send_or_stop(agent: Entity) -> impl View {
    keyed(
        derived(move |world: &World| {
            world.get::<Busy>(agent).is_some()
        })
        .when(every_frame()),
        move |busy| {
            let round = |face: AnyView| {
                button(face)
                    .width(px(ROUND))
                    .height(px(ROUND))
                    .padding(UiRect::all(px(0.0)))
                    .radius(ROUND / 2.0)
            };
            if *busy {
                let square = with_cx(|cx| {
                    frame()
                        .width(px(10.0))
                        .height(px(10.0))
                        .radius(2.0)
                        .fill(cx.theme().palette.red)
                });
                round(square.boxed())
                    .variant(Variant::Danger)
                    .tagged(Stopping)
                    .tip("Stop")
                    .on_activate(move |world| {
                        let request = world_menu::stop(world, agent);
                        chat::replied(world, request);
                    })
                    .boxed()
            } else {
                round(glyph(Glyph::Send, 16.0).boxed())
                    .variant(Variant::Primary)
                    .tip("Send")
                    .on_activate(chat::send_typed)
                    .boxed()
            }
        },
    )
    .within(bare())
}

/// The button that compacts the agent's conversation.
fn compact(agent: Entity) -> impl View {
    keyed(
        derived(chat::without_model).when(every_frame()),
        move |&none| {
            if none {
                return frame().boxed();
            }
            chat::tool_button(
                Glyph::Notes,
                "Compact now: summarize the earlier messages",
                move |world| compact_now(world, agent),
            )
        },
    )
    .within(bare())
}

/// Summarizes the earlier part of `agent`'s conversation now.
fn compact_now(world: &mut World, agent: Entity) {
    if let Some(to) = id_of(world, agent) {
        world.trigger(Compact {
            request: protocol::RequestId::new(),
            to,
        });
    }
}

/// The least width of the model's name before the toolbar folds.
const LEAST_NAME: f32 = 64.0;

/// The spare room that brings a folded part of the toolbar back.
const SLACK: f32 = 8.0;

/// On the composer's toolbar: what of it is folded away to fit it.
#[derive(Component, Default)]
struct Toolbar {
    fold: Fold,
    /// The room the mode's words take, as last shown.
    mode_words: f32,
}

/// What of the composer's toolbar is folded away to fit it.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
enum Fold {
    #[default]
    Nothing,
    /// The icon buttons stand in one "More" button.
    Icons,
    /// The mode's chip shows its icon alone too.
    Tight,
}

/// What the toolbar's icon buttons do, as "More" offers them.
#[derive(Clone, Copy)]
enum ToolbarAction {
    Images,
    Files,
    Folder,
    Compact,
}

impl ToolbarAction {
    fn words(self) -> &'static str {
        match self {
            ToolbarAction::Images => "Attach Images",
            ToolbarAction::Files => "Attach Files",
            ToolbarAction::Folder => "Attach a Folder",
            ToolbarAction::Compact => "Compact Now",
        }
    }

    fn run(self, world: &mut World, agent: Entity) {
        match self {
            ToolbarAction::Images => {
                chat::attach::choose(world, What::Images);
            }
            ToolbarAction::Files => {
                chat::attach::choose(world, What::Files);
            }
            ToolbarAction::Folder => {
                chat::attach::choose(world, What::Folders);
            }
            ToolbarAction::Compact => compact_now(world, agent),
        }
    }
}

/// The "More" button that holds the toolbar's icon buttons.
fn more(agent: Entity) -> impl View {
    keyed(
        derived(|world: &World| {
            (
                chat::attach::sees_images(world),
                !chat::without_model(world),
            )
        })
        .when(every_frame()),
        move |&(sees, model)| {
            let actions = [
                (sees, ToolbarAction::Images),
                (true, ToolbarAction::Files),
                (true, ToolbarAction::Folder),
                (model, ToolbarAction::Compact),
            ]
            .into_iter()
            .filter_map(|(shown, action)| shown.then_some(action))
            .collect::<Vec<_>>();
            let words = actions.iter().map(|action| action.words());
            let words = words.collect::<Vec<_>>();
            with_cx(move |cx| {
                let side = cx.theme().space.row;
                let actions = actions.clone();
                menu_button("", words.clone(), move |world, at| {
                    if let Some(action) = actions.get(at) {
                        action.run(world, agent);
                    }
                })
                .icon(Glyph::More.image(ICON))
                .width(px(side))
                .height(px(side))
                .padding(UiRect::all(px(0.0)))
                .radius(side / 2.0)
                .tooltip(|| note("More"))
            })
            .boxed()
        },
    )
    .within(bare())
}

/// Folds the composer's toolbar until the model's chip fits.
fn crowd(
    theme: Themed,
    mut toolbars: Query<(&mut Toolbar, &ComputedNode, &Children)>,
    rows: Query<&Children>,
    mut nodes: Query<(&ComputedNode, &mut Node), Without<ChipName>>,
    names: Query<(Entity, &ComputedNode, &Node), With<ChipName>>,
    words: Query<(Entity, &ComputedNode), With<ModeWords>>,
    parents: Query<&ChildOf>,
) {
    let width = |node: &ComputedNode| {
        node.size().x * node.inverse_scale_factor()
    };
    let gap = chat::TOOL_GAP * theme.space.gap;
    let icons = 3.0 * (theme.space.row + gap);
    for (mut toolbar, node, inner) in &mut toolbars {
        let room = width(node);
        let Some(parts) =
            inner.first().and_then(|row| rows.get(*row).ok())
        else {
            continue;
        };
        let [chip, mode, spacer, ..] = parts[..] else {
            continue;
        };
        if room <= 0.0 {
            continue;
        }
        let of = |part: Entity| {
            nodes.get(part).map_or(0.0, |(node, _)| width(node))
        };
        let others = parts
            .iter()
            .filter(|part| ![chip, spacer].contains(part))
            .map(|part| of(*part))
            .sum::<f32>()
            + gap * (parts.len() - 1) as f32;
        let (now, whole) = names
            .iter()
            .find(|(name, ..)| {
                parents
                    .iter_ancestors(*name)
                    .any(|above| above == chip)
            })
            .map_or((0.0, 0.0), |(_, shown, ui)| {
                let now = width(shown);
                match ui.width {
                    Val::Px(whole) => (now, whole),
                    _ => (now, now),
                }
            });
        if let Some((_, shown)) = words.iter().find(|(words, _)| {
            parents.iter_ancestors(*words).any(|above| above == mode)
        }) && toolbar.mode_words != width(shown)
        {
            toolbar.mode_words = width(shown);
        }
        // The chip with its name cut to its least.
        let least = of(chip) - now + whole.min(LEAST_NAME);
        let left = room - others;
        let short = left < least - 0.5;
        // Spare room keeps a toolbar at the edge from flickering.
        let fits = |more: f32| left - more >= least + SLACK;
        let fold = match toolbar.fold {
            Fold::Nothing if short => Fold::Icons,
            Fold::Icons if short => Fold::Tight,
            Fold::Icons if fits(icons) => Fold::Nothing,
            Fold::Tight if fits(toolbar.mode_words) => Fold::Icons,
            fold => fold,
        };
        if toolbar.fold != fold {
            toolbar.fold = fold;
        }
        let most = Val::Px(match fold {
            Fold::Tight => left.max(0.0),
            Fold::Nothing | Fold::Icons => left.max(least),
        });
        if let Ok((_, mut ui)) = nodes.get_mut(chip)
            && ui.max_width != most
        {
            ui.max_width = most;
        }
    }
}

/// The composer's toolbar, under its field.
fn chips(agent: Entity) -> impl View {
    AnyView::new(move |cx: &mut Cx| {
        let gap = chat::TOOL_GAP * cx.theme().space.gap;
        let bar = cx.build(
            bare()
                .width(percent(100.0))
                .min_width(px(0.0))
                .tagged(Toolbar::default()),
        );
        let tools = keyed(
            component::<Toolbar, _>(bar, |toolbar| {
                toolbar.is_some_and(|toolbar| {
                    toolbar.fold != Fold::Nothing
                })
            }),
            move |&folded| match folded {
                true => more(agent).boxed(),
                false => {
                    row((chat::attach::control(), compact(agent)))
                        .gap(gap)
                        .align(AlignItems::Center)
                        .boxed()
                }
            },
        )
        .within(bare().shrink(0.0));
        cx.under(bar, |cx| {
            cx.build(
                row((
                    model_or_setup(agent),
                    row((mode_chip(agent, bar),)).shrink(0.0),
                    frame().grow(1.0),
                    tools,
                    row((send_or_stop(agent),)).shrink(0.0),
                ))
                .gap(gap)
                .align(AlignItems::Center)
                .width(percent(100.0))
                .min_width(px(0.0))
                .overflow(Overflow::clip_x()),
            )
        });
        bar
    })
}

/// The chip of the agent's model, or the way to set one up.
fn model_or_setup(agent: Entity) -> impl View {
    keyed(
        derived(chat::without_model).when(every_frame()),
        move |&none| {
            if !none {
                return model_chip(agent).boxed();
            }
            button(
                row((
                    glyph(Glyph::Chip, 12.0).tone(Tone::Critical),
                    label("Set Up a Model").wrap(false),
                ))
                .gap(6.0)
                .align(AlignItems::Center),
            )
            .padding(UiRect::axes(px(8.0), px(0.0)))
            .rules(chip_style)
            .tagged(Variant::Text)
            .tooltip(|| {
                note("No model is set up yet, so nothing can answer")
            })
            .on_activate(crate::welcome::again)
            .boxed()
        },
    )
    .within(bare().shrink(0.0))
}

/// The chat shown and the agent it is with.
fn seated() -> Signal<(Entity, Entity)> {
    resource::<Convo, _>(|convo| (convo.chat, convo.agent))
}

/// The transcript, what waits on the chief, and the composer.
fn talk() -> AnyView {
    let bar = keyed(seated(), |&(_, agent)| {
        chat::message_bar(chips(agent)).boxed()
    })
    .within(bare());
    with_cx(move |cx| {
        let space = &cx.theme().space;
        let (gap, widest) = (space.gap, px(space.thread_width));
        column((chat::transcript(), wait::in_chat(), bar))
            .fills_height()
            .gap(gap)
            .width(percent(100.0))
            .max_width(widest)
            .margin(UiRect::horizontal(Val::Auto))
    })
    .boxed()
}

/// The thread of the agent the chat is with.
pub(super) fn living() -> impl View {
    let about = keyed(seated(), |&(_, agent)| header(agent).boxed())
        .within(bare());
    let body = keyed(
        resource::<Sheet, _>(|sheet| *sheet == Sheet::Details),
        |open| match open {
            true => keyed(seated(), |&(_, agent)| {
                details::sheet(agent).boxed()
            })
            .within(filling())
            .boxed(),
            false => talk(),
        },
    )
    .within(filling());
    with_cx(move |cx| {
        column((about, body))
            .fills_height()
            .gap(cx.theme().space.gap)
    })
}

/// Reads the chat agent's prompt and turns the page to it.
pub(crate) fn show_prompt(world: &mut World) {
    let convo = world.resource::<Convo>();
    let agent = convo.agent;
    *world.resource_mut::<Briefing>() = Briefing {
        agent: Some(agent),
        ..Briefing::default()
    };
    super::papers::prompt(world);
    show(world, Showing::Prompt);
}

/// Leaves the prompt of an agent the chat is no longer with.
pub(super) fn leave_prompt(
    convo: Res<Convo>,
    briefing: Res<Briefing>,
    mut showing: ResMut<Showing>,
) {
    if *showing == Showing::Prompt
        && briefing.agent != Some(convo.agent)
    {
        *showing = Showing::Thread;
    }
}

/// The prompt's page about the agent the chat is with.
fn aside(body: impl View + 'static) -> impl View {
    let head =
        keyed(resource::<Convo, _>(|convo| convo.agent), |agent| {
            let name = with_cx(|cx| {
                row((title("Prompt"), frame().grow(1.0), copy()))
                    .gap(cx.theme().space.gap)
                    .align(AlignItems::Center)
                    .width(percent(100.0))
            });
            page_head(path(*agent, Some("Prompt")), name).boxed()
        })
        .within(bare());
    with_cx(move |cx| {
        column((head, body))
            .fills_height()
            .gap(cx.theme().space.section)
    })
}

/// What heads each section of a prompt, by the server's names.
const SOURCES: [(&str, &str); 10] = [
    ("charter", "Charter"),
    ("machine", "This computer"),
    ("project", "The project"),
    ("character", "Character"),
    ("memory", "Memory"),
    ("skills", "Skills"),
    ("place", "Its place"),
    ("realm", "The realm"),
    ("profiles", "Profiles it may add agents of"),
    ("errand", "Errand"),
];

/// What heads the section `name`.
fn source(name: &str) -> &str {
    SOURCES
        .iter()
        .find(|(known, _)| *known == name)
        .map_or(name, |(_, heading)| heading)
}

/// Puts the whole prompt on the clipboard.
fn copy_prompt(world: &mut World) {
    let text = world.resource::<Briefing>().whole();
    let Some(mut clipboard) =
        world.get_resource_mut::<bevy::clipboard::Clipboard>()
    else {
        return;
    };
    match clipboard.set_text(text) {
        Ok(()) => world.resource_mut::<Briefing>().copied = true,
        Err(e) => {
            bevy::log::warn!("could not copy the prompt: {e:?}");
        }
    }
}

/// The prompt's sections under headings, or why it was not read.
fn prompt_sections(
    prompt: &[protocol::PromptSection],
    failed: Option<&str>,
) -> AnyView {
    if let Some(why) = failed {
        return label(format!("The prompt could not be read: {why}"))
            .tone(Tone::Critical)
            .boxed();
    }
    let sections = prompt
        .iter()
        .map(|section| {
            let name = source(&section.name).to_string();
            // Not Markdown, which would run the lines together.
            let text = hamlet_ui::markdown::page(
                &section.text.replace('\n', "  \n"),
                Tone::Body,
            )
            .boxed();
            column((card_name(name, Tone::Body), text))
                .gap(6.0)
                .boxed()
        })
        .collect::<Vec<_>>();
    // Clear of the scroll bar, which is drawn over the right edge.
    with_cx(move |cx| {
        let line = cx.theme().palette.gold_6;
        let gap = cx.theme().space.panel;
        let mut parts = Vec::new();
        for section in sections {
            if !parts.is_empty() {
                parts.push(hairline(line).boxed());
            }
            parts.push(section);
        }
        let page = faded(
            scroll((column(parts).gap(gap),))
                .overflow(Overflow::scroll_x())
                .width(percent(100.0)),
        );
        column((card((page,)),))
            .padding(UiRect::right(px(12.0)))
            .min_width(px(0.0))
    })
    .boxed()
}

/// The way to copy the prompt, once it is read.
fn copy() -> impl View {
    keyed(
        resource::<Briefing, _>(|b| (b.prompt.is_empty(), b.copied)),
        |(empty, copied)| match (empty, copied) {
            (true, _) => frame().boxed(),
            (false, true) => {
                action(Glyph::Check, "Copied", copy_prompt)
            }
            (false, false) => {
                action(Glyph::Notes, "Copy", copy_prompt)
            }
        },
    )
    .within(bare())
}

/// The whole of the agent's prompt, part by part.
pub(super) fn prompt_page() -> impl View {
    let shown = keyed(
        resource::<Briefing, _>(|b| {
            (b.prompt.clone(), b.failed.clone())
        }),
        |(prompt, failed)| prompt_sections(prompt, failed.as_deref()),
    )
    .within(bare());
    aside(scroll_area((shown,)))
}

#[cfg(test)]
mod tests {
    use bevy::app::App;
    use hamlet_client::outbox::Outbox;

    use super::*;
    use crate::testing::{panel_app, press, words};

    fn hub_app() -> App {
        let mut app = panel_app();
        app.add_plugins((
            chat::plugin,
            crate::hub::plugin,
            wait::plugin,
        ));
        app.init_resource::<crate::status::Bar>();
        app
    }

    fn ada_app() -> App {
        let mut app = hub_app();
        let ada = crate::testing::seat_ada(&mut app);
        app.insert_resource(hamlet_realm::pick::Selected(Some(
            Thing::Agent(ada),
        )))
        .insert_resource(Showing::Thread)
        .insert_resource(Sheet::Details);
        app.update();
        bevy_fynix::mount::<HamletTheme>(
            app.world_mut(),
            crate::hub::screen(),
        );
        app.update();
        app
    }

    fn request(app: &App, what: &str) -> serde_json::Value {
        app.world()
            .resource::<Outbox>()
            .iter()
            .map(|message| serde_json::to_value(message).unwrap())
            .filter(|message| message["what"] == what)
            .last()
            .unwrap_or_else(|| panic!("no read {what}"))
    }

    fn has_span(app: &mut App, text: &str) -> bool {
        app.world_mut()
            .query::<&bevy::text::TextSpan>()
            .iter(app.world())
            .any(|span| span.0 == text)
    }

    fn receive(app: &mut App, message: serde_json::Value) {
        hamlet_client::receive(
            app.world_mut(),
            serde_json::from_value(message).unwrap(),
        );
        app.update();
    }

    #[test]
    fn edits_of_the_character_and_of_a_memory_file_return_to_where_they_began()
     {
        use bevy::ecs::observer::On;
        use bevy::ecs::resource::Resource;
        use bevy::ecs::system::ResMut;
        use hamlet_desktop::editor::{Edit, Edited};
        use serde_json::json;
        #[derive(Resource, Default)]
        struct Edits(Vec<Edit>);
        let mut app = ada_app();
        app.init_resource::<Edits>().add_observer(
            |edit: On<Edit>, mut edits: ResMut<Edits>| {
                edits.0.push(edit.event().clone());
            },
        );
        let papers = request(&app, "papers");
        receive(
            &mut app,
            json!({"type": "read", "to": papers["id"], "papers": {"memory": [{"path": "farm/pears.md", "description": "", "text": "Pears"}], "learned": [], "character": "", "skills": []}}),
        );
        press(&mut app, "Edit");
        let read = request(&app, "papers");
        assert_eq!(read["full"], true);
        let text = "character ".repeat(1000);
        receive(
            &mut app,
            json!({"type": "read", "to": read["id"], "papers": {"memory": [], "learned": [], "character": text, "skills": []}}),
        );
        let edit =
            app.world().resource::<Edits>().0.last().unwrap().clone();
        assert_eq!(edit.text, text);
        app.world_mut().trigger(Edited {
            key: edit.key.clone(),
            text: "Saved character".into(),
        });
        app.update();
        let sent = app
            .world()
            .resource::<Outbox>()
            .iter()
            .map(|m| serde_json::to_value(m).unwrap())
            .find(|m| m["type"] == "edit")
            .unwrap();
        assert_eq!(sent["doc"], "character");
        assert_eq!(sent["profile"], json!(2));
        assert_eq!(sent["text"], "Saved character");
        receive(
            &mut app,
            json!({"type": "reply", "to": sent["id"], "kind": "error", "text": "Character rejected"}),
        );
        assert!(
            words(&mut app)
                .iter()
                .any(|text| text == "Character rejected")
        );
        // A memory file is removed by sending no text.
        super::super::papers::edit_memory(
            app.world_mut(),
            "farm/pears.md".into(),
        );
        let read = request(&app, "papers");
        receive(
            &mut app,
            json!({"type": "read", "to": read["id"], "papers": {"memory": [{"path": "farm/pears.md", "description": "", "text": "Whole pears"}], "learned": [], "character": "", "skills": []}}),
        );
        let edit =
            app.world().resource::<Edits>().0.last().unwrap().clone();
        assert_eq!(edit.text, "Whole pears");
        app.world_mut().trigger(Edited {
            key: edit.key,
            text: "Ripe pears".into(),
        });
        app.update();
        let sent = app
            .world()
            .resource::<Outbox>()
            .iter()
            .map(|m| serde_json::to_value(m).unwrap())
            .filter(|m| m["doc"] == "memory")
            .last()
            .unwrap();
        assert_eq!(sent["of"], json!({"agent": 2}));
        assert_eq!(sent["path"], "farm/pears.md");
        assert_eq!(sent["text"], "Ripe pears");
        super::super::papers::remove_memory(
            app.world_mut(),
            "farm/pears.md".into(),
        );
        let removal = app
            .world()
            .resource::<Outbox>()
            .iter()
            .map(|m| serde_json::to_value(m).unwrap())
            .filter(|m| m["doc"] == "memory")
            .last()
            .unwrap();
        assert_eq!(removal["text"], "");
    }

    fn select_scout(app: &mut App) {
        use hamlet_client::chats::Chat;
        let mut state = hamlet_testing::realm::state();
        state
            .agents
            .push(hamlet_testing::realm::agent(3, "home", false));
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::State(state),
        );
        let agent =
            hamlet_client::realm::named(app.world_mut(), "agent#3")
                .unwrap();
        let to = protocol::AgentId(3);
        let chat = app
            .world_mut()
            .spawn(Chat(protocol::ChatRef { with: to }))
            .id();
        let mut convo = app.world_mut().resource_mut::<Convo>();
        convo.agent = agent;
        convo.chat = chat;
        convo.with = Some("agent#3".into());
        app.update();
    }

    #[test]
    fn late_reads_cannot_fill_the_next_agents_pages() {
        use serde_json::json;
        let mut app = ada_app();
        let papers = request(&app, "papers");
        show_prompt(app.world_mut());
        let prompt = request(&app, "prompt");
        select_scout(&mut app);
        let scout = request(&app, "papers");
        assert_eq!(scout["of"], json!({"agent": 3}));
        receive(
            &mut app,
            json!({"type": "read", "to": scout["id"], "papers": {"memory": [{"path": "notes.md", "description": "", "text": "Scout notes"}], "learned": [], "character": "Scout character", "skills": []}}),
        );
        receive(
            &mut app,
            json!({"type": "read", "to": papers["id"], "papers": {"memory": [{"path": "notes.md", "description": "", "text": "Wrong notes"}], "learned": [], "character": "Wrong character", "skills": []}}),
        );
        receive(
            &mut app,
            json!({"type": "read", "to": prompt["id"], "prompt": [{"name": "charter", "text": "Wrong prompt"}]}),
        );
        // The prompt page is left for the thread of the next agent.
        assert_eq!(
            *app.world().resource::<Showing>(),
            Showing::Thread
        );
        assert!(has_span(&mut app, "Scout character"));
        assert!(!has_span(&mut app, "Wrong character"));
        press(&mut app, "notes.md");
        app.update();
        assert!(has_span(&mut app, "Scout notes"));
        assert!(!has_span(&mut app, "Wrong notes"));
        assert!(app.world().resource::<Briefing>().prompt.is_empty());
    }
}
