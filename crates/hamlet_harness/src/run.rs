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
/// frames, then a picture of each panel it declared.
pub fn plain(harness: &mut Harness) -> Script {
    let pictures = harness.panels().into_iter().map(|panel| {
        Step::Render(Picture {
            file: format!("{panel}.png"),
            panel,
        })
    });
    Script {
        steps: std::iter::once(Step::Tick(2))
            .chain(pictures)
            .collect(),
    }
}

/// Puts the plugin through `script`, drawing pictures under `out`.
pub fn run(
    harness: &mut Harness,
    script: &Script,
    out: &Path,
) -> Report {
    let mut said = 0;
    let mut seen = Vec::new();
    let mut lines = vec![format!("plugin {}", harness.id())];
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
        Step::Tick(frames) => format!("tick {frames}"),
        Step::Click(t) => format!("click {}/{}", t.panel, t.node),
        Step::Type(t) => {
            format!("type {}/{} {:?}", t.panel, t.node, t.text)
        }
        Step::Toggle(t) => format!("toggle {}/{}", t.panel, t.node),
        Step::SetAgents(agents) => {
            format!("set-agents ({} agents)", agents.len())
        }
        Step::SetPapers(papers) => {
            format!(
                "set-papers ({} bytes of notes)",
                papers.notes.len()
            )
        }
        Step::Grant(capability) => format!("grant {capability}"),
        Step::Deny(capability) => format!("deny {capability}"),
        Step::Render(p) => {
            format!("render {} to {}", p.panel, p.file)
        }
        Step::Expect(e) => {
            format!("expect {} shows {:?}", e.panel, e.text)
        }
    }
}

fn apply(
    harness: &mut Harness,
    step: &Step,
    out: &Path,
    lines: &mut Vec<String>,
) -> Result<(), String> {
    match step {
        Step::Tick(frames) => {
            (0..*frames).for_each(|_| harness.tick());
            Ok(())
        }
        Step::Click(t) => harness.click(&t.panel, &t.node),
        Step::Type(t) => {
            harness.type_text(&t.panel, &t.node, &t.text)
        }
        Step::Toggle(t) => harness.toggle(&t.panel, &t.node),
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
        Step::Grant(capability) => harness.grant(capability),
        Step::Deny(capability) => harness.deny(capability),
        Step::Render(p) => {
            let path = out.join(&p.file);
            harness.render(&p.panel, &path)?;
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
    }
}

/// Adds what the plugin logged since the last look, and the panels
/// that look otherwise than they did.
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
    let stopped = harness.stopped();
    for panel in harness.panels() {
        let now = match &stopped {
            Some(why) => vec![format!("stopped: {why}")],
            None => harness.outline(&panel),
        };
        let was = seen.iter().position(|(shown, _)| *shown == panel);
        if was.is_some_and(|at| seen[at].1 == now) {
            continue;
        }
        lines.push(format!("  panel {panel}"));
        lines.extend(now.iter().map(|line| format!("    {line}")));
        match was {
            Some(at) => seen[at].1 = now,
            None => seen.push((panel, now)),
        }
    }
}
