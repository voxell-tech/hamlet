//! The commands that shape the society: hamlets, hires and scholars.
//! Each is a function of its own, named in [`SOCIETY`] beside how it
//! is written.

use hamlet_store::ids::{Actor, HamletId};

use super::{Answer, BLANK, Command, Line, ScholarReach, is_scholar};
use crate::core::Core;
use crate::society::addresses::{self, Reach};
use crate::society::agents::{self, AgentKind, Effort, EndHow};
use crate::society::hamlets::{self, Founding, Mode};
use crate::society::hires::{self, Hiring, Spec};
use crate::society::refusal::Refusal;
use crate::society::{authority, city, record, roads, workers};
use crate::spend;

/// The commands that shape the society: hamlets, hires and scholars.
pub(super) const SOCIETY: &[Command] = &[
    Command {
        name: "hamlets",
        usage: "/hamlets",
        does: "List the hamlets",
        run: list_hamlets,
    },
    Command {
        name: "create-hamlet",
        usage: "/create-hamlet [scholar#id|blank] <name>[: purpose]",
        does: "Create a hamlet named <name> under a scholar as its \
               mayor, or a blank agent named Mayor if none is given",
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
        does: "Remove a hamlet and end its hires",
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
        does: "Set how many hires this hamlet holds",
        run: tier,
    },
    Command {
        name: "mayor",
        usage: "/mayor <address> [reason]",
        does: "Give this hamlet's office to a hire",
        run: mayor,
    },
    Command {
        name: "hire",
        usage: "/hire <scholar#id>[@<version>]|blank [in <hamlet>] \
                [under <address>] [purpose]",
        does: "Hire a scholar into this hamlet, or a blank agent \
               that is not a scholar of the city",
        run: hire,
    },
    Command {
        name: "end",
        usage: "/end <address> [reason]",
        does: "End a hire",
        run: |core, line| hire_fate(core, line, Fate::Remove),
    },
    Command {
        name: "restore",
        usage: "/restore <address> [reason]",
        does: "Bring an ended hire back",
        run: |core, line| hire_fate(core, line, Fate::Restore),
    },
    Command {
        name: "erase",
        usage: "/erase <address>",
        does: "Erase an ended hire for good",
        run: |core, line| hire_fate(core, line, Fate::Erase),
    },
    Command {
        name: "merge",
        usage: "/merge <address> into <address> [reason]",
        does: "Make one hire of two; /restore splits them again",
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
        name: "nickname",
        usage: "/nickname <address> [nickname]",
        does: "Call a hire or a worker something in the world, or \
               by its name again",
        run: nickname,
    },
    Command {
        name: "tools",
        usage: "/tools <address> -<tool> +<tool>",
        does: "Turn a hire's tools off or back on",
        run: tools,
    },
    Command {
        name: "allowance",
        usage: "/allowance <address> <dollars|none>",
        does: "Give a hire an allowance for the period",
        run: allowance,
    },
    Command {
        name: "road",
        usage: "/road <address> <address> [reason]",
        does: "Lay a road between two hires",
        run: |core, line| road(core, line, Roadwork::Lay),
    },
    Command {
        name: "unroad",
        usage: "/unroad <address> <address> [reason]",
        does: "Take up a road",
        run: |core, line| road(core, line, Roadwork::TakeUp),
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
        name: "scholars",
        usage: "/scholars",
        does: "List the city's scholars",
        run: list_scholars,
    },
    Command {
        name: "scholar",
        usage: "/scholar new <name>[: <character>]",
        does: "Make a scholar in the city; a name is a label with \
               no colon, and the character follows the colon",
        run: new_scholar,
    },
    Command {
        name: "name-scholar",
        usage: "/name-scholar <scholar#id> <name>",
        does: "Give a scholar a new name",
        run: name_scholar,
    },
    Command {
        name: "remove-scholar",
        usage: "/remove-scholar <scholar#id> [reason]",
        does: "Archive a scholar and end its hires",
        run: |core, line| scholar_fate(core, line, Housing::Remove),
    },
    Command {
        name: "restore-scholar",
        usage: "/restore-scholar <scholar#id> [reason]",
        does: "Bring an archived scholar back with its hires",
        run: |core, line| scholar_fate(core, line, Housing::Restore),
    },
    Command {
        name: "erase-scholar",
        usage: "/erase-scholar <scholar#id>",
        does: "Erase an archived scholar for good",
        run: |core, line| scholar_fate(core, line, Housing::Erase),
    },
    Command {
        name: "move-house",
        usage: "/move-house <scholar#id> <x> <z> [degrees]",
        does: "Set a scholar's house down elsewhere in the city",
        run: move_house,
    },
    Command {
        name: "welcome",
        usage: "/welcome <scholar#id> [reason]",
        does: "Let a scholar waiting at the harbor into the city",
        run: |core, line| scholar_fate(core, line, Housing::Welcome),
    },
    Command {
        name: "turn-away",
        usage: "/turn-away <scholar#id> [reason]",
        does: "Keep a scholar at the harbor, or send it back there",
        run: |core, line| scholar_fate(core, line, Housing::TurnAway),
    },
];

/// The fate a command gives a hamlet or a hire that may end.
#[derive(Clone, Copy, PartialEq)]
enum Fate {
    /// It ends, or the hamlet is removed.
    Remove,
    /// It comes back.
    Restore,
    /// It goes for good.
    Erase,
}

/// The place a command gives a scholar in the city.
#[derive(Clone, Copy)]
enum Housing {
    Remove,
    Restore,
    Erase,
    Welcome,
    TurnAway,
}

/// A command's work on a road.
#[derive(Clone, Copy)]
enum Roadwork {
    Lay,
    TakeUp,
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
    // A mayor not given is a blank agent named Mayor.
    let blank = || Hiring::Blank {
        name: String::new(),
        character: String::new(),
    };
    let (mayor, name_at) = match words.first() {
        Some(&BLANK) => (blank(), 1),
        Some(scholar) if is_scholar(scholar) => {
            (found!(core.hiring(scholar, chief)), 1)
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
        "" => format!("Mayor of {shown}."),
        given => given.to_owned(),
    };
    let hired = found!(core.attempt(by(core), |cx, _| {
        hamlets::found(
            cx,
            Founding {
                name: shown.clone(),
                chief,
                mayor,
                purpose,
            },
        )
    }));
    let db = core.realm.store().connection();
    Ok(Ok(format!(
        "The hamlet {shown} (`{}`) is created under its mayor `{}`.",
        hired.hamlet,
        addresses::name_of(db, hired.agent)?
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
                "The hamlet `{id}` was removed and its hires ended."
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
        format!("The hamlet `{id}` holds at most {tier} hires.")
    }))
}

fn mayor(core: &mut Core<'_, '_>, line: &Line) -> Answer {
    let Some(id) = line.here.clone() else {
        return Ok(Err(core.no_hamlet()));
    };
    let Some(text) = line.words().first().copied() else {
        return usage(core, line);
    };
    found!(core.chief_of(&id));
    let hire = found!(core.resolve(text, Some(&id), Reach::Living));
    let reason = line.tail(1);
    let done = core.attempt(by(core), |cx, _| {
        hires::seat(cx, &id, hire, &reason)
    })?;
    Ok(
        done.map(|()| {
            format!("`{text}` holds the office of `{id}`.")
        }),
    )
}

fn hire(core: &mut Core<'_, '_>, line: &Line) -> Answer {
    let chief = core.realm.chief();
    let words = line.words();
    let Some(scholar) = words.first() else {
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
            found!(hamlets::living(db, &hamlet)).mayor
        }
    };
    let hiring = if *scholar == BLANK {
        Hiring::Blank {
            name: String::new(),
            character: String::new(),
        }
    } else {
        found!(core.hiring(scholar, chief))
    };
    let hired = found!(core.attempt(Actor::Chief(chief), |cx, _| {
        hires::hire(
            cx,
            Spec {
                hamlet: hamlet.clone(),
                parent: Some(parent),
                hiring,
                purpose,
                reason: String::new(),
                owner: chief,
            },
        )
    }));
    let db = core.realm.store().connection();
    Ok(Ok(format!(
        "`{}` is hired.",
        addresses::name_of(db, hired.agent)?
    )))
}

fn hire_fate(
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
    let hire = found!(core.resolve(text, line.here.as_ref(), reach));
    found!(core.leads(hire));
    let reason = line.tail(1);
    let db = core.realm.store().connection();
    // A merged hire comes back by splitting its merge.
    let merge = match fate {
        Fate::Restore => {
            crate::society::merges::of_absorbed(db, hire)?
        }
        Fate::Remove | Fate::Erase => None,
    };
    let done =
        core.attempt(by(core), |cx, _| match (fate, &merge) {
            (Fate::Remove, _) => {
                hires::end(cx, hire, EndHow::Ended, &reason)
            }
            (Fate::Restore, Some(merge)) => {
                crate::society::merges::split(cx, merge.id, &reason)
            }
            (Fate::Restore, None) => {
                hires::come_back(cx, hire, &reason)
            }
            (Fate::Erase, _) => hires::erase(cx, hire),
        })?;
    Ok(done.map(|()| match fate {
        Fate::Remove => format!("`{text}` was ended."),
        Fate::Restore if merge.is_some() => format!(
            "`{text}` is back; what it brought to the hire it was \
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
    for hire in [absorbed, kept] {
        found!(core.leads(hire));
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
    let hire =
        found!(core.resolve(text, line.here.as_ref(), Reach::Living));
    found!(core.steers(hire));
    let model = (*model != "none").then_some(*model);
    let reason = line.tail(2);
    let done = core.attempt(by(core), |cx, _| {
        agents::set_model(cx, hire, model, &reason)
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
        format!("`{text}` asks for the fast tier: {value}.")
    }))
}

fn nickname(core: &mut Core<'_, '_>, line: &Line) -> Answer {
    let Some(text) = line.words().first().copied() else {
        return usage(core, line);
    };
    let agent =
        found!(core.resolve(text, line.here.as_ref(), Reach::Living));
    // A worker is named by whoever may steer it, a hire by its leads.
    let worker = {
        let db = core.realm.store().connection();
        agents::get(db, agent)?.kind == AgentKind::Worker
    };
    if worker {
        found!(core.steers(agent));
    } else {
        found!(core.leads(agent));
    }
    let nickname = line.tail(1);
    let given = (!nickname.is_empty()).then_some(nickname);
    let done = core.attempt(by(core), |cx, _| {
        if worker {
            workers::set_nickname(cx, agent, given.as_deref())
        } else {
            hires::set_nickname(cx, agent, given.as_deref(), "")
        }
    })?;
    Ok(done.map(|()| match &given {
        Some(nickname) => {
            format!("`{text}` is called {nickname} now.")
        }
        None => format!("`{text}` goes by its name again."),
    }))
}

fn tools(core: &mut Core<'_, '_>, line: &Line) -> Answer {
    let words = line.words();
    let Some(text) = words.first() else {
        return usage(core, line);
    };
    let hire =
        found!(core.resolve(text, line.here.as_ref(), Reach::Living));
    found!(core.leads(hire));
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
        let kept = hires::tools_off(db, hire)?;
        return Ok(Ok(if kept.is_empty() {
            format!("`{text}` has every tool.")
        } else {
            format!("`{text}` has these off: {}", kept.join(", "))
        }));
    }
    let done = core.attempt(by(core), |cx, _| {
        hires::set_tools(cx, hire, &off, &on, "")
    })?;
    Ok(done.map(|()| format!("`{text}`'s tools are changed.")))
}

fn allowance(core: &mut Core<'_, '_>, line: &Line) -> Answer {
    let words = line.words();
    let (Some(text), Some(amount)) = (words.first(), words.get(1))
    else {
        return usage(core, line);
    };
    let hire =
        found!(core.resolve(text, line.here.as_ref(), Reach::Living));
    found!(core.leads(hire));
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
        hires::set_allowance(cx, hire, allowance, "")
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

/// Lays a road between two hires, or takes it up.
fn road(
    core: &mut Core<'_, '_>,
    line: &Line,
    work: Roadwork,
) -> Answer {
    let words = line.words();
    let (Some(a), Some(b)) = (words.first(), words.get(1)) else {
        return usage(core, line);
    };
    let here = line.here.as_ref();
    let a = found!(core.resolve(a, here, Reach::Living));
    let b = found!(core.resolve(b, here, Reach::Living));
    for end in [a, b] {
        found!(core.leads(end));
    }
    let reason = line.tail(2);
    match work {
        Roadwork::Lay => {
            let done = core.attempt(by(core), |cx, _| {
                roads::lay(cx, a, b, &reason, None)
            })?;
            Ok(done.map(|_| "The road is laid.".into()))
        }
        Roadwork::TakeUp => {
            let done = core.attempt(by(core), |cx, _| {
                roads::take_up(cx, a, b, &reason)
            })?;
            Ok(done.map(|()| "The road is taken up.".into()))
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
    let report =
        spend::report(db, line.here.as_ref(), crate::clock::now())?;
    Ok(Ok(report.summary))
}

fn reset_budget(core: &mut Core<'_, '_>, line: &Line) -> Answer {
    if let Some(id) = &line.here {
        found!(core.chief_of(id));
    }
    let chief = core.realm.chief();
    let at = crate::clock::now();
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

fn list_scholars(core: &mut Core<'_, '_>, _: &Line) -> Answer {
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
        "The city has no scholar yet.".into()
    } else {
        list
    }))
}

fn new_scholar(core: &mut Core<'_, '_>, line: &Line) -> Answer {
    let Some(&"new") = line.words().first() else {
        return usage(core, line);
    };
    let chief = core.realm.chief();
    // The name runs to the first colon, the character follows it.
    let given = line.tail(1);
    let (scholar, character) = given.split_once(':').map_or_else(
        || (given.clone(), String::new()),
        |(scholar, character)| {
            (scholar.to_owned(), character.trim().to_owned())
        },
    );
    let id = found!(core.attempt(by(core), |cx, _| {
        city::make(cx, &scholar, chief, &character, None)
    }));
    let db = core.realm.store().connection();
    Ok(Ok(format!(
        "The scholar `{}` lives in the city.",
        addresses::scholar_name_of(db, id)?
    )))
}

fn name_scholar(core: &mut Core<'_, '_>, line: &Line) -> Answer {
    let Some(text) = line.words().first().copied() else {
        return usage(core, line);
    };
    let scholar = found!(core.scholar_at(text, ScholarReach::Living));
    let by = by(core);
    let db = core.realm.store().connection();
    if !authority::may_keep(db, by, scholar)? {
        return Ok(Err(Refusal::NotOwner {
            owner: addresses::scholar_name_of(db, scholar)?,
        }));
    }
    let new = line.tail(1);
    found!(core.attempt(by, |cx, _| city::rename(cx, scholar, &new)));
    let db = core.realm.store().connection();
    Ok(Ok(format!(
        "The scholar is now named `{}`.",
        addresses::scholar_name_of(db, scholar)?
    )))
}

/// The scholar a scholar command names, which the chief keeps, with
/// the name it is shown by.
fn kept_scholar(
    core: &mut Core<'_, '_>,
    line: &Line,
    reach: ScholarReach,
) -> crate::change::Done<(hamlet_store::ids::ScholarId, String)> {
    let Some(text) = line.words().first().copied() else {
        return Ok(Err(core.deeds.usage(&line.name)));
    };
    let scholar = found!(core.scholar_at(text, reach));
    let by = by(core);
    let db = core.realm.store().connection();
    let shown = addresses::scholar_name_of(db, scholar)?;
    if !authority::may_keep(db, by, scholar)? {
        return Ok(Err(Refusal::NotOwner { owner: shown }));
    }
    Ok(Ok((scholar, shown)))
}

fn scholar_fate(
    core: &mut Core<'_, '_>,
    line: &Line,
    housing: Housing,
) -> Answer {
    let reach =
        if matches!(housing, Housing::Restore | Housing::Erase) {
            ScholarReach::Kept
        } else {
            ScholarReach::Living
        };
    let (scholar, text) = found!(kept_scholar(core, line, reach));
    let reason = line.tail(1);
    let done = core.attempt(by(core), |cx, _| match housing {
        Housing::Remove => city::archive(cx, scholar, &reason),
        Housing::Restore => city::restore(cx, scholar, &reason),
        Housing::Welcome => city::welcome(cx, scholar, &reason),
        Housing::TurnAway => city::turn_away(cx, scholar, &reason),
        Housing::Erase => city::erase(cx, scholar),
    })?;
    Ok(done.map(|()| match housing {
        Housing::Remove => {
            format!("`{text}` left the city, with its hires.")
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
    let (scholar, text) =
        found!(kept_scholar(core, line, ScholarReach::Living));
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
        city::place(cx, scholar, x, z, turn)
    })?;
    Ok(
        done.map(|()| {
            format!("`{text}`'s house stands at {x}, {z}.")
        }),
    )
}
