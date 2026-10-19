//! Running a script against a plugin, and the transcript of it.

use std::path::Path;

use crate::harness::Harness;
use crate::outline;
use crate::script::{Picture, Script, Step};

/// What running a script left.
pub struct Report {
    /// The transcript: each step, then the tree of each panel when
    /// it changed and what the plugin logged.
    pub lines: Vec<String>,
    /// The number of the step that failed, from 1, if one did. The
    /// steps after it were not run.
    pub failed: Option<usize>,
}

/// The script a plugin is put through when it is given none: two
/// frames, then a picture of each panel it declared. With several
/// plugins, each is put in front for its pictures.
pub fn plain(harness: &mut Harness) -> Script {
    let several = harness.ids().len() > 1;
    let mut steps = vec![Step::Tick(2)];
    for id in harness.ids().to_vec() {
        if several {
            steps.push(Step::Plugin(id.clone()));
        }
        for panel in harness.panels_of(&id) {
            steps.push(Step::Render(Picture {
                file: match several {
                    true => format!("{id}-{panel}.png"),
                    false => format!("{panel}.png"),
                },
                panel,
            }));
        }
    }
    Script { steps }
}

/// Puts the plugin through `script`, drawing pictures under `out`.
pub fn run(
    harness: &mut Harness,
    script: &Script,
    out: &Path,
) -> Report {
    let mut said = 0;
    let mut seen = Vec::new();
    let mut lines = harness
        .ids()
        .iter()
        .map(|id| format!("plugin {id}"))
        .collect::<Vec<_>>();
    show(harness, &mut lines, &mut seen, &mut said);
    for (at, step) in script.steps.iter().enumerate() {
        let number = at + 1;
        lines.push(format!("step {number}: {}", describe(step)));
        let done = apply(harness, step, out, &mut lines);
        show(harness, &mut lines, &mut seen, &mut said);
        if let Err(why) = done {
            lines.push(format!("FAILED step {number}: {why}"));
            return Report {
                lines,
                failed: Some(number),
            };
        }
    }
    Report {
        lines,
        failed: None,
    }
}

fn describe(step: &Step) -> String {
    match step {
        Step::Plugin(id) => format!("plugin {id}"),
        Step::Tick(frames) => format!("tick {frames}"),
        Step::Click(t) => format!("click {}/{}", t.panel, t.node),
        Step::Type(t) => {
            format!("type {}/{} {:?}", t.panel, t.node, t.text)
        }
        Step::Toggle(t) => format!("toggle {}/{}", t.panel, t.node),
        Step::RunMenu(label) => format!("run-menu {label:?}"),
        Step::Press(chord) => format!("press {chord}"),
        Step::SetAgents(agents) => {
            format!("set-agents ({} agents)", agents.len())
        }
        Step::SetPapers(papers) => {
            format!(
                "set-papers ({} bytes of notes)",
                papers.notes.len()
            )
        }
        Step::SetSelected(name) => format!("set-selected {name:?}"),
        Step::Grant(capability) => format!("grant {capability}"),
        Step::Deny(capability) => format!("deny {capability}"),
        Step::Restart(_) => "restart".into(),
        Step::Render(p) => {
            format!("render {} to {}", p.panel, p.file)
        }
        Step::Context(c) => {
            format!("context {} {:?}", c.subject, c.item)
        }
        Step::Section(h) => {
            format!("section {:?} to {}", h.title, h.file)
        }
        Step::Page(h) => {
            format!("page {:?} to {}", h.title, h.file)
        }
        Step::Expect(e) => {
            format!("expect {} shows {:?}", e.panel, e.text)
        }
        Step::ExpectTold(text) => format!("expect-told {text:?}"),
        Step::CallTool(call) => format!("call-tool {}", call.tool),
    }
}

fn apply(
    harness: &mut Harness,
    step: &Step,
    out: &Path,
    lines: &mut Vec<String>,
) -> Result<(), String> {
    match step {
        Step::Plugin(id) => harness.select(id),
        Step::Tick(frames) => {
            (0..*frames).for_each(|_| harness.tick());
            Ok(())
        }
        Step::Click(t) => harness.click(&t.panel, &t.node),
        Step::Type(t) => {
            harness.type_text(&t.panel, &t.node, &t.text)
        }
        Step::Toggle(t) => harness.toggle(&t.panel, &t.node),
        Step::RunMenu(label) => harness.run_menu(label),
        Step::Press(chord) => harness.press(chord),
        Step::SetAgents(agents) => {
            let agents = agents
                .iter()
                .map(|a| (a.hamlet.as_str(), a.name.as_str()))
                .collect::<Vec<_>>();
            harness.set_agents(&agents);
            Ok(())
        }
        Step::SetPapers(papers) => {
            harness.set_papers(
                &papers.notes,
                &papers.character,
                &papers.skills,
            );
            Ok(())
        }
        Step::SetSelected(name) => {
            harness.set_selected(
                Some(name)
                    .filter(|n| !n.is_empty())
                    .map(String::as_str),
            );
            Ok(())
        }
        Step::Grant(capability) => harness.grant(capability),
        Step::Deny(capability) => harness.deny(capability),
        Step::Restart(true) => {
            harness.restart();
            Ok(())
        }
        Step::Restart(false) => {
            Err("`restart` is written `restart = true`".into())
        }
        Step::Render(p) => {
            let path = out.join(&p.file);
            harness.render(&p.panel, &path)?;
            lines.push(format!("wrote {}", path.display()));
            Ok(())
        }
        Step::Context(c) => {
            if let Some(file) = &c.file {
                let path = out.join(file);
                harness.render_context(&c.subject, &path)?;
                lines.push(format!("wrote {}", path.display()));
            }
            harness.run_context(&c.subject, &c.item)
        }
        Step::Section(h) => {
            let path = out.join(&h.file);
            harness.render_section(&h.title, &path)?;
            lines.push(format!("wrote {}", path.display()));
            Ok(())
        }
        Step::Page(h) => {
            let path = out.join(&h.file);
            harness.render_page(&h.title, &path)?;
            lines.push(format!("wrote {}", path.display()));
            Ok(())
        }
        Step::Expect(e) => {
            let shows = harness
                .tree(&e.panel)
                .is_some_and(|root| outline::shows(&root, &e.text));
            match shows {
                true => Ok(()),
                false => Err(format!(
                    "panel `{}` does not show {:?}",
                    e.panel, e.text
                )),
            }
        }
        Step::CallTool(call) if !call.offered => {
            match harness.offers(&call.hamlet, &call.tool) {
                false => Ok(()),
                true => Err(format!(
                    "`{}` is offered to the agents of {}",
                    call.tool, call.hamlet
                )),
            }
        }
        Step::CallTool(call) => {
            let done = harness.call_tool(
                &call.hamlet,
                &call.tool,
                &call.args,
            );
            let (failed, text) = match done {
                Ok(text) => (false, text),
                Err(why) => (true, why),
            };
            lines.push(format!("  tool returned {text:?}"));
            match (failed == call.fails, text.contains(&call.shows)) {
                (true, true) => Ok(()),
                (true, false) => Err(format!(
                    "the tool did not return {:?}",
                    call.shows
                )),
                (false, _) => Err(format!(
                    "the call {} ({text:?})",
                    if failed { "failed" } else { "did not fail" }
                )),
            }
        }
        Step::ExpectTold(text) => {
            match harness
                .told()
                .iter()
                .any(|told| told.contains(text))
            {
                true => Ok(()),
                false => Err(format!(
                    "the chief was not told {text:?}; the notices \
                     are {:?}",
                    harness.told()
                )),
            }
        }
    }
}

/// Adds what the plugins logged since the last look, and the panels
/// that look otherwise than they did. With several plugins a panel is
/// named with its plugin.
fn show(
    harness: &mut Harness,
    lines: &mut Vec<String>,
    seen: &mut Vec<(String, Vec<String>)>,
    said: &mut usize,
) {
    for line in &harness.logs()[*said..] {
        lines.push(format!("  log {line}"));
    }
    *said = harness.logs().len();
    let several = harness.ids().len() > 1;
    for id in harness.ids().to_vec() {
        let stopped = harness.stopped_of(&id);
        for panel in harness.panels_of(&id) {
            let now = match &stopped {
                Some(why) => vec![format!("stopped: {why}")],
                None => harness.outline_of(&id, &panel),
            };
            let name = match several {
                true => format!("{id}/{panel}"),
                false => panel,
            };
            let was =
                seen.iter().position(|(shown, _)| *shown == name);
            if was.is_some_and(|at| seen[at].1 == now) {
                continue;
            }
            lines.push(format!("  panel {name}"));
            lines
                .extend(now.iter().map(|line| format!("    {line}")));
            match was {
                Some(at) => seen[at].1 = now,
                None => seen.push((name, now)),
            }
        }
    }
}
