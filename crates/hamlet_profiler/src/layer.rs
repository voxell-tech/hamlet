//! The tracing layer that records the spans Bevy's `trace` feature
//! opens: each frame, schedule and system, and what they open in
//! turn.

use std::cell::RefCell;
use std::fmt;
use std::time::Instant;

use bevy::log::tracing_subscriber::Layer;
use bevy::log::tracing_subscriber::layer::Context;
use bevy::log::tracing_subscriber::registry::LookupSpan;
use tracing::Subscriber;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id};

use crate::Profiler;
use crate::record::{Entry, Lane, NameId};

thread_local! {
    /// The spans this thread is in that are recorded, the innermost
    /// last. A cell because tracing calls the layer through `&self`
    /// from every thread.
    static OPEN: RefCell<Vec<Open>> = const { RefCell::new(Vec::new()) };
}

/// A recorded span this thread is in.
struct Open {
    span: Id,
    entry: Entry,
    start: Instant,
}

/// What a span is to the recorder, kept on the span.
#[derive(Clone, Copy)]
struct Tag {
    kind: Kind,
    name: NameId,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// The root of a lane's frame.
    Frame(Lane),
    /// The main world handed to the render thread.
    Extract,
    Schedule,
    System,
    Other,
}

/// Records into its [`Profiler`].
pub(crate) struct Recording(pub(crate) Profiler);

impl<S> Layer<S> for Recording
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(
        &self,
        attributes: &Attributes<'_>,
        id: &Id,
        context: Context<'_, S>,
    ) {
        let Some(span) = context.span(id) else {
            return;
        };
        let metadata = attributes.metadata();
        let mut fields = Fields::default();
        attributes.record(&mut fields);
        let value = fields.name.or(fields.first);
        let kind = match (metadata.name(), value.as_deref()) {
            ("update", _)
                if metadata.target().starts_with("bevy_app") =>
            {
                Kind::Frame(Lane::Main)
            }
            ("sub app", Some("RenderApp")) => {
                Kind::Frame(Lane::Render)
            }
            ("sub app", Some("RenderExtractApp")) => Kind::Extract,
            ("schedule", Some(_)) => Kind::Schedule,
            ("system", Some(_)) => Kind::System,
            _ => Kind::Other,
        };
        let name = match (kind, value) {
            (Kind::Schedule | Kind::System, Some(value)) => value,
            (_, Some(value)) => {
                format!("{} {value}", metadata.name())
            }
            (_, None) => metadata.name().to_string(),
        };
        let name = self.0.lock().intern(&name);
        span.extensions_mut().insert(Tag { kind, name });
    }

    fn on_enter(&self, id: &Id, context: Context<'_, S>) {
        if !self.0.is_recording() {
            return;
        }
        let start = Instant::now();
        let Some(tag) = context
            .span(id)
            .and_then(|span| span.extensions().get::<Tag>().copied())
        else {
            return;
        };
        OPEN.with_borrow_mut(|open| {
            let near = open.last().map(|open| open.entry);
            let mut recorder = self.0.lock();
            let entry = match (tag.kind, near) {
                (Kind::Frame(lane), None) => {
                    recorder.begin(lane, tag.name)
                }
                (Kind::System, near) => {
                    recorder.enter_system(tag.name, near)
                }
                // Spans on a thread outside any frame, as a task
                // pool's, are not the frame's.
                (_, None) => None,
                (Kind::Schedule, Some(near)) => {
                    recorder.enter_schedule(near, tag.name)
                }
                (Kind::Extract, Some(near)) => {
                    recorder.extract(near);
                    recorder.enter(near, tag.name)
                }
                (_, Some(near)) => recorder.enter(near, tag.name),
            };
            let Some(entry) = entry else {
                return;
            };
            open.push(Open {
                span: id.clone(),
                entry,
                start,
            });
            recorder.add_cost(entry, start.elapsed());
        });
    }

    fn on_exit(&self, id: &Id, _: Context<'_, S>) {
        let end = Instant::now();
        let Some(open) = OPEN.with_borrow_mut(|open| {
            let at =
                open.iter().rposition(|open| open.span == *id)?;
            Some(open.remove(at))
        }) else {
            return;
        };
        if !self.0.is_recording() {
            return;
        }
        let mut recorder = self.0.lock();
        let spiked = recorder.exit(open.entry, end - open.start);
        recorder.add_cost(open.entry, end.elapsed());
        drop(recorder);
        if spiked.is_some() {
            self.0.stop();
        }
    }
}

/// A span's `name` field, or its first other one.
#[derive(Default)]
struct Fields {
    name: Option<String>,
    first: Option<String>,
}

impl Fields {
    fn keep(&mut self, field: &Field, value: String) {
        if field.name() == "name" {
            self.name = Some(value);
        } else if self.first.is_none() {
            self.first = Some(value);
        }
    }
}

impl Visit for Fields {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.keep(field, value.to_string());
    }

    fn record_debug(
        &mut self,
        field: &Field,
        value: &dyn fmt::Debug,
    ) {
        self.keep(field, format!("{value:?}"));
    }
}
