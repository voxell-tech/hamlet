//! The frames recorded: each thread's spans as a tree, and the GPU's
//! time.

use std::collections::{HashMap, VecDeque};
use std::time::Duration;

/// How many frames are kept.
pub const KEPT: usize = 300;

/// The shortest span kept in a frame's tree. A shorter one's time
/// stays in its parent's self time.
pub const FLOOR: Duration = Duration::from_micros(10);

/// A thread whose work is recorded frame by frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lane {
    Main,
    Render,
}

impl Lane {
    fn at(self) -> usize {
        match self {
            Lane::Main => 0,
            Lane::Render => 1,
        }
    }
}

/// A span's name, read with [`Recorder::name`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct NameId(u32);

/// A span of a frame's tree, with every time it ran under its parent
/// summed.
#[derive(Clone, Copy, Debug)]
pub struct Span {
    pub name: NameId,
    /// The frame's root is at 0.
    pub depth: u16,
    pub total: Duration,
    /// The total less the children's, never below zero: systems run
    /// side by side under their schedule.
    pub self_time: Duration,
    pub calls: u32,
}

/// One thread's work in a frame.
#[derive(Clone, Debug)]
pub struct Timing {
    pub total: Duration,
    /// The time the recorder took on this thread's spans, on every
    /// thread they ran on.
    pub cost: Duration,
    /// The frame's tree depth first, the root first.
    pub spans: Vec<Span>,
}

/// One frame of the main thread, the render thread's work on what it
/// extracted, and the GPU's.
#[derive(Clone, Debug)]
pub struct Frame {
    pub number: u64,
    pub main: Option<Timing>,
    pub render: Option<Timing>,
    /// From the start of the frame's first render pass to the end of
    /// its last.
    pub gpu: Option<Duration>,
}

impl Frame {
    /// The longest of the main thread's, the render thread's and the
    /// GPU's time.
    pub fn longest(&self) -> Duration {
        let total = |timing: &Option<Timing>| {
            timing.as_ref().map_or(Duration::ZERO, |t| t.total)
        };
        total(&self.main)
            .max(total(&self.render))
            .max(self.gpu.unwrap_or_default())
    }

    pub fn timing(&self, lane: Lane) -> Option<&Timing> {
        match lane {
            Lane::Main => self.main.as_ref(),
            Lane::Render => self.render.as_ref(),
        }
    }
}

/// A span entered into a lane's frame: the frame it was entered in,
/// and its node there.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Entry {
    pub lane: Lane,
    generation: u32,
    node: u32,
}

/// The kept frames, and the ones being recorded.
#[derive(Default)]
pub struct Recorder {
    names: Vec<Box<str>>,
    ids: HashMap<Box<str>, NameId>,
    frames: VecDeque<Frame>,
    /// The number the next main frame takes.
    next: u64,
    /// The main frame the render thread was handed last.
    extracted: Option<u64>,
    lanes: [Live; 2],
    /// The schedules each system is in, by their names.
    schedules_of: HashMap<NameId, Vec<NameId>>,
    spike: Option<Duration>,
    spiked: Option<u64>,
    /// Kept here so a frame's tree is laid out without allocating.
    order: Vec<u32>,
    starts: Vec<u32>,
    walk: Vec<(u32, u16)>,
}

/// A lane's frame while it is recorded.
#[derive(Default)]
struct Live {
    open: bool,
    generation: u32,
    number: u64,
    nodes: Vec<Node>,
    children: HashMap<(u32, NameId), u32>,
    /// The schedules running, the innermost last, each with its
    /// node.
    schedules: Vec<(NameId, u32)>,
    cost: Duration,
}

/// A span of a frame being recorded.
struct Node {
    name: NameId,
    parent: u32,
    total: Duration,
    calls: u32,
}

impl Recorder {
    pub fn name(&self, id: NameId) -> &str {
        self.names.get(id.0 as usize).map_or("", |name| name)
    }

    /// The kept frames, the oldest first. The newest may be
    /// unfinished.
    pub fn frames(&self) -> impl DoubleEndedIterator<Item = &Frame> {
        self.frames.iter()
    }

    pub fn frame(&self, number: u64) -> Option<&Frame> {
        let first = self.frames.front()?.number;
        self.frames.get(number.checked_sub(first)? as usize)
    }

    /// The newest frame whose main and render threads are both
    /// recorded.
    pub fn latest(&self) -> Option<&Frame> {
        self.frames.iter().rev().find(|frame| {
            frame.main.is_some() && frame.render.is_some()
        })
    }

    /// The time over which a frame stops the recording.
    pub fn spike(&self) -> Option<Duration> {
        self.spike
    }

    pub(crate) fn set_spike(&mut self, spike: Option<Duration>) {
        self.spike = spike;
    }

    /// The frame that stopped the recording last.
    pub fn spiked(&self) -> Option<u64> {
        self.spiked
    }

    pub(crate) fn intern(&mut self, name: &str) -> NameId {
        if let Some(&id) = self.ids.get(name) {
            return id;
        }
        let id = NameId(self.names.len() as u32);
        self.names.push(name.into());
        self.ids.insert(name.into(), id);
        id
    }

    /// Notes that the systems named `systems` are in the schedule
    /// labelled `label`.
    pub(crate) fn add_schedule<'a>(
        &mut self,
        label: &str,
        systems: impl Iterator<Item = &'a str>,
    ) {
        let label = self.intern(label);
        for system in systems {
            let system = self.intern(system);
            let labels = self.schedules_of.entry(system).or_default();
            if !labels.contains(&label) {
                labels.push(label);
            }
        }
    }

    /// The number of the frame the render thread records.
    pub fn rendering(&self) -> Option<u64> {
        let live = &self.lanes[Lane::Render.at()];
        live.open.then_some(live.number)
    }

    /// Notes that the main frame `entry` is in hands its world to
    /// the render thread.
    pub(crate) fn extract(&mut self, entry: Entry) {
        if let Some(live) = self.live(entry) {
            self.extracted = Some(live.number);
        }
    }

    /// Starts recording `lane`'s next frame, under a root named
    /// `name`. The render thread's frame takes the number of the
    /// main frame it was handed.
    pub(crate) fn begin(
        &mut self,
        lane: Lane,
        name: NameId,
    ) -> Option<Entry> {
        let number = match lane {
            Lane::Main => {
                let number = self.next;
                self.next += 1;
                if self.frames.len() == KEPT {
                    self.frames.pop_front();
                }
                self.frames.push_back(Frame {
                    number,
                    main: None,
                    render: None,
                    gpu: None,
                });
                number
            }
            Lane::Render => self.extracted.take()?,
        };
        let live = &mut self.lanes[lane.at()];
        live.open = true;
        live.generation = live.generation.wrapping_add(1);
        live.number = number;
        live.nodes.clear();
        live.children.clear();
        live.schedules.clear();
        live.cost = Duration::ZERO;
        live.nodes.push(Node {
            name,
            parent: 0,
            total: Duration::ZERO,
            calls: 1,
        });
        Some(Entry {
            lane,
            generation: live.generation,
            node: 0,
        })
    }

    fn live(&mut self, entry: Entry) -> Option<&mut Live> {
        let live = &mut self.lanes[entry.lane.at()];
        (live.open && live.generation == entry.generation)
            .then_some(live)
    }

    /// Enters a span named `name` under `parent`.
    pub(crate) fn enter(
        &mut self,
        parent: Entry,
        name: NameId,
    ) -> Option<Entry> {
        let live = self.live(parent)?;
        let node = match live.children.get(&(parent.node, name)) {
            Some(&node) => node,
            None => {
                let node = live.nodes.len() as u32;
                live.nodes.push(Node {
                    name,
                    parent: parent.node,
                    total: Duration::ZERO,
                    calls: 0,
                });
                live.children.insert((parent.node, name), node);
                node
            }
        };
        live.nodes[node as usize].calls += 1;
        Some(Entry { node, ..parent })
    }

    /// Enters the schedule labelled `name` under `parent`.
    pub(crate) fn enter_schedule(
        &mut self,
        parent: Entry,
        name: NameId,
    ) -> Option<Entry> {
        let entry = self.enter(parent, name)?;
        self.lanes[entry.lane.at()]
            .schedules
            .push((name, entry.node));
        Some(entry)
    }

    /// Enters the system named `name` under the innermost running
    /// schedule it is in. Systems run on any thread, so the one it
    /// runs on says nothing; `near`, the span it was entered in, if
    /// any, picks between a schedule of each lane.
    pub(crate) fn enter_system(
        &mut self,
        name: NameId,
        near: Option<Entry>,
    ) -> Option<Entry> {
        let parent = self.schedule_of(name, near)?;
        self.enter(parent, name)
    }

    fn schedule_of(
        &self,
        system: NameId,
        near: Option<Entry>,
    ) -> Option<Entry> {
        let lanes = match near.map(|entry| entry.lane) {
            Some(Lane::Render) => [Lane::Render, Lane::Main],
            _ => [Lane::Main, Lane::Render],
        };
        let entry = |lane: Lane, node: u32| {
            let live = &self.lanes[lane.at()];
            live.open.then_some(Entry {
                lane,
                generation: live.generation,
                node,
            })
        };
        if let Some(labels) = self.schedules_of.get(&system) {
            for lane in lanes {
                let found = self.lanes[lane.at()]
                    .schedules
                    .iter()
                    .rev()
                    .find(|(label, _)| labels.contains(label));
                if let Some(&(_, node)) = found
                    && let Some(entry) = entry(lane, node)
                {
                    return Some(entry);
                }
            }
        }
        // Not in a schedule seen yet: the innermost running one.
        lanes.into_iter().find_map(|lane| {
            let &(_, node) =
                self.lanes[lane.at()].schedules.last()?;
            entry(lane, node)
        })
    }

    /// Leaves the span entered as `entry`, which took `took`. Leaving
    /// a root ends its frame, and gives the frame's number if it
    /// took longer than the spike allows.
    pub(crate) fn exit(
        &mut self,
        entry: Entry,
        took: Duration,
    ) -> Option<u64> {
        let live = self.live(entry)?;
        live.nodes[entry.node as usize].total += took;
        if live
            .schedules
            .last()
            .is_some_and(|&(_, node)| node == entry.node)
        {
            live.schedules.pop();
        }
        if entry.node != 0 {
            return None;
        }
        self.finish(entry.lane)
    }

    /// Adds `cost` to the recorder's own time in `entry`'s frame.
    pub(crate) fn add_cost(&mut self, entry: Entry, cost: Duration) {
        if let Some(live) = self.live(entry) {
            live.cost += cost;
        }
    }

    /// Sets the GPU's time of frame `number`, and gives the number
    /// if that is longer than the spike allows.
    pub(crate) fn set_gpu(
        &mut self,
        number: u64,
        took: Duration,
    ) -> Option<u64> {
        let first = self.frames.front()?.number;
        let frame = self
            .frames
            .get_mut(number.checked_sub(first)? as usize)?;
        frame.gpu = Some(took);
        self.check(number, took)
    }

    /// Stops the frames being recorded, and drops the one the main
    /// thread was in.
    pub(crate) fn close(&mut self) {
        for live in &mut self.lanes {
            live.open = false;
        }
        self.extracted = None;
        if self
            .frames
            .back()
            .is_some_and(|frame| frame.main.is_none())
        {
            self.frames.pop_back();
        }
    }

    pub(crate) fn resume(&mut self) {
        self.spiked = None;
    }

    fn check(&mut self, number: u64, took: Duration) -> Option<u64> {
        let over = self.spike.is_some_and(|spike| took > spike);
        if over {
            self.spiked = Some(number);
            self.close();
        }
        over.then_some(number)
    }

    /// Ends `lane`'s frame and keeps it.
    fn finish(&mut self, lane: Lane) -> Option<u64> {
        let timing = self.lay_out(lane);
        let live = &mut self.lanes[lane.at()];
        live.open = false;
        let number = live.number;
        let took = timing.total;
        let first = self.frames.front()?.number;
        let frame = self
            .frames
            .get_mut(number.checked_sub(first)? as usize)?;
        match lane {
            Lane::Main => frame.main = Some(timing),
            Lane::Render => frame.render = Some(timing),
        }
        self.check(number, took)
    }

    /// `lane`'s frame as a tree, depth first, without the spans
    /// shorter than [`FLOOR`].
    fn lay_out(&mut self, lane: Lane) -> Timing {
        let live = &self.lanes[lane.at()];
        let nodes = &live.nodes;
        // Each node's children, in the order they were first entered:
        // `order[starts[n]..starts[n + 1]]`.
        let (order, starts, walk) =
            (&mut self.order, &mut self.starts, &mut self.walk);
        starts.clear();
        starts.resize(nodes.len() + 1, 0);
        for node in &nodes[1..] {
            starts[node.parent as usize + 1] += 1;
        }
        for at in 1..starts.len() {
            starts[at] += starts[at - 1];
        }
        order.clear();
        order.resize(nodes.len().saturating_sub(1), 0);
        // Each start is moved along as its children are placed,
        // ending at the next one's, then all are moved back by one.
        for (at, node) in nodes.iter().enumerate().skip(1) {
            let slot = &mut starts[node.parent as usize];
            order[*slot as usize] = at as u32;
            *slot += 1;
        }
        starts.rotate_right(1);
        starts[0] = 0;
        let mut spans = Vec::new();
        walk.clear();
        walk.push((0, 0));
        while let Some((at, depth)) = walk.pop() {
            let node = &nodes[at as usize];
            let children = &order[starts[at as usize] as usize
                ..starts[at as usize + 1] as usize];
            let inside = children
                .iter()
                .map(|&child| nodes[child as usize].total)
                .sum::<Duration>();
            spans.push(Span {
                name: node.name,
                depth,
                total: node.total,
                self_time: node.total.saturating_sub(inside),
                calls: node.calls,
            });
            // Pushed last to first, so they come out first to last.
            for &child in children.iter().rev() {
                if nodes[child as usize].total >= FLOOR {
                    walk.push((child, depth + 1));
                }
            }
        }
        Timing {
            total: nodes[0].total,
            cost: live.cost,
            spans,
        }
    }
}
