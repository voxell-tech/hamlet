//! What a retained panel costs the host, frame by frame: the calls
//! into the plugin, the calls it makes on its nodes, and the time
//! spent, at rest, on a hover, on a click and while a value moves.
//!
//! ```sh
//! cargo run --release -p hamlet_engine_harness --example retained_costs -- <home> engine/samples/fynix_counter
//! ```
//!
//! The home is made empty first and taken away after.

use std::path::{Path, PathBuf};
use std::time::Duration;

use hamlet_engine_grants::Answer;
use hamlet_engine_harness::Harness;
use hamlet_engine_wasm::{NodeOf, NodeStats};

const PANEL: &str = "hamlet.fynix_counter/counter";

fn main() -> Result<(), String> {
    let mut args = std::env::args().skip(1).map(PathBuf::from);
    let (Some(home), Some(folder)) = (args.next(), args.next())
    else {
        return Err("retained_costs <home> <plugin folder>".into());
    };
    let outcome = measure(&home, &folder);
    let _ = std::fs::remove_dir_all(&home);
    outcome
}

fn measure(home: &Path, folder: &Path) -> Result<(), String> {
    let mut harness = Harness::new(home);
    let id = harness.install(folder)?;
    harness.allow(id.as_str(), Answer::Allow)?;
    let before = harness.stats();
    let mounted = harness.until(|harness| {
        harness
            .outline(PANEL)
            .is_some_and(|outline| outline.contains("Count: 0"))
    });
    if !mounted {
        return Err("the panel was never built".into());
    }
    let world = harness.world();
    let nodes = world.query::<&NodeOf>().iter(world).count();
    report("mount", &harness.settle_from(before));
    println!("nodes: {nodes}");
    println!();

    report("idle", &harness.run_frames(120));

    harness.hover(PANEL, "Hover me", true)?;
    report("hover in", &harness.settle());
    harness.hover(PANEL, "Hover me", false)?;
    report("hover out", &harness.settle());

    harness.click(PANEL, "Add one")?;
    report("click", &harness.settle());

    harness.click(PANEL, "Fade")?;
    report("a fade", &harness.settle());
    Ok(())
}

/// What one frame cost.
#[derive(Clone, Copy, Default)]
struct Frame {
    stats: NodeStats,
    /// The whole update, every system of the engine.
    update: Duration,
}

/// The example's methods on [`Harness`].
trait CostsHarnessExt {
    fn stats(&mut self) -> NodeStats;

    /// Runs `count` frames: what each cost.
    fn run_frames(&mut self, count: usize) -> Vec<Frame>;

    /// Runs frames until ten in a row cost the plugin nothing: what
    /// each cost.
    fn settle(&mut self) -> Vec<Frame>;

    /// As [`settle`](Self::settle), the first frame counted from
    /// `before`.
    fn settle_from(&mut self, before: NodeStats) -> Vec<Frame>;
}

impl CostsHarnessExt for Harness {
    fn stats(&mut self) -> NodeStats {
        *self.world().resource::<NodeStats>()
    }

    fn run_frames(&mut self, count: usize) -> Vec<Frame> {
        let mut frames = Vec::new();
        for _ in 0..count {
            let before = self.stats();
            let update = self.timed_frame();
            frames.push(Frame {
                stats: self.stats().since(before),
                update,
            });
        }
        frames
    }

    fn settle(&mut self) -> Vec<Frame> {
        let before = self.stats();
        self.settle_from(before)
    }

    fn settle_from(&mut self, before: NodeStats) -> Vec<Frame> {
        let mut frames = Vec::new();
        let mut quiet = 0;
        let mut last = before;
        while quiet < 10 && frames.len() < 1_000 {
            let update = self.timed_frame();
            let now = self.stats();
            let frame = now.since(last);
            last = now;
            quiet = if frame.calls == 0 { quiet + 1 } else { 0 };
            frames.push(Frame {
                stats: frame,
                update,
            });
        }
        frames.truncate(frames.len().saturating_sub(10));
        frames
    }
}

/// The example's methods on [`NodeStats`].
trait CostsStatsExt {
    /// What was counted since `before`.
    fn since(self, before: NodeStats) -> NodeStats;
}

impl CostsStatsExt for NodeStats {
    fn since(self, before: NodeStats) -> NodeStats {
        NodeStats {
            calls: self.calls - before.calls,
            node_calls: self.node_calls - before.node_calls,
            changes: self.changes - before.changes,
            call_time: self
                .call_time
                .saturating_sub(before.call_time),
            apply_time: self
                .apply_time
                .saturating_sub(before.apply_time),
        }
    }
}

/// Prints the first frames of `frames` one by one, and the mean of
/// the rest.
fn report(what: &str, frames: &[Frame]) {
    println!("{what}:");
    let first = frames.len().min(3);
    for (at, frame) in frames[..first].iter().enumerate() {
        line(&format!("frame {at}"), 1.0, frame);
    }
    let rest = &frames[first..];
    if rest.is_empty() {
        return;
    }
    let sum =
        rest.iter().fold(Frame::default(), |sum, frame| Frame {
            stats: NodeStats {
                calls: sum.stats.calls + frame.stats.calls,
                node_calls: sum.stats.node_calls
                    + frame.stats.node_calls,
                changes: sum.stats.changes + frame.stats.changes,
                call_time: sum.stats.call_time
                    + frame.stats.call_time,
                apply_time: sum.stats.apply_time
                    + frame.stats.apply_time,
            },
            update: sum.update + frame.update,
        });
    line(
        &format!("mean of {} more", rest.len()),
        rest.len() as f64,
        &sum,
    );
}

fn line(what: &str, count: f64, frame: &Frame) {
    let stats = frame.stats;
    let mean = |value: u64| value as f64 / count;
    let micros = |time: Duration| time.as_secs_f64() * 1e6 / count;
    println!(
        "  {what}: {:.1} calls in, {:.1} calls on nodes ({:.1} \
         changes), {:.1} us in calls, {:.1} us building widgets, \
         {:.1} us update",
        mean(stats.calls),
        mean(stats.node_calls),
        mean(stats.changes),
        micros(stats.call_time),
        micros(stats.apply_time),
        micros(frame.update),
    );
}
