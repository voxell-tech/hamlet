//! The commands that shape the society: hamlets, agents and profiles.
//! Each is a function of its own, named in [`SOCIETY`] beside how it
//! is written.

use hamlet_store::ids::{Actor, HamletId};

use super::targets::{ProfileReach, is_profile};
use super::{Answer, Command, Line};
use crate::core::Core;
use crate::society::addresses::{self, Reach};
use crate::society::agents::{self, AgentKind, Effort, EndHow};
use crate::society::hamlet_agents::{self, ProfileChoice, Spec};
use crate::society::hamlets::{self, Founding, Mode};
use crate::society::refusal::Refusal;
use crate::society::{
    authority, channels, city, record, spend, workers,
};

/// The word that stands in place of a profile for a blank agent.
const BLANK: &str = "blank";

/// The commands that shape the society: hamlets, agents and profiles.
pub(super) const SOCIETY: &[Command] = &[
    Command {
        name: "hamlets",
        usage: "/hamlets",
        does: "List the hamlets",
        run: list_hamlets,
    },
    Command {
        name: "create-hamlet",
        usage: "/create-hamlet [profile#id|blank] <name>[: purpose]",
        does: "Create a hamlet named <name> under a profile as its \
               lead, or a blank agent named Lead if none is given",
        run: create_hamlet,
    },
    Command {
        name: "name-hamlet",
        usage: "/name-hamlet <hamlet> <name>",
        does: "Give a hamlet another name to be shown by",
        run: name_hamlet,
    },
    Command {
        name: "move-hamlet",
        usage: "/move-hamlet <hamlet> <x> <z>",
        does: "Set a hamlet down elsewhere on the hamlet island",
        run: move_hamlet,
    },
    Command {
        name: "remove-hamlet",
        usage: "/remove-hamlet <hamlet> [reason]",
        does: "Remove a hamlet and end its agents",
        run: |core, line| hamlet_fate(core, line, Fate::Remove),
    },
    Command {
        name: "restore-hamlet",
        usage: "/restore-hamlet <hamlet> [reason]",
        does: "Bring a removed hamlet back",
        run: |core, line| hamlet_fate(core, line, Fate::Restore),
    },
    Command {
        name: "erase-hamlet",
        usage: "/erase-hamlet <hamlet>",
        does: "Erase a removed hamlet for good",
        run: |core, line| hamlet_fate(core, line, Fate::Erase),
    },
    Command {
        name: "tier",
        usage: "/tier <50|100|200|500>",
        does: "Set how many agents this hamlet holds",
        run: tier,
    },
    Command {
        name: "lead",
        usage: "/lead <address> [reason]",
        does: "Give this hamlet's office to an agent",
        run: lead,
    },
    Command {
        name: "add-agent",
        usage: "/add-agent <profile#id>[@<version>]|blank [in \
                <hamlet>] [under <address>] [purpose]",
        does: "Add an agent of a profile to this hamlet, or a blank \
               agent that is not a profile of the city",
        run: add_agent,
    },
    Command {
        name: "end",
        usage: "/end <address> [reason]",
        does: "End an agent",
        run: |core, line| agent_fate(core, line, Fate::Remove),
    },
    Command {
        name: "restore",
        usage: "/restore <address> [reason]",
        does: "Bring an ended agent back",
        run: |core, line| agent_fate(core, line, Fate::Restore),
    },
    Command {
        name: "erase",
        usage: "/erase <address>",
        does: "Erase an ended agent for good",
        run: |core, line| agent_fate(core, line, Fate::Erase),
    },
    Command {
        name: "merge",
        usage: "/merge <address> into <address> [reason]",
        does: "Make one agent of two; /restore splits them again",
        run: merge,
    },
    Command {
        name: "model",
        usage: "/model <address> <model|none> [reason]",
        does: "Choose an agent's model",
        run: model,
    },
    Command {
        name: "effort",
        usage: "/effort <address> \
                <off|minimal|low|medium|high|xhigh|max|none>",
        does: "Choose how hard an agent's model thinks",
        run: effort,
    },
    Command {
        name: "fast",
        usage: "/fast <address> <on|off>",
        does: "Turn an agent's fast tier on or off",
        run: fast,
    },
    Command {
        name: "name-agent",
        usage: "/name-agent <address> [name]",
        does: "Give an agent or a worker a name to be shown by in \
               the world, or none to go by its own again",
        run: name_agent,
    },
    Command {
        name: "tools",
        usage: "/tools <address> -<tool> +<tool>",
        does: "Turn an agent's tools off or back on",
        run: tools,
    },
    Command {
        name: "allowance",
        usage: "/allowance <address> <dollars|none>",
        does: "Give an agent an allowance for the period",
        run: allowance,
    },
    Command {
        name: "channel",
        usage: "/channel <address> <address> [reason]",
        does: "Open a channel between two agents",
        run: |core, line| channel(core, line, Channelwork::Open),
    },
    Command {
        name: "unchannel",
        usage: "/unchannel <address> <address> [reason]",
        does: "Close a channel",
        run: |core, line| channel(core, line, Channelwork::Close),
    },
    Command {
        name: "folder",
        usage: "/folder <folder|none>",
        does: "Choose the folder this hamlet works on",
        run: folder,
    },
    Command {
        name: "mode",
        usage: "/mode <manual|auto>",
        does: "Choose how this hamlet's work is overseen",
        run: mode,
    },
    Command {
        name: "budget",
        usage: "/budget",
        does: "Show this hamlet's spend",
        run: budget,
    },
    Command {
        name: "reset-budget",
        usage: "/reset-budget",
        does: "Start this hamlet's budget period again",
        run: reset_budget,
    },
    Command {
        name: "history",
        usage: "/history",
        does: "Show this hamlet's decisions",
        run: history,
    },
    Command {
        name: "profiles",
        usage: "/profiles",
        does: "List the city's profiles",
        run: list_profiles,
    },
    Command {
        name: "profile",
        usage: "/profile new <name>[: <character>]",
        does: "Make a profile in the city; a name is a label with \
               no colon, and the character follows the colon",
        run: new_profile,
    },
    Command {
        name: "name-profile",
        usage: "/name-profile <profile#id> <name>",
        does: "Give a profile a new name",
        run: name_profile,
    },
    Command {
        name: "remove-profile",
        usage: "/remove-profile <profile#id> [reason]",
        does: "Archive a profile and end its agents",
        run: |core, line| profile_fate(core, line, Housing::Remove),
    },
    Command {
        name: "restore-profile",
        usage: "/restore-profile <profile#id> [reason]",
        does: "Bring an archived profile back with its agents",
        run: |core, line| profile_fate(core, line, Housing::Restore),
    },
    Command {
        name: "erase-profile",
        usage: "/erase-profile <profile#id>",
        does: "Erase an archived profile for good",
        run: |core, line| profile_fate(core, line, Housing::Erase),
    },
    Command {
        name: "move-house",
        usage: "/move-house <profile#id> <x> <z> [degrees]",
        does: "Set a profile's house down elsewhere in the city",
        run: move_house,
    },
    Command {
        name: "welcome",
        usage: "/welcome <profile#id> [reason]",
        does: "Let a profile waiting at the harbor into the city",
        run: |core, line| profile_fate(core, line, Housing::Welcome),
    },
    Command {
        name: "turn-away",
        usage: "/turn-away <profile#id> [reason]",
        does: "Keep a profile at the harbor, or send it back there",
        run: |core, line| profile_fate(core, line, Housing::TurnAway),
    },
];

/// The fate a command gives a hamlet or an agent that may end.
#[derive(Clone, Copy, PartialEq)]
enum Fate {
    /// It ends, or the hamlet is removed.
    Remove,
    /// It comes back.
    Restore,
    /// It goes for good.
    Erase,
}

/// The place a command gives a profile in the city.
#[derive(Clone, Copy)]
enum Housing {
    Remove,
    Restore,
    Erase,
    Welcome,
    TurnAway,
}

/// A command's work on a channel.
#[derive(Clone, Copy)]
enum Channelwork {
    Open,
    Close,
}

/// The chief as the actor of a command.
fn by(core: &Core<'_, '_>) -> Actor {
    Actor::Chief(core.realm.chief())
}

/// The usage of `line`'s command, as its refusal.
fn usage(core: &Core<'_, '_>, line: &Line) -> Answer {
    Ok(Err(core.deeds.usage(&line.name)))
}

/// Refuses as it is, or goes on with what was found.
macro_rules! found {
    ($result:expr) => {
        match $result? {
            Ok(found) => found,
            Err(refusal) => return Ok(Err(refusal)),
        }
    };
}

fn list_hamlets(core: &mut Core<'_, '_>, _: &Line) -> Answer {
    let db = core.realm.store().connection();
    let list = hamlets::all(db)?
        .into_iter()
        .filter(hamlets::Hamlet::is_living)
        .map(|h| format!("- {} (`{}`)", h.name, h.id))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(Ok(if list.is_empty() {
        "No hamlet yet. /create-hamlet makes one.".into()
    } else {
        list
    }))
}

fn create_hamlet(core: &mut Core<'_, '_>, line: &Line) -> Answer {
    let chief = core.realm.chief();
    let words = line.words();
    // A lead not given is a blank agent named Lead.
    let blank = || ProfileChoice::Blank {
        name: String::new(),
        character: String::new(),
    };
    let (lead, name_at) = match words.first() {
        Some(&BLANK) => (blank(), 1),
        Some(profile) if is_profile(profile) => {
            (found!(core.profile_choice(profile, chief)), 1)
        }
        _ => (blank(), 0),
    };
    // A name has no colon, so what follows one is the purpose; one
    // given none is there to run the hamlet.
    let rest = line.tail(name_at);
    let (shown, purpose) = rest
        .split_once(':')
        .map_or((rest.as_str(), ""), |(n, p)| (n, p));
    let shown = shown.trim().to_owned();
    if shown.is_empty() {
        return usage(core, line);
    }
    let purpose = match purpose.trim() {
        "" => format!("Lead of {shown}."),
        given => given.to_owned(),
    };
    let added = found!(core.attempt(by(core), |cx, _| {
        hamlets::found(
            cx,
            Founding {
                name: shown.clone(),
                chief,
                lead,
                purpose,
            },
        )
    }));
    let db = core.realm.store().connection();
    Ok(Ok(format!(
        "The hamlet {shown} (`{}`) is created under its lead `{}`.",
        added.hamlet,
        addresses::name_of(db, added.agent)?
    )))
}

fn name_hamlet(core: &mut Core<'_, '_>, line: &Line) -> Answer {
    let Some(id) = line.words().first().copied() else {
        return usage(core, line);
    };
    let id = HamletId(id.to_owned());
    found!(core.chief_of(&id));
    let shown = line.tail(1);
    let done = core.attempt(by(core), |cx, _| {
        hamlets::rename(cx, &id, &shown)
    })?;
    Ok(done.map(|()| {
        format!("The hamlet `{id}` is named {}.", shown.trim())
    }))
}

fn move_hamlet(core: &mut Core<'_, '_>, line: &Line) -> Answer {
    let words = line.words();
    let (Some(id), Some(x), Some(z)) = (
        words.first(),
        words.get(1).and_then(|w| w.parse::<f64>().ok()),
        words.get(2).and_then(|w| w.parse::<f64>().ok()),
    ) else {
        return usage(core, line);
    };
    let id = HamletId((*id).to_owned());
    found!(core.chief_of(&id));
    let done = core
        .attempt(by(core), |cx, _| hamlets::place(cx, &id, x, z))?;
    Ok(done.map(|()| {
        format!("The hamlet `{id}` is set down at {x}, {z}.")
    }))
}

fn hamlet_fate(
    core: &mut Core<'_, '_>,
    line: &Line,
    fate: Fate,
) -> Answer {
    let Some(id) = line.words().first().copied() else {
        return usage(core, line);
    };
    let id = HamletId(id.to_owned());
    found!(core.chief_of(&id));
    // The chief needs no reason to close a hamlet down.
    let reason = match line.tail(1) {
        said if !said.is_empty() => said,
        _ if fate == Fate::Remove => "closed by the chief".to_owned(),
        said => said,
    };
    let done = core.attempt(by(core), |cx, _| match fate {
        Fate::Remove => hamlets::remove(cx, &id, &reason),
        Fate::Restore => hamlets::restore(cx, &id, &reason),
        Fate::Erase => hamlets::erase(cx, &id),
    })?;
    Ok(done.map(|()| match fate {
        Fate::Remove => {
            format!(
                "The hamlet `{id}` was removed and its agents ended."
            )
        }
        Fate::Restore => format!("The hamlet `{id}` is restored."),
        Fate::Erase => {
            format!("The hamlet `{id}` was erased for good.")
        }
    }))
}

fn tier(core: &mut Core<'_, '_>, line: &Line) -> Answer {
    let Some(id) = line.here.clone() else {
        return Ok(Err(core.no_hamlet()));
    };
    let Some(tier) =
        line.words().first().and_then(|w| w.parse::<i64>().ok())
    else {
        return usage(core, line);
    };
    found!(core.chief_of(&id));
    let done = core.attempt(by(core), |cx, _| {
        hamlets::set_tier(cx, &id, tier)
    })?;
    Ok(done.map(|()| {
        format!("The hamlet `{id}` holds at most {tier} agents.")
    }))
}

fn lead(core: &mut Core<'_, '_>, line: &Line) -> Answer {
    let Some(id) = line.here.clone() else {
        return Ok(Err(core.no_hamlet()));
    };
    let Some(text) = line.words().first().copied() else {
        return usage(core, line);
    };
    found!(core.chief_of(&id));
    let agent = found!(core.resolve(text, Some(&id), Reach::Living));
    let reason = line.tail(1);
    let done = core.attempt(by(core), |cx, _| {
        hamlet_agents::set_lead(cx, &id, agent, &reason)
    })?;
    Ok(
        done.map(|()| {
            format!("`{text}` holds the office of `{id}`.")
        }),
    )
}

fn add_agent(core: &mut Core<'_, '_>, line: &Line) -> Answer {
    let chief = core.realm.chief();
    let words = line.words();
    let Some(profile) = words.first() else {
        return usage(core, line);
    };
    let mut rest = &words[1..];
    let mut hamlet = line.here.clone();
    let mut parent = None;
    while let [key, value, more @ ..] = rest {
        match *key {
            "under" => {
                parent = Some(found!(core.resolve(
                    value,
                    hamlet.as_ref(),
                    Reach::Living,
                )));
            }
            "in" => hamlet = Some(HamletId((*value).to_owned())),
            _ => break,
        }
        rest = more;
    }
    let Some(hamlet) = hamlet else {
        return Ok(Err(core.no_hamlet()));
    };
    found!(core.chief_of(&hamlet));
    let purpose = rest.join(" ");
    let parent = match parent {
        Some(parent) => parent,
        None => {
            let db = core.realm.store().connection();
            found!(hamlets::living(db, &hamlet)).lead
        }
    };
    let choice = if *profile == BLANK {
        ProfileChoice::Blank {
            name: String::new(),
            character: String::new(),
        }
    } else {
        found!(core.profile_choice(profile, chief))
    };
    let added = found!(core.attempt(Actor::Chief(chief), |cx, _| {
        hamlet_agents::add(
            cx,
            Spec {
                hamlet: hamlet.clone(),
                parent: Some(parent),
                profile: choice,
                purpose,
                reason: String::new(),
                owner: chief,
            },
        )
    }));
    let db = core.realm.store().connection();
    Ok(Ok(format!(
        "`{}` is added.",
        addresses::name_of(db, added.agent)?
    )))
}

fn agent_fate(
    core: &mut Core<'_, '_>,
    line: &Line,
    fate: Fate,
) -> Answer {
    let Some(text) = line.words().first().copied() else {
        return usage(core, line);
    };
    let reach = if fate == Fate::Remove {
        Reach::Living
    } else {
        Reach::Kept
    };
    let agent = found!(core.resolve(text, line.here.as_ref(), reach));
    found!(core.directs(agent));
    let reason = line.tail(1);
    let db = core.realm.store().connection();
    // A merged agent comes back by splitting its merge.
    let merge = match fate {
        Fate::Restore => {
            crate::society::merges::of_absorbed(db, agent)?
        }
        Fate::Remove | Fate::Erase => None,
    };
    let done =
        core.attempt(by(core), |cx, _| match (fate, &merge) {
            (Fate::Remove, _) => {
                hamlet_agents::end(cx, agent, EndHow::Ended, &reason)
            }
            (Fate::Restore, Some(merge)) => {
                crate::society::merges::split(cx, merge.id, &reason)
            }
            (Fate::Restore, None) => {
                hamlet_agents::come_back(cx, agent, &reason)
            }
            (Fate::Erase, _) => hamlet_agents::erase(cx, agent),
        })?;
    Ok(done.map(|()| match fate {
        Fate::Remove => format!("`{text}` was ended."),
        Fate::Restore if merge.is_some() => format!(
            "`{text}` is back; what it brought to the agent it was \
             merged into is taken out again."
        ),
        Fate::Restore => format!("`{text}` is back."),
        Fate::Erase => format!("`{text}` was erased for good."),
    }))
}

fn merge(core: &mut Core<'_, '_>, line: &Line) -> Answer {
    let words = line.words();
    let (Some(absorbed), Some(&"into"), Some(kept)) =
        (words.first(), words.get(1), words.get(2))
    else {
        return usage(core, line);
    };
    let here = line.here.as_ref();
    let absorbed =
        found!(core.resolve(absorbed, here, Reach::Living));
    let kept = found!(core.resolve(kept, here, Reach::Living));
    for agent in [absorbed, kept] {
        found!(core.directs(agent));
    }
    let reason = match line.tail(3) {
        reason if reason.is_empty() => {
            "merged by the chief".to_owned()
        }
        reason => reason,
    };
    let done = core.attempt(by(core), |cx, _| {
        crate::society::merges::merge(cx, kept, absorbed, &reason)
    })?;
    Ok(done.map(|_| {
        format!("Merged `{}` into `{}`.", words[0], words[2])
    }))
}

fn model(core: &mut Core<'_, '_>, line: &Line) -> Answer {
    let words = line.words();
    let (Some(text), Some(model)) = (words.first(), words.get(1))
    else {
        return usage(core, line);
    };
    let agent =
        found!(core.resolve(text, line.here.as_ref(), Reach::Living));
    found!(core.steers(agent));
    let model = (*model != "none").then_some(*model);
    let reason = line.tail(2);
    let done = core.attempt(by(core), |cx, _| {
        agents::set_model(cx, agent, model, &reason)
    })?;
    Ok(done.map(|()| {
        format!(
            "`{text}` runs {}.",
            model.unwrap_or("its default model")
        )
    }))
}

fn effort(core: &mut Core<'_, '_>, line: &Line) -> Answer {
    let words = line.words();
    let (Some(text), Some(value)) = (words.first(), words.get(1))
    else {
        return usage(core, line);
    };
    let agent =
        found!(core.resolve(text, line.here.as_ref(), Reach::Living));
    found!(core.steers(agent));
    let effort = match *value {
        "none" => None,
        level => match level.parse::<Effort>() {
            Ok(effort) => Some(effort),
            Err(_) => return usage(core, line),
        },
    };
    let done = core.attempt(by(core), |cx, _| {
        agents::set_effort(cx, agent, effort)
    })?;
    Ok(done.map(|()| match effort {
        Some(effort) => format!("`{text}` thinks at {effort}."),
        None => {
            format!(
                "`{text}` thinks as hard as the realm's setting \
                 says."
            )
        }
    }))
}

fn fast(core: &mut Core<'_, '_>, line: &Line) -> Answer {
    let words = line.words();
    let (Some(text), Some(value)) = (words.first(), words.get(1))
    else {
        return usage(core, line);
    };
    let agent =
        found!(core.resolve(text, line.here.as_ref(), Reach::Living));
    found!(core.steers(agent));
    let fast = match *value {
        "on" => true,
        "off" => false,
        _ => return usage(core, line),
    };
    let done = core.attempt(by(core), |cx, _| {
        agents::set_fast(cx, agent, fast)
    })?;
    Ok(done.map(|()| {
        format!("`{text}` calls for the fast tier: {value}.")
    }))
}

fn name_agent(core: &mut Core<'_, '_>, line: &Line) -> Answer {
    let Some(text) = line.words().first().copied() else {
        return usage(core, line);
    };
    let agent =
        found!(core.resolve(text, line.here.as_ref(), Reach::Living));
    // A worker is named by whoever may steer it, an agent by whoever
    // may direct it.
    let worker = {
        let db = core.realm.store().connection();
        agents::get(db, agent)?.kind == AgentKind::Worker
    };
    if worker {
        found!(core.steers(agent));
    } else {
        found!(core.directs(agent));
    }
    let typed = line.tail(1);
    let given = (!typed.is_empty()).then_some(typed);
    let done = core.attempt(by(core), |cx, _| {
        if worker {
            workers::set_name(cx, agent, given.as_deref())
        } else {
            hamlet_agents::set_name(cx, agent, given.as_deref(), "")
        }
    })?;
    Ok(done.map(|()| match &given {
        Some(given) => {
            format!("`{text}` is called {given} now.")
        }
        None => format!("`{text}` goes by its own name again."),
    }))
}

fn tools(core: &mut Core<'_, '_>, line: &Line) -> Answer {
    let words = line.words();
    let Some(text) = words.first() else {
        return usage(core, line);
    };
    let agent =
        found!(core.resolve(text, line.here.as_ref(), Reach::Living));
    found!(core.directs(agent));
    let marked = |mark: char| {
        words[1..]
            .iter()
            .filter_map(|w| w.strip_prefix(mark))
            .map(String::from)
            .collect::<Vec<_>>()
    };
    let (off, on) = (marked('-'), marked('+'));
    if off.is_empty() && on.is_empty() {
        let db = core.realm.store().connection();
        let kept = hamlet_agents::tools_off(db, agent)?;
        return Ok(Ok(if kept.is_empty() {
            format!("`{text}` has every tool.")
        } else {
            format!("`{text}` has these off: {}", kept.join(", "))
        }));
    }
    let done = core.attempt(by(core), |cx, _| {
        hamlet_agents::set_tools(cx, agent, &off, &on, "")
    })?;
    Ok(done.map(|()| format!("`{text}`'s tools are changed.")))
}

fn allowance(core: &mut Core<'_, '_>, line: &Line) -> Answer {
    let words = line.words();
    let (Some(text), Some(amount)) = (words.first(), words.get(1))
    else {
        return usage(core, line);
    };
    let agent =
        found!(core.resolve(text, line.here.as_ref(), Reach::Living));
    found!(core.directs(agent));
    let allowance = match *amount {
        "none" => None,
        amount => match amount.trim_start_matches('$').parse::<f64>()
        {
            Ok(dollars) if dollars >= 0.0 => {
                Some(spend::micros(dollars))
            }
            _ => return usage(core, line),
        },
    };
    let done = core.attempt(by(core), |cx, _| {
        hamlet_agents::set_allowance(cx, agent, allowance, "")
    })?;
    Ok(done.map(|()| match allowance {
        Some(micros) => {
            format!(
                "`{text}` may spend {} a period.",
                spend::money(micros)
            )
        }
        None => format!("`{text}` has no allowance of its own."),
    }))
}

/// Opens a channel between two agents, or closes it.
fn channel(
    core: &mut Core<'_, '_>,
    line: &Line,
    work: Channelwork,
) -> Answer {
    let words = line.words();
    let (Some(a), Some(b)) = (words.first(), words.get(1)) else {
        return usage(core, line);
    };
    let here = line.here.as_ref();
    let a = found!(core.resolve(a, here, Reach::Living));
    let b = found!(core.resolve(b, here, Reach::Living));
    for end in [a, b] {
        found!(core.directs(end));
    }
    let reason = line.tail(2);
    match work {
        Channelwork::Open => {
            let done = core.attempt(by(core), |cx, _| {
                channels::open(cx, a, b, &reason, None)
            })?;
            Ok(done.map(|_| "The channel is open.".into()))
        }
        Channelwork::Close => {
            let done = core.attempt(by(core), |cx, _| {
                channels::close(cx, a, b, &reason)
            })?;
            Ok(done.map(|()| "The channel is closed.".into()))
        }
    }
}

fn folder(core: &mut Core<'_, '_>, line: &Line) -> Answer {
    let Some(id) = line.here.clone() else {
        return Ok(Err(core.no_hamlet()));
    };
    found!(core.chief_of(&id));
    let folder = match line.words().first() {
        None => return usage(core, line),
        Some(&"none") => None,
        Some(_) => Some(line.rest.clone()),
    };
    if let Some(folder) = &folder
        && !std::path::Path::new(folder).is_dir()
    {
        return Ok(Err(Refusal::NoFile {
            path: folder.clone(),
        }));
    }
    let done = core.attempt(by(core), |cx, _| {
        hamlets::set_folder(cx, &id, folder.as_deref())
    })?;
    Ok(done.map(|()| match &folder {
        Some(folder) => format!("`{id}` works on {folder}."),
        None => format!("`{id}` works on no folder."),
    }))
}

fn mode(core: &mut Core<'_, '_>, line: &Line) -> Answer {
    let Some(id) = line.here.clone() else {
        return Ok(Err(core.no_hamlet()));
    };
    found!(core.chief_of(&id));
    let mode = match line.words().first() {
        Some(&"manual") => Mode::Manual,
        Some(&"auto") => Mode::Auto,
        _ => return usage(core, line),
    };
    let done = core.attempt(by(core), |cx, _| {
        hamlets::set_mode(cx, &id, mode)
    })?;
    Ok(done.map(|()| format!("`{id}` is in {mode} mode.")))
}

fn budget(core: &mut Core<'_, '_>, line: &Line) -> Answer {
    let db = core.realm.store().connection();
    let report = spend::report(
        db,
        line.here.as_ref(),
        crate::app::clock::now(),
    )?;
    Ok(Ok(report.summary))
}

fn reset_budget(core: &mut Core<'_, '_>, line: &Line) -> Answer {
    if let Some(id) = &line.here {
        found!(core.chief_of(id));
    }
    let chief = core.realm.chief();
    let at = crate::app::clock::now();
    let db = core.realm.store().connection();
    spend::reset(db, line.here.as_ref(), chief, at)?;
    core.publish();
    Ok(Ok("The budget's period starts again from now.".into()))
}

fn history(core: &mut Core<'_, '_>, line: &Line) -> Answer {
    let Some(id) = line.here.clone() else {
        return Ok(Err(core.no_hamlet()));
    };
    let db = core.realm.store().connection();
    Ok(Ok(record::lines(db, &id, 0)?))
}

fn list_profiles(core: &mut Core<'_, '_>, _: &Line) -> Answer {
    let db = core.realm.store().connection();
    let list = city::all(db)?
        .into_iter()
        .filter(|s| s.apart().is_none())
        .map(|s| {
            let archived =
                if s.is_living() { "" } else { " (archived)" };
            format!("- {}{archived}", s.label())
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok(Ok(if list.is_empty() {
        "The city has no profile yet.".into()
    } else {
        list
    }))
}

fn new_profile(core: &mut Core<'_, '_>, line: &Line) -> Answer {
    let Some(&"new") = line.words().first() else {
        return usage(core, line);
    };
    let chief = core.realm.chief();
    // The name runs to the first colon, the character follows it.
    let given = line.tail(1);
    let (profile, character) = given.split_once(':').map_or_else(
        || (given.clone(), String::new()),
        |(profile, character)| {
            (profile.to_owned(), character.trim().to_owned())
        },
    );
    let id = found!(core.attempt(by(core), |cx, _| {
        city::make(cx, &profile, chief, &character, None)
    }));
    let db = core.realm.store().connection();
    Ok(Ok(format!(
        "The profile `{}` lives in the city.",
        addresses::profile_name_of(db, id)?
    )))
}

fn name_profile(core: &mut Core<'_, '_>, line: &Line) -> Answer {
    let Some(text) = line.words().first().copied() else {
        return usage(core, line);
    };
    let profile = found!(core.profile_at(text, ProfileReach::Living));
    let by = by(core);
    let db = core.realm.store().connection();
    if !authority::may_keep(db, by, profile)? {
        return Ok(Err(Refusal::NotOwner {
            owner: addresses::profile_name_of(db, profile)?,
        }));
    }
    let new = line.tail(1);
    found!(core.attempt(by, |cx, _| city::rename(cx, profile, &new)));
    let db = core.realm.store().connection();
    Ok(Ok(format!(
        "The profile is now named `{}`.",
        addresses::profile_name_of(db, profile)?
    )))
}

/// The profile a profile command names, which the chief keeps, with
/// the name it is shown by.
fn kept_profile(
    core: &mut Core<'_, '_>,
    line: &Line,
    reach: ProfileReach,
) -> crate::realm::change::Done<(hamlet_store::ids::ProfileId, String)>
{
    let Some(text) = line.words().first().copied() else {
        return Ok(Err(core.deeds.usage(&line.name)));
    };
    let profile = found!(core.profile_at(text, reach));
    let by = by(core);
    let db = core.realm.store().connection();
    let shown = addresses::profile_name_of(db, profile)?;
    if !authority::may_keep(db, by, profile)? {
        return Ok(Err(Refusal::NotOwner { owner: shown }));
    }
    Ok(Ok((profile, shown)))
}

fn profile_fate(
    core: &mut Core<'_, '_>,
    line: &Line,
    housing: Housing,
) -> Answer {
    let reach =
        if matches!(housing, Housing::Restore | Housing::Erase) {
            ProfileReach::Kept
        } else {
            ProfileReach::Living
        };
    let (profile, text) = found!(kept_profile(core, line, reach));
    let reason = line.tail(1);
    let done = core.attempt(by(core), |cx, _| match housing {
        Housing::Remove => city::archive(cx, profile, &reason),
        Housing::Restore => city::restore(cx, profile, &reason),
        Housing::Welcome => city::welcome(cx, profile, &reason),
        Housing::TurnAway => city::turn_away(cx, profile, &reason),
        Housing::Erase => city::erase(cx, profile),
    })?;
    Ok(done.map(|()| match housing {
        Housing::Remove => {
            format!("`{text}` left the city, with its agents.")
        }
        Housing::Restore => format!("`{text}` is back in the city."),
        Housing::Welcome => {
            format!("`{text}` moved into its house in the city.")
        }
        Housing::TurnAway => format!("`{text}` waits at the harbor."),
        Housing::Erase => format!("`{text}` was erased for good."),
    }))
}

fn move_house(core: &mut Core<'_, '_>, line: &Line) -> Answer {
    let (profile, text) =
        found!(kept_profile(core, line, ProfileReach::Living));
    let words = line.words();
    let number =
        |at: usize| words.get(at).map(|w| w.parse::<f64>().ok());
    let (Some(Some(x)), Some(Some(z)), turn) =
        (number(1), number(2), number(3))
    else {
        return usage(core, line);
    };
    let Some(turn) = turn.map_or(Some(None), |t| {
        t.map(|degrees| Some(degrees.to_radians()))
    }) else {
        return usage(core, line);
    };
    let done = core.attempt(by(core), |cx, _| {
        city::place(cx, profile, x, z, turn)
    })?;
    Ok(
        done.map(|()| {
            format!("`{text}`'s house stands at {x}, {z}.")
        }),
    )
}
