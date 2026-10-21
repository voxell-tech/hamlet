//! A hamlet's BYLAWS tab: the limits of its own file, each control
//! writing one key of it through the agents service, which refuses
//! the realm's.

use agents_ui::{
    AgentsWorldExt, Heard, note, part, setting, switch, whole,
};
use hamlet_engine_sdk::api::agents::{
    AgentsReply, AgentsRequest, Bylaws, CatalogueModel, Priced,
};
use hamlet_engine_sdk::ui::{
    AnyView, FrameProps, Size, Space, Tone, UiExt, ViewExt, World,
    column, dropdown, field, icon_button, keyed, label, resource,
    row, toggle,
};

use crate::agent::{Models, offered};

/// The most a sum of dollars of the bylaws may be.
const MOST_DOLLARS: f64 = 1_000_000.0;

/// The bylaws' methods on [`CatalogueModel`].
trait CatalogueModelExt {
    /// One line for whoever chooses it: its name, what it costs, how
    /// much it holds and what it is for.
    fn card(&self) -> String;
}

impl CatalogueModelExt for CatalogueModel {
    fn card(&self) -> String {
        let mut card = match self.priced {
            Priced::Unknown => {
                format!("{}: price not known", self.name)
            }
            _ => format!(
                "{}: ${:.2} in and ${:.2} out for a million tokens",
                self.name, self.input, self.output
            ),
        };
        if let Some(context) = self.context {
            card.push_str(&format!(", holds {context} tokens"));
        }
        if !self.note.trim().is_empty() {
            card.push_str(&format!(". {}", self.note.trim()));
        }
        card
    }
}

/// The bylaws of the hamlet last asked for.
#[derive(Default)]
pub(crate) struct Laws {
    of: Option<String>,
    bylaws: Bylaws,
    /// Why the last change did not go through.
    problem: Option<String>,
}

pub(crate) fn prepare(world: &mut World) {
    world.init_resource::<Laws>();
}

/// Reads the bylaws of the hamlet `id` again.
pub(crate) fn read(world: &mut World, id: &str) {
    // Another hamlet's are not shown meanwhile.
    if world.resource::<Laws>().of.as_deref() != Some(id) {
        world.insert(Laws {
            of: Some(id.to_string()),
            ..Laws::default()
        });
    }
    world.ask(AgentsRequest::Bylaws(id.to_string()));
}

/// Lets go of the bylaws, as their page is no longer shown.
pub(crate) fn leave(world: &mut World) {
    world.resource_mut::<Laws>().of = None;
}

/// Reads the bylaws shown again, as the hamlets change.
pub(crate) fn follow(world: &mut World) {
    if let Some(of) = world.resource::<Laws>().of.clone() {
        world.ask(AgentsRequest::Bylaws(of));
    }
}

pub(crate) fn heard(world: &mut World, heard: &Heard) {
    match heard {
        Heard::Reply(
            Some(AgentsRequest::Bylaws(of)),
            AgentsReply::Bylaws(bylaws),
        ) => {
            let laws = world.resource::<Laws>();
            if laws.of.as_ref() == Some(of) && laws.bylaws != *bylaws
            {
                world.resource_mut::<Laws>().bylaws = bylaws.clone();
            }
        }
        Heard::Reply(
            Some(AgentsRequest::SetBylaw { hamlet, .. }),
            AgentsReply::Done,
        ) if world.resource::<Laws>().of.as_ref() == Some(hamlet) => {
            world.resource_mut::<Laws>().problem = None;
            read(world, hamlet);
        }
        Heard::Failed(
            Some(AgentsRequest::SetBylaw { hamlet, .. }),
            why,
        ) if world.resource::<Laws>().of.as_ref() == Some(hamlet) => {
            world.resource_mut::<Laws>().problem =
                Some(why.to_string());
            read(world, hamlet);
        }
        _ => {}
    }
}

/// Sets `key` of `section` of the bylaws shown to `value`, or takes
/// it out.
fn set(
    world: &mut World,
    section: &str,
    key: &str,
    value: Option<String>,
) {
    let Some(hamlet) = world.resource::<Laws>().of.clone() else {
        return;
    };
    world.ask(AgentsRequest::SetBylaw {
        hamlet,
        section: section.to_string(),
        key: key.to_string(),
        value,
    });
}

fn refuse(world: &mut World, why: String) {
    world.resource_mut::<Laws>().problem = Some(why);
}

/// A whole number of the bylaws, at `key` of `section`, from 0 to
/// `most`.
fn number(
    what: &'static str,
    (section, key): (&'static str, &'static str),
    most: u64,
    get: fn(&Bylaws) -> u64,
) -> AnyView {
    whole::<Laws>(
        what,
        most,
        move |laws| Some(get(&laws.bylaws)),
        move |world, n| set(world, section, key, Some(n.to_string())),
        refuse,
    )
}

/// A yes or a no of the bylaws, at `key` of `section`.
fn yes(
    what: &'static str,
    (section, key): (&'static str, &'static str),
    get: fn(&Bylaws) -> bool,
) -> AnyView {
    switch::<Laws>(
        what,
        move |laws| get(&laws.bylaws),
        move |world, on| {
            set(world, section, key, Some(on.to_string()))
        },
    )
}

/// A sum in dollars of the bylaws, at `key` of `section`, from 0 to
/// [`MOST_DOLLARS`]. 0 takes the key out.
fn dollars(
    what: &'static str,
    (section, key): (&'static str, &'static str),
    get: fn(&Bylaws) -> Option<f64>,
) -> AnyView {
    let control = field(resource::<Laws, _>(move |laws| {
        format!("{:.2}", get(&laws.bylaws).unwrap_or(0.0))
    }))
    .on_commit(move |world, typed| {
        let Some(sum) = typed
            .trim()
            .trim_start_matches('$')
            .parse::<f64>()
            .ok()
            .filter(|sum| (0.0..=MOST_DOLLARS).contains(sum))
        else {
            return refuse(
                world,
                format!(
                    "{what}: a sum of dollars from 0 to \
                     {MOST_DOLLARS}."
                ),
            );
        };
        let value = (sum > 0.0).then(|| format!("{sum:.2}"));
        set(world, section, key, value);
    })
    .boxed();
    setting(what, control, None)
}

fn budget(id: String) -> AnyView {
    let period = dropdown(
        vec!["A DAY".to_string(), "A MONTH".to_string()],
        resource::<Laws, _>(|laws| u32::from(laws.bylaws.monthly)),
    )
    .on_pick(|world, at| {
        let period = if at == 1 { "month" } else { "day" };
        set(world, "budget", "period", Some(format!("\"{period}\"")));
    })
    .boxed();
    column((
        part("meter", "THE BUDGET"),
        note(
            "The most model calls may cost in a period. 0 means no \
             limit.",
        ),
        setting("It starts again every", period, None),
        dollars("May spend, in dollars", ("budget", "cost"), |b| {
            b.budget_cost
        }),
        row((
            icon_button("restore", "START IT AGAIN NOW").on_click(
                move |world| world.decree(&id, "/reset-budget"),
            ),
            note("What was spent stays in the record."),
        ))
        .space(Space::Gap),
        part("coins", "ONE TASK"),
        note("Counted across every agent the task touches."),
        dollars("May cost, in dollars", ("task", "cost"), |b| {
            b.task_cost
        }),
        number(
            "Messages between agents",
            ("task", "messages"),
            10_000,
            |b| u64::from(b.task_messages),
        ),
        dollars(
            "Price of a road, in dollars",
            ("road", "cost"),
            |b| Some(b.road_cost),
        ),
    ))
    .space(Space::Half)
    .boxed()
}

/// Puts `name` on the list of models the hamlet may run, or takes it
/// off.
fn pick(world: &mut World, name: &str, on: bool) {
    let mut list = world.resource::<Laws>().bylaws.models.clone();
    list.retain(|known| known != name);
    if on {
        list.push(name.to_string());
    }
    let written = list
        .iter()
        .map(|name| format!("{name:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    set(world, "", "models", Some(format!("[{written}]")));
}

fn models() -> AnyView {
    let ticks = keyed(
        resource::<Models, _>(|models| models.allowed.clone()),
        |allowed| {
            if allowed.is_empty() {
                return note(
                    "The home allows no model besides its default \
                     yet. Allow some under Models in Settings.",
                )
                .boxed();
            }
            let rows = allowed
                .iter()
                .map(|model| {
                    let (shown, picked) =
                        (model.name.clone(), model.name.clone());
                    row((
                        toggle(resource::<Laws, _>(move |laws| {
                            laws.bylaws.models.contains(&shown)
                        }))
                        .on_toggle(
                            move |world, on| pick(world, &picked, on),
                        ),
                        note(model.card()),
                    ))
                    .space(Space::Gap)
                    .boxed()
                })
                .collect::<Vec<_>>();
            column(rows).space(Space::Half).boxed()
        },
    );
    let default = keyed(
        resource::<Laws, _>(|laws| laws.bylaws.model.clone()),
        |own| {
            let own = own.clone();
            keyed(
                resource::<Models, _>(move |models| {
                    let now = own
                        .clone()
                        .unwrap_or_else(|| models.default.clone());
                    (
                        offered(models, &now),
                        now,
                        models.default.clone(),
                    )
                }),
                |(names, now, home)| {
                    let at = names
                        .iter()
                        .position(|name| name == now)
                        .unwrap_or(names.len());
                    let (names, home) = (names.clone(), home.clone());
                    dropdown(
                        names.clone(),
                        u32::try_from(at).unwrap_or(u32::MAX),
                    )
                    .on_pick(move |world, picked| {
                        let Some(model) = names.get(picked as usize)
                        else {
                            return;
                        };
                        // The home's own is what a hamlet has with
                        // none set.
                        let value = (*model != home)
                            .then(|| format!("{model:?}"));
                        set(world, "", "model", value);
                    })
                    .boxed()
                },
            )
            .boxed()
        },
    );
    column((
        part("chip", "MODELS THIS HAMLET MAY RUN"),
        note(
            "The mayor picks from the ticked models. The default \
             model is always allowed.",
        ),
        ticks,
        setting("Its default model", default.boxed(), None),
        note("Used when neither you nor the mayor chose a model."),
    ))
    .space(Space::Half)
    .boxed()
}

fn mayor() -> AnyView {
    column((
        part("crown", "THE MAYOR MAY, WITHOUT ASKING"),
        yes("Create agents", ("mayor", "create_agents"), |b| {
            b.create_agents
        }),
        yes("Remove agents", ("mayor", "remove_agents"), |b| {
            b.remove_agents
        }),
        yes("Merge agents", ("mayor", "merge_agents"), |b| {
            b.merge_agents
        }),
        yes("Build roads", ("mayor", "build_roads"), |b| {
            b.build_roads
        }),
        number(
            "Days of its decisions agents are told",
            ("mayor", "told_days"),
            3650,
            |b| b.told_days,
        ),
        part("signpost", "AGENTS AND ROADS"),
        number(
            "Most agents in this hamlet",
            ("", "most_agents"),
            1000,
            |b| b.most_agents,
        ),
        number(
            "Most roads the mayor builds",
            ("", "most_roads"),
            10_000,
            |b| b.most_roads,
        ),
    ))
    .space(Space::Half)
    .boxed()
}

/// The BYLAWS tab of the hamlet `id`.
pub(crate) fn bylaws(id: String) -> AnyView {
    let problem = keyed(
        resource::<Laws, _>(|laws| {
            laws.problem
                .clone()
                .or_else(|| laws.bylaws.problem.clone())
        }),
        |problem| match problem {
            Some(why) => label(why.clone())
                .size(Size::Small)
                .tone(Tone::Critical)
                .boxed(),
            None => column(()).boxed(),
        },
    );
    column((
        note(
            "These apply to this hamlet only. Rules for every \
             hamlet are under Realm in Settings.",
        ),
        problem.boxed(),
        budget(id),
        models(),
        mayor(),
    ))
    .space(Space::Double)
    .boxed()
}
