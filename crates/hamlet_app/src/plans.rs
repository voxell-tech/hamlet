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
use bevy::ecs::schedule::common_conditions::{
    resource_changed, resource_exists_and_changed,
};
use bevy::ecs::schedule::{IntoScheduleConfigs, SystemCondition};
use bevy::ecs::system::{Local, Res, ResMut};
use bevy::ecs::world::World;
use bevy::time::Time;
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
use hamlet_protocol::client as protocol;
use hamlet_ui::styles::{
    ICON, action, bare, close, icon_button, note,
};
use hamlet_ui::theme::StatusKind;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::{AnyView, with_cx};
use jiff::Timestamp;
use jiff::tz::TimeZone;

pub(crate) use self::reading::{CHATGPT_USAGE, Mood, Reading, Row};
use crate::PanelSystems;
use crate::hub::Showing;
use crate::models::mark;
use crate::settings::Pane;
use crate::toasts::Toasts;

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
                ask.run_if(
                    resource_changed::<Connection>
                        .or_eager(
                            resource_exists_and_changed::<Showing>,
                        )
                        .or_eager(
                            resource_exists_and_changed::<Pane>,
                        ),
                )
                .in_set(PanelSystems::Follow),
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

/// Asks the server for the limits when a page that shows them opens:
/// the Hub's realm page, or Settings' endpoints. The server reads a
/// provider at most once a minute however often it is asked.
fn ask(world: &mut World) {
    let shown = world.get_resource::<Showing>()
        == Some(&Showing::Realm)
        || world.get_resource::<Pane>() == Some(&Pane::Endpoints);
    if shown {
        check(world);
    }
}

/// Asks the server to read each signed-in provider's limits again.
pub(crate) fn check(world: &mut World) {
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
/// minutes pass, since a reset's words count down.
fn follow(
    providers: Option<Res<Providers>>,
    mut plans: ResMut<Plans>,
    mut minute: Local<i64>,
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
    plans.set_if_neq(Plans(read));
}

/// Raises a toast when a plan starts refusing, and when its pace
/// first says a window will run out before it resets: each once a
/// window, until it resets. What stood when the window began is not
/// new.
fn warn(
    time: Res<Time>,
    plans: Res<Plans>,
    mut toasts: ResMut<Toasts>,
    mut told: Local<HashSet<(String, String, i64)>>,
    mut primed: Local<bool>,
) {
    if !plans.is_changed() {
        return;
    }
    let now = time.elapsed_secs();
    for plan in &plans.0 {
        let reading = &plan.reading;
        let near =
            reading.nearest.and_then(|at| reading.rows.get(at));
        let (kind, head, key) = match reading.mood {
            Mood::Refused => (
                StatusKind::Danger,
                format!("{} refused", plan.title),
                near.map_or((String::new(), 0), |row| {
                    (row.id.clone(), row.resets.unwrap_or(0))
                }),
            ),
            Mood::Short => {
                match near.filter(|row| row.runs_out.is_some()) {
                    Some(row) => (
                        StatusKind::Warning,
                        format!("{} may run out", plan.title),
                        (row.id.clone(), row.resets.unwrap_or(0)),
                    ),
                    None => continue,
                }
            }
            Mood::Fine | Mood::Unknown => continue,
        };
        let key = (plan.provider.clone(), key.0, key.1);
        if told.insert(key) && *primed {
            toasts.note(kind, head, reading.sentence.clone(), now);
        }
    }
    *primed = true;
}

/// Opens `provider`'s limits under it in Settings' endpoints.
pub(crate) fn open(world: &mut World) {
    *world.resource_mut::<Pane>() = Pane::Endpoints;
    crate::dock::show_tab(world, "settings");
}

/// The status of `mood`: none while it is fine, amber when it runs
/// short, red at a refusal.
pub(crate) fn status_of(mood: Mood) -> Option<StatusKind> {
    match mood {
        Mood::Fine | Mood::Unknown => None,
        Mood::Short => Some(StatusKind::Warning),
        Mood::Refused => Some(StatusKind::Danger),
    }
}

/// How much of a window is used, as a bar that fills with it, and a
/// thin mark where even use would stand now.
fn bar(row_: &Row) -> AnyView {
    let (used, pace, mood) = (row_.used, row_.pace, row_.mood);
    with_cx(move |cx| {
        let palette = &cx.theme().palette;
        // Quiet while it lasts; the lamp's orange when it runs short,
        // red at a refusal.
        let fill = match mood {
            Mood::Fine | Mood::Unknown => palette.grey_1,
            Mood::Short => palette.status.attention.mark,
            Mood::Refused => palette.status.danger.mark,
        };
        let mut parts = vec![
            frame()
                .width(percent(used.unwrap_or(0.0) * 100.0))
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
                    .fill(palette.gold_1)
                    .position(PositionType::Absolute)
                    .inset(UiRect::new(
                        percent(pace * 100.0),
                        Val::Auto,
                        px(-OVERHANG),
                        Val::Auto,
                    ))
                    .tooltip(|| {
                        note(
                            "Where even use would stand now: past \
                             it, this limit is used faster than it \
                             lasts",
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
            .fill(palette.gold_6)
    })
    .boxed()
}

/// One window in Settings: its name, its bar and what is used, in
/// columns that line up from row to row, and under the bar when it
/// resets, what is left of a cap, and when it runs out at this pace
/// unless the sentence above already says so.
fn window(row_: &Row, said: bool) -> AnyView {
    let tone =
        status_of(row_.mood).map_or(Tone::Body, StatusKind::words);
    let mut under: Vec<String> =
        row_.reset.iter().chain(&row_.left).cloned().collect();
    if let Some(at) = row_.runs_out.filter(|_| !said) {
        under.push(format!(
            "runs out around {} at this pace",
            at_clock(at)
        ));
    }
    let top = row((
        row((label(row_.name.clone()).wrap(false),))
            .width(px(NAME))
            .shrink(0.0),
        if row_.used.is_some() {
            bar(row_)
        } else {
            frame().grow(1.0).boxed()
        },
        amount(row_, tone),
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

/// What a window has used, right-aligned in a column of its own so
/// every bar ends at the same place.
fn amount(row_: &Row, tone: Tone) -> AnyView {
    row((label(row_.amount.clone()).tone(tone).wrap(false),))
        .width(px(AMOUNT))
        .shrink(0.0)
        .justify(JustifyContent::FlexEnd)
        .boxed()
}

/// `at` as a clock time from now, in the system's zone.
fn at_clock(at: i64) -> String {
    reading::clock(
        at,
        Timestamp::now().as_millisecond(),
        &TimeZone::system(),
    )
}

/// The plan's sentence, led by a mark of its status when it runs
/// short or refuses, since amber words would not read as amber.
fn said(reading: &Reading) -> AnyView {
    let status = status_of(reading.mood);
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
    let mut parts = vec![said(reading)];
    parts.extend(reading.rows.iter().enumerate().map(
        |(at, row_)| {
            // The sentence speaks of the nearest window's run-out.
            window(
                row_,
                Some(at) == nearest && reading.mood == Mood::Short,
            )
        },
    ));
    let mut foot: Vec<AnyView> = Vec::new();
    if plan.provider == "chatgpt" {
        foot.push(
            action(Glyph::Into, "Open ChatGPT usage", |_| {
                hamlet_desktop::open::browse(CHATGPT_USAGE);
            })
            .boxed(),
        );
    }
    foot.push(frame().grow(1.0).boxed());
    // A provider that tells nothing has nothing to check again.
    if reading.mood != Mood::Unknown {
        let when = match &plan.name {
            Some(name) => format!("{name} plan · {}", reading.when),
            None => reading.when.clone(),
        };
        let when = if reading.stale {
            format!("{when}, may have changed")
        } else {
            when
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

/// A plan at a glance, for a narrow column: its mark and name with
/// what its nearest window has used, that window's bar across the
/// whole line, and under it the window's name and when it resets or
/// runs out. Where it has no window, a few words. It opens the
/// plan's limits in Settings.
fn line(plan: &Plan) -> AnyView {
    let reading = &plan.reading;
    let near = reading
        .nearest
        .and_then(|at| reading.rows.get(at))
        .filter(|row_| {
            reading.mood != Mood::Refused || row_.used.is_some()
        });
    let tone =
        status_of(reading.mood).map_or(Tone::Body, StatusKind::words);
    let provider = plan.provider.clone();
    let mut head: Vec<AnyView> = vec![
        mark(&provider, ICON).boxed(),
        label(plan.title.clone()).bold(true).wrap(false).boxed(),
        frame().grow(1.0).boxed(),
    ];
    let mut body: Vec<AnyView> = Vec::new();
    match near {
        Some(row_) => {
            head.push(
                label(row_.amount.clone())
                    .tone(tone)
                    .wrap(false)
                    .boxed(),
            );
            if row_.used.is_some() {
                body.push(bar(row_));
            }
            let when = match (row_.runs_out, row_.resets) {
                (Some(out), Some(resets)) => format!(
                    "may run out {}, resets {}",
                    at_clock(out),
                    at_clock(resets)
                ),
                (Some(out), None) => {
                    format!("may run out {}", at_clock(out))
                }
                (None, Some(resets))
                    if row_.mood == Mood::Refused =>
                {
                    format!("used up until {}", at_clock(resets))
                }
                (None, _) => row_
                    .reset
                    .clone()
                    .or_else(|| row_.left.clone())
                    .unwrap_or_default(),
            };
            let words = if when.is_empty() {
                row_.name.clone()
            } else {
                format!("{} · {when}", row_.name)
            };
            body.push(label(words).tone(Tone::Dim).boxed());
        }
        // ChatGPT's refusal says all it can in a few words here; its
        // sentence is in Settings.
        None if plan.provider == "chatgpt"
            && reading.mood == Mood::Refused =>
        {
            body.push(
                label(format!(
                    "Refused at {}. OpenAI does not say until when.",
                    reading.when.trim_start_matches("refused at ")
                ))
                .tone(tone)
                .boxed(),
            );
        }
        None if plan.provider == "chatgpt" => {
            body.push(
                note("OpenAI does not share how much is used.")
                    .boxed(),
            );
        }
        None => {
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
                crate::screens::overview::part_title(
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
/// refusal to tell, and ChatGPT, whose use is told nowhere else.
fn glanced(plans: &Plans) -> Vec<Plan> {
    plans
        .0
        .iter()
        .filter(|p| {
            !p.reading.rows.is_empty()
                || p.reading.mood == Mood::Refused
                || p.provider == "chatgpt"
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
    let lower = error.to_lowercase();
    let says_limit = ["limit", "quota", "402", "usage"]
        .iter()
        .any(|word| lower.contains(word));
    if plan.reading.mood != Mood::Refused && !says_limit {
        return None;
    }
    Some(match provider {
        "chatgpt" => "OpenAI refused this: your ChatGPT plan, or \
                      the share of it you allow Hamlet, is used up. \
                      OpenAI does not say which, or when it lifts."
            .into(),
        _ if plan.reading.mood == Mood::Refused => {
            format!(
                "{} refused this. {}",
                plan.title, plan.reading.sentence
            )
        }
        _ => format!("{} refused this for a limit.", plan.title),
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
        app.init_resource::<Toasts>()
            .insert_resource(Showing::Realm)
            .add_plugins(plugin);
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

    fn toasted(app: &App) -> usize {
        app.world().resource::<Toasts>().len()
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
        for text in ["Plans", "Claude · Max", "Session", "46% used"]
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
        assert!(all_words(&mut app).contains("100% used"));
    }
}
