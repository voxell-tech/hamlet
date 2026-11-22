//! The limits of the chief's plans: how much of each signed-in
//! subscription is used and when it resets, on the Hub's realm page
//! and under each provider in Settings, with a toast when a plan
//! refuses or will run out before it resets.

mod reading;

use std::collections::HashSet;

use bevy::app::{App, PreUpdate};
use bevy::ecs::change_detection::{DetectChanges, DetectChangesMut};
use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, Local, Res, ResMut};
use bevy::ecs::world::World;
use bevy::ui::{
    AlignItems, JustifyContent, PositionType, UiRect, Val, percent,
    px,
};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, button, column, frame,
    label, row,
};
use bevy_fynix::{ViewExt, keyed, resource};
use hamlet_client::Connection;
use hamlet_client::connection::Contact;
use hamlet_client::messages::{Reconnected, Response};
use hamlet_client::outbox::{Outbox, RequestId};
use hamlet_client::realm::Providers;
use hamlet_core::panels::PanelSystems;
use hamlet_core::settings::{ENDPOINTS, OpenSettings, SettingsPane};
use hamlet_core::toasts::Toast;
use hamlet_protocol::client as protocol;
use hamlet_ui::styles::{
    ICON, action, bare, close, icon_button, note, pill,
};
use hamlet_ui::theme::StatusKind;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::{AnyView, with_cx};
use jiff::Timestamp;
use jiff::tz::TimeZone;

pub(crate) use self::reading::{CHATGPT_USAGE, Mood, Reading, Row};
use crate::models::mark;

/// The width of a window's name before its bar, of what it used after
/// it, and the gap between them.
const NAME: f32 = 116.0;
const AMOUNT: f32 = 112.0;
const GAP: f32 = 10.0;

/// The height of a window's bar, and of its pace mark's overhang.
const BAR: f32 = 6.0;
const OVERHANG: f32 = 3.0;

pub fn plugin(app: &mut App) {
    app.init_resource::<Plans>()
        .init_resource::<Asking>()
        .add_observer(answered)
        .add_observer(
            |_: On<Reconnected>, mut asking: ResMut<Asking>| {
                asking.0 = None;
            },
        )
        .add_systems(
            PreUpdate,
            (
                ask.in_set(PanelSystems::Follow),
                (follow, warn).chain().in_set(PanelSystems::Bind),
            ),
        );
}

/// Each signed-in provider that told its limits, as the chief reads
/// them now.
#[derive(Resource, Default, Clone, PartialEq)]
pub struct Plans(pub(crate) Vec<Plan>);

/// A provider's plan and how it stands.
#[derive(Clone, PartialEq)]
pub(crate) struct Plan {
    pub provider: String,
    /// Its provider's name and the plan's, as "Claude · Max".
    pub title: String,
    /// The plan's name, where the provider tells it, as "Max".
    pub name: Option<String>,
    pub reading: Reading,
}

impl Plans {
    /// The plan of `provider`, once it told its limits.
    pub(crate) fn of(&self, provider: &str) -> Option<&Plan> {
        self.0.iter().find(|plan| plan.provider == provider)
    }
}

/// The check of the limits on its way to the server.
#[derive(Resource, Default)]
struct Asking(Option<RequestId>);

/// Asks the server for the limits when Settings' endpoints page
/// opens, or the connection comes back while it is open. The server
/// reads a provider at most once a minute however often it is asked.
fn ask(
    pane: Option<Res<SettingsPane>>,
    connection: Res<Connection>,
    mut commands: Commands,
) {
    let moved = connection.is_changed()
        || pane.as_ref().is_some_and(DetectChanges::is_changed);
    let shown = pane.is_some_and(|pane| pane.0 == Some(ENDPOINTS));
    if moved && shown {
        commands.queue(check);
    }
}

/// Asks the server to read each signed-in provider's limits again.
pub fn check(world: &mut World) {
    if world.resource::<Connection>().state != Contact::Connected
        || world.resource::<Asking>().0.is_some()
    {
        return;
    }
    let id = world.resource_mut::<Outbox>().send(|id| {
        protocol::ClientMessage::Check(protocol::Check::Limits { id })
    });
    world.resource_mut::<Asking>().0 = Some(id);
}

fn answered(response: On<Response>, mut asking: ResMut<Asking>) {
    if asking.0.as_ref() == Some(&response.to) {
        asking.0 = None;
    }
}

/// Reads each provider's limits again when they change, and as the
/// minutes pass, since a reset's words count down. A window that
/// reset since its limits were told asks the server for them again,
/// so that a used up one does not stay so.
fn follow(
    providers: Option<Res<Providers>>,
    mut plans: ResMut<Plans>,
    mut minute: Local<i64>,
    mut commands: Commands,
) {
    let now = Timestamp::now().as_millisecond();
    let changed =
        providers.as_ref().is_some_and(DetectChanges::is_changed);
    if !changed && now / 60_000 == *minute {
        return;
    }
    *minute = now / 60_000;
    let zone = TimeZone::system();
    let read = providers
        .iter()
        .flat_map(|providers| providers.0.iter())
        .filter_map(|provider| {
            let limits = provider.limits.as_ref()?;
            let reading =
                reading::read(&provider.id, limits, now, &zone);
            let title = match &limits.plan {
                Some(plan) => format!("{} · {plan}", provider.name),
                None => provider.name.clone(),
            };
            Some(Plan {
                provider: provider.id.clone(),
                title,
                name: limits.plan.clone(),
                reading,
            })
        })
        .collect::<Vec<_>>();
    if read.iter().any(|plan| plan.reading.due) {
        commands.queue(check);
    }
    plans.set_if_neq(Plans(read));
}

/// Raises a toast when a plan starts refusing, and when its pace
/// first says a window will run out before it resets: each once a
/// window, until it resets. What stood when the window began is not
/// new.
fn warn(
    plans: Res<Plans>,
    mut commands: Commands,
    mut told: Local<HashSet<(String, String, i64)>>,
    mut primed: Local<bool>,
) {
    if !plans.is_changed() {
        return;
    }
    for plan in &plans.0 {
        let reading = &plan.reading;
        let near =
            reading.nearest.and_then(|at| reading.rows.get(at));
        let key = near.map_or((String::new(), 0), |row| {
            (row.id.clone(), row.resets.unwrap_or(0))
        });
        let (kind, head) = match reading.mood {
            Mood::Out => (
                StatusKind::Danger,
                format!("{}: limit reached", plan.title),
            ),
            Mood::Low
                if near.is_some_and(|r| r.runs_out.is_some()) =>
            {
                (
                    StatusKind::Attention,
                    format!("{}: may run out", plan.title),
                )
            }
            Mood::Low => (
                StatusKind::Attention,
                format!("{}: running low", plan.title),
            ),
            Mood::Unknown | Mood::Plenty | Mood::Fair => continue,
        };
        // Running low and then used up are each told once.
        let key = (
            format!("{}/{:?}", plan.provider, reading.mood),
            key.0,
            key.1,
        );
        if told.insert(key) && *primed {
            commands.trigger(Toast {
                kind,
                head,
                line: reading.sentence.clone(),
            });
        }
    }
    *primed = true;
}

/// Opens `provider`'s limits under it in Settings' endpoints.
pub(crate) fn open(world: &mut World) {
    world.trigger(OpenSettings(Some(ENDPOINTS)));
}

/// The colour of `mood`: green with plenty left, yellow with some,
/// orange when low, red when used up, and none while unknown.
pub(crate) fn status_of(mood: Mood) -> Option<StatusKind> {
    match mood {
        Mood::Unknown => None,
        Mood::Plenty => Some(StatusKind::Success),
        Mood::Fair => Some(StatusKind::Warning),
        Mood::Low => Some(StatusKind::Attention),
        Mood::Out => Some(StatusKind::Danger),
    }
}

/// The status of a plan that needs the chief: low or used up.
pub(crate) fn alert_of(mood: Mood) -> Option<StatusKind> {
    status_of(mood).filter(|_| mood >= Mood::Low)
}

/// How much of a window is left, as a bar in its colour that empties
/// as it is used, and a thin mark where it would end at an even pace.
fn bar(row_: &Row) -> AnyView {
    let (left, pace, mood) = (row_.left, row_.pace, row_.mood);
    with_cx(move |cx| {
        let palette = &cx.theme().palette;
        let fill = status_of(mood).map_or(palette.text_dim, |kind| {
            palette.status.get(kind).mark
        });
        let mut parts = vec![
            frame()
                .width(percent(left.unwrap_or(0.0) * 100.0))
                .height(percent(100.0))
                .radius(BAR / 2.0)
                .fill(fill)
                .boxed(),
        ];
        if let Some(pace) = pace {
            parts.push(
                frame()
                    .width(px(2.0))
                    .height(px(BAR + 2.0 * OVERHANG))
                    .fill(palette.text)
                    .position(PositionType::Absolute)
                    .inset(UiRect::new(
                        percent(pace * 100.0),
                        Val::Auto,
                        px(-OVERHANG),
                        Val::Auto,
                    ))
                    .tooltip(|| {
                        note(
                            "Even pace: with use spread evenly \
                             until the reset, the bar would end \
                             here. Short of it, this limit is used \
                             faster than it lasts.",
                        )
                    })
                    .boxed(),
            );
        }
        row(parts)
            .height(px(BAR))
            .grow(1.0)
            .min_width(px(48.0))
            .radius(BAR / 2.0)
            .fill(palette.hairline)
    })
    .boxed()
}

/// One window in Settings: its name, its bar and what is left, in
/// columns that line up from row to row, and under the bar when it
/// resets and where its pace takes it, unless the sentence above
/// already says so.
fn window(row_: &Row, said: bool) -> AnyView {
    let under = row_
        .reset
        .iter()
        .chain(row_.forecast.iter().filter(|_| !said))
        .cloned()
        .collect::<Vec<_>>();
    let top = row((
        row((label(row_.name.clone()).wrap(false),))
            .width(px(NAME))
            .shrink(0.0),
        if row_.left.is_some() {
            bar(row_)
        } else {
            frame().grow(1.0).boxed()
        },
        amount(row_),
    ))
    .gap(GAP)
    .align(AlignItems::Center);
    if under.is_empty() {
        return top.boxed();
    }
    close((
        top,
        row((note(under.join(" · ")),))
            .padding(UiRect::left(px(NAME + GAP))),
    ))
    .boxed()
}

/// What a window has left, right-aligned in a column of its own so
/// every bar ends at the same place, and what it used on hover.
fn amount(row_: &Row) -> AnyView {
    let tone = match row_.mood {
        Mood::Unknown => Tone::Dim,
        Mood::Out => Tone::Critical,
        _ => Tone::Body,
    };
    let shown =
        row((label(row_.amount.clone()).tone(tone).wrap(false),))
            .width(px(AMOUNT))
            .shrink(0.0)
            .justify(JustifyContent::FlexEnd);
    match row_.used.clone() {
        Some(used) => {
            shown.tooltip(move || note(used.clone())).boxed()
        }
        None => shown.boxed(),
    }
}

/// `at` as a clock time from now, in the system's zone.
fn at_clock(at: i64) -> String {
    reading::clock(
        at,
        Timestamp::now().as_millisecond(),
        &TimeZone::system(),
    )
}

/// The plan's sentence, led by a mark of its status when it runs low
/// or is used up, since orange words would not read as orange.
fn said(reading: &Reading) -> AnyView {
    let status = alert_of(reading.mood);
    let words = label(reading.sentence.clone())
        .tone(status.map_or(Tone::Body, StatusKind::words));
    match status {
        Some(kind) => row((
            glyph(Glyph::Meter, ICON).tone(kind.tone()),
            row((words,)).grow(1.0).min_width(px(0.0)),
        ))
        .gap(8.0)
        .align(AlignItems::Start)
        .boxed(),
        None => words.boxed(),
    }
}

/// A plan in full: its sentence, every window, and below them when it
/// was read and a way to read it again, or, where the provider tells
/// nothing, where to look instead.
fn detail(plan: &Plan) -> AnyView {
    let reading = &plan.reading;
    let nearest = reading.nearest;
    let mut parts = Vec::new();
    if !reading.sentence.is_empty() {
        parts.push(said(reading));
    }
    parts.extend(reading.rows.iter().enumerate().map(
        |(at, row_)| {
            // The sentence speaks of the nearest window's run-out.
            window(
                row_,
                Some(at) == nearest
                    && reading.mood == Mood::Low
                    && row_.runs_out.is_some(),
            )
        },
    ));
    let mut foot: Vec<AnyView> = Vec::new();
    let unknown =
        reading.mood == Mood::Unknown && reading.rows.is_empty();
    // A ChatGPT plan whose use could not be read is still shown on
    // ChatGPT's own page.
    if plan.provider == "chatgpt" && unknown {
        foot.push(
            action(Glyph::Into, "Open ChatGPT Usage", |_| {
                hamlet_desktop::open::browse(CHATGPT_USAGE);
            })
            .boxed(),
        );
    }
    foot.push(frame().grow(1.0).boxed());
    // A provider that tells nothing has nothing to check again; a
    // ChatGPT read that failed may succeed next time.
    if !unknown || plan.provider == "chatgpt" {
        let when = if reading.stale {
            format!("{} · may be out of date", reading.when)
        } else {
            reading.when.clone()
        };
        foot.push(note(when).boxed());
        foot.push(
            icon_button(
                Glyph::Restore,
                "Check the limits now",
                check,
            )
            .boxed(),
        );
    }
    parts.push(row(foot).gap(GAP).align(AlignItems::Center).boxed());
    column(parts)
        .gap(8.0)
        .opacity(if reading.stale { 0.6 } else { 1.0 })
        .boxed()
}

/// The limits of `provider` under its row in Settings, once it told
/// them.
pub(crate) fn of_provider(provider: String) -> AnyView {
    keyed(
        resource::<Plans, _>(move |plans| {
            plans.of(&provider).cloned()
        }),
        |plan| match plan {
            Some(plan) => detail(plan),
            None => frame().boxed(),
        },
    )
    .within(bare())
    .boxed()
}

/// The name of `provider`'s plan beside the provider's name in
/// Settings, once it told it.
pub(crate) fn plan_name(provider: String) -> AnyView {
    keyed(
        resource::<Plans, _>(move |plans| {
            plans.of(&provider).and_then(|plan| plan.name.clone())
        }),
        |name| match name {
            Some(name) => pill(name.clone()).boxed(),
            None => frame().boxed(),
        },
    )
    .within(bare())
    .boxed()
}

/// A plan at a glance, for a narrow column: its mark and name with
/// what its nearest window has left, that window's bar across the
/// whole line, and under it the window's name and when it resets or
/// runs out. Where it has no window, a few words. It opens the
/// plan's limits in Settings.
fn line(plan: &Plan) -> AnyView {
    let reading = &plan.reading;
    let near =
        reading.nearest.and_then(|at| reading.rows.get(at)).filter(
            |row_| reading.mood != Mood::Out || row_.left.is_some(),
        );
    let provider = plan.provider.clone();
    let mut head: Vec<AnyView> = vec![
        mark(&provider, ICON).boxed(),
        label(plan.title.clone()).bold(true).wrap(false).boxed(),
        frame().grow(1.0).boxed(),
    ];
    let mut body: Vec<AnyView> = Vec::new();
    match near {
        Some(row_) => {
            head.push(amount(row_));
            if row_.left.is_some() {
                body.push(bar(row_));
            }
            let when = match (row_.runs_out, row_.resets) {
                (Some(out), Some(resets)) => format!(
                    "runs out around {} at this pace, resets {}",
                    at_clock(out),
                    at_clock(resets)
                ),
                (Some(out), None) => {
                    format!(
                        "runs out around {} at this pace",
                        at_clock(out)
                    )
                }
                (None, Some(resets)) if row_.mood == Mood::Out => {
                    format!("reached, resets {}", at_clock(resets))
                }
                (None, _) => row_
                    .reset
                    .as_deref()
                    .map(|reset| {
                        reset.replacen("Resets", "resets", 1)
                    })
                    .unwrap_or_default(),
            };
            let words = if when.is_empty() {
                row_.name.clone()
            } else {
                format!("{} · {when}", row_.name)
            };
            body.push(label(words).tone(Tone::Dim).boxed());
        }
        None => {
            let tone = alert_of(reading.mood)
                .map_or(Tone::Body, StatusKind::words);
            body.push(
                label(reading.sentence.clone()).tone(tone).boxed(),
            );
        }
    }
    head.push(glyph(Glyph::Into, ICON).tone(Tone::Dim).boxed());
    let mut parts =
        vec![row(head).gap(8.0).align(AlignItems::Center).boxed()];
    parts.extend(body);
    button(
        column(parts)
            .gap(6.0)
            .width(percent(100.0))
            .padding(UiRect::all(px(8.0))),
    )
    .tooltip(|| note("Open its limits in Settings"))
    .on_activate(open)
    .boxed()
}

/// The plans at a glance, for the Hub's realm page: one line each,
/// drawn only while a subscription told its limits, with when the
/// oldest of them was read.
pub(crate) fn part() -> AnyView {
    keyed(resource::<Plans, _>(glanced), |plans| {
        if plans.is_empty() {
            return frame().boxed();
        }
        let oldest = plans
            .iter()
            .filter(|p| p.reading.mood != Mood::Unknown)
            .map(|p| p.reading.checked)
            .min();
        let lines =
            column(plans.iter().map(line).collect::<Vec<_>>())
                .gap(4.0)
                .boxed();
        with_cx(move |cx| {
            let mut title = vec![
                hamlet_ui::styles::part_title(
                    "Plans".into(),
                    Tone::Faint,
                )
                .boxed(),
                frame().grow(1.0).boxed(),
            ];
            if let Some(oldest) = oldest {
                let now = Timestamp::now().as_millisecond();
                title.push(
                    note(format!(
                        "checked {}",
                        reading::ago(now - oldest)
                    ))
                    .boxed(),
                );
            }
            title.push(
                icon_button(
                    Glyph::Restore,
                    "Check the limits now",
                    check,
                )
                .boxed(),
            );
            column((
                row(title).gap(GAP).align(AlignItems::Center),
                lines,
            ))
            .gap(cx.theme().space.gap)
        })
        .boxed()
    })
    .within(bare())
    .boxed()
}

/// The plans worth a glance: those with a window to show or a
/// refusal to tell.
fn glanced(plans: &Plans) -> Vec<Plan> {
    plans
        .0
        .iter()
        .filter(|p| {
            !p.reading.rows.is_empty() || p.reading.mood == Mood::Out
        })
        .cloned()
        .collect()
}

/// Words for a reply the plan of the chat's model refused: what was
/// used up and, where the provider says, when it lifts. None when the
/// failure was not a plan's limit.
pub(crate) fn refusal(
    world: &World,
    model: &str,
    error: &str,
) -> Option<String> {
    let provider = crate::models::provider_of(model);
    let plan = world.get_resource::<Plans>()?.of(provider)?;
    // Only the failure's own words tell it was the plan's limit: a
    // plan out of use later says nothing of a reply that failed for
    // another reason, and a context window is no plan's limit.
    let lower = error.to_lowercase();
    let says_limit = [
        "usage limit",
        "usage_limit",
        "limit reached",
        "hit your limit",
        "rate limit",
        "rate_limit",
        "quota",
        "402",
        "credits",
        "key limit",
        "spending limit",
        "extra usage",
    ]
    .iter()
    .any(|words| lower.contains(words));
    let says_context = ["context", "too long", "too many tokens"]
        .iter()
        .any(|words| lower.contains(words));
    if !says_limit || says_context {
        return None;
    }
    Some(if plan.reading.mood == Mood::Out {
        format!("{} stopped: {}", plan.title, plan.reading.sentence)
    } else {
        format!("{} stopped at a usage limit.", plan.title)
    })
}

#[cfg(test)]
mod tests {
    use bevy_fynix::mount;
    use hamlet_ui::theme::HamletTheme;
    use serde_json::json;

    use super::*;
    use crate::testing::{all_words, panel_app, sent, settle};

    /// The window on the realm's page, connected, with the provider
    /// `claude` signed in.
    fn realm_page() -> App {
        let mut app = panel_app();
        app.init_resource::<Toasted>()
            .add_observer(
                |_: On<Toast>, mut toasted: ResMut<Toasted>| {
                    toasted.0 += 1;
                },
            )
            .add_plugins(plugin)
            .add_systems(
                PreUpdate,
                crate::hub::ask_limits.in_set(PanelSystems::Follow),
            );
        app.world_mut().spawn(crate::hub::Showing::Realm);
        app.world_mut().resource_mut::<Connection>().state =
            Contact::Connected;
        app.world_mut().resource_mut::<Providers>().0 =
            vec![protocol::ProviderView {
                configured: true,
                id: "claude".into(),
                label: "Sign in to Claude".into(),
                limits: None,
                methods: vec![protocol::LoginMethod::Subscription],
                name: "Claude".into(),
            }];
        settle(&mut app);
        app
    }

    fn told(app: &mut App, limits: serde_json::Value) {
        let limits = serde_json::from_value(limits).unwrap();
        app.world_mut().resource_mut::<Providers>().0[0].limits =
            Some(limits);
        settle(app);
    }

    /// The toasts raised.
    #[derive(Resource, Default)]
    struct Toasted(usize);

    fn toasted(app: &App) -> usize {
        app.world().resource::<Toasted>().0
    }

    #[test]
    fn the_realm_page_asks_for_the_limits_and_toasts_a_refusal_once()
    {
        let mut app = realm_page();
        // Mounted before any plan told its limits, as the page opens.
        mount::<HamletTheme>(app.world_mut(), part());
        let asked = sent(&mut app)
            .into_iter()
            .filter(|m| m["type"] == "check")
            .collect::<Vec<_>>();
        assert_eq!(asked.len(), 1, "{asked:?}");
        assert_eq!(asked[0]["what"], "limits");

        let now = Timestamp::now().as_millisecond();
        let window = |used: f64| {
            json!({"id": "five_hour", "name": "Session", "length": 300,
                   "used": used, "resets": now + 3_600_000})
        };
        told(
            &mut app,
            json!({"checked": now, "plan": "Max", "state": "ok",
                   "windows": [window(46.0)]}),
        );
        let shown = all_words(&mut app);
        for text in ["Plans", "Claude · Max", "Session", "54% left"]
        {
            assert!(shown.contains(text), "{shown}");
        }
        assert_eq!(toasted(&app), 0);

        // A refusal is toasted once, however often it is told again.
        let refused = json!({"checked": now, "plan": "Max",
            "state": "refused", "windows": [window(100.0)]});
        told(&mut app, refused.clone());
        assert_eq!(toasted(&app), 1);
        told(
            &mut app,
            json!({"checked": now + 1, "plan": "Max",
            "state": "refused", "windows": [window(100.0)]}),
        );
        assert_eq!(toasted(&app), 1);
        assert!(all_words(&mut app).contains("0% left"));
    }
}
