//! The system interface a plugin is given, WASI 0.2, which grants
//! almost nothing: no files, no arguments, no sockets and no lookups,
//! and of the environment only the variables its manifest names. It
//! has the clocks and randomness, an empty stdin, and its stdout and
//! stderr go to its log.

use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::time::Instant;

use bevy::platform::cell::SyncCell;
use bytes::Bytes;
use hamlet_engine_core::LogLevel;
use tokio::io::AsyncWrite;
use wasmtime::component::{HasData, Linker, Resource, ResourceTable};
use wasmtime_wasi::cli::{IsTerminal, StdoutStream};
use wasmtime_wasi::clocks::{WasiClocksCtxView, WasiClocksView};
use wasmtime_wasi::p2::bindings::clocks::monotonic_clock;
use wasmtime_wasi::p2::pipe::SinkOutputStream;
use wasmtime_wasi::p2::{
    DynPollable, OutputStream, Pollable, StreamResult,
};
use wasmtime_wasi::{WasiCtx, WasiCtxView, WasiView};

use crate::instance::{Caps, State, keep};

/// The most bytes one write to stdout or stderr is kept of: the most
/// WASI's blocking write takes.
const CHUNK: usize = 4096;

/// The most writes to stdout and stderr one call keeps, whatever its
/// limits: the channel holds room for each from the start.
const WRITES: usize = 1024;

/// Adds the system interface to `linker`, its waits cut at the
/// call's deadline.
pub(crate) fn add_to_linker(
    linker: &mut Linker<State>,
) -> wasmtime::Result<()> {
    wasmtime_wasi::p2::add_to_linker_sync(linker)?;
    linker.allow_shadowing(true);
    monotonic_clock::add_to_linker::<State, Clocked>(
        linker,
        State::clock,
    )
}

/// A plugin's system interface: its context, the resources it holds,
/// and what it wrote to stdout and stderr.
pub(crate) struct Wasi {
    // These are `Send` but not `Sync`, and the store is in a
    // component.
    ctx: SyncCell<WasiCtx>,
    table: SyncCell<ResourceTable>,
    written: SyncCell<Receiver<Written>>,
    /// The line begun and not yet ended at each level.
    lines: [Vec<u8>; 3],
    /// When the call under way must end, if its time can be told.
    deadline: Option<Instant>,
}

impl Wasi {
    /// A context that grants nothing but the environment `env`,
    /// whose stdout and stderr go to the log.
    pub(crate) fn new(caps: &Caps, env: &[(String, String)]) -> Self {
        let (sender, written) = sync_channel(WRITES);
        let stdout = Output {
            level: LogLevel::Info,
            sender: sender.clone(),
        };
        let stderr = Output {
            level: LogLevel::Warn,
            sender,
        };
        let mut builder = WasiCtx::builder();
        for (name, value) in env {
            builder.env(name, value);
        }
        let ctx = builder
            .stdout(stdout)
            .stderr(stderr)
            .allow_tcp(false)
            .allow_udp(false)
            .allow_ip_name_lookup(false)
            // The host makes the bytes before they reach the plugin's
            // memory, which holds no more. wasmtime-wasi has no way
            // to change it once built, so a new limit
            // reaches it in the plugin's next store, as
            // it starts or is reloaded.
            .max_random_size(caps.bytes.min(caps.memory) as u64)
            .build();
        Self {
            ctx: SyncCell::new(ctx),
            table: SyncCell::new(ResourceTable::new()),
            written: SyncCell::new(written),
            lines: Default::default(),
            deadline: None,
        }
    }

    /// Holds the plugin to `caps` for the call about to be made.
    pub(crate) fn arm(&mut self, caps: &Caps) {
        self.deadline = Instant::now().checked_add(caps.time);
        self.table.get().set_max_capacity(caps.resources);
    }

    /// Moves what the plugin wrote since this was last asked into
    /// `logs`, a line each, within `caps`: `caps.lines` writes, and
    /// the rest left out.
    pub(crate) fn take_written(
        &mut self,
        caps: &Caps,
        logs: &mut Vec<(LogLevel, String)>,
    ) {
        let written = self.written.get();
        let mut kept = 0;
        while let Ok(Written { level, bytes }) = written.try_recv() {
            if kept == caps.lines {
                continue;
            }
            kept += 1;
            let line = &mut self.lines[level as usize];
            for piece in bytes.split_inclusive(|&byte| byte == b'\n')
            {
                let room = caps.line_bytes.saturating_sub(line.len());
                line.extend_from_slice(
                    &piece[..piece.len().min(room)],
                );
                if piece.ends_with(b"\n") {
                    end(level, line, caps, logs);
                }
            }
        }
        for level in [LogLevel::Info, LogLevel::Warn] {
            end(level, &mut self.lines[level as usize], caps, logs);
        }
    }
}

/// Ends `line` as a line of the log.
fn end(
    level: LogLevel,
    line: &mut Vec<u8>,
    caps: &Caps,
    logs: &mut Vec<(LogLevel, String)>,
) {
    let text = String::from_utf8_lossy(line).trim_end().to_string();
    line.clear();
    if !text.is_empty() {
        keep(logs, caps, level, text);
    }
}

impl WasiView for State {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        let wasi = &mut self.wasi;
        WasiCtxView {
            ctx: wasi.ctx.get(),
            table: wasi.table.get(),
        }
    }
}

/// One write to stdout or stderr.
struct Written {
    level: LogLevel,
    bytes: Bytes,
}

/// Stdout or stderr. A write past what a call keeps is left out, and
/// the plugin is not told.
#[derive(Clone)]
struct Output {
    level: LogLevel,
    sender: SyncSender<Written>,
}

impl IsTerminal for Output {
    fn is_terminal(&self) -> bool {
        false
    }
}

impl StdoutStream for Output {
    fn p2_stream(&self) -> Box<dyn OutputStream> {
        Box::new(self.clone())
    }

    // Only WASI 0.2 is linked, which never asks for this.
    fn async_stream(&self) -> Box<dyn AsyncWrite + Send + Sync> {
        SinkOutputStream.async_stream()
    }
}

#[wasmtime_wasi::async_trait]
impl Pollable for Output {
    async fn ready(&mut self) {}
}

impl OutputStream for Output {
    fn write(&mut self, bytes: Bytes) -> StreamResult<()> {
        // A copy, so the rest of a longer write is not kept with it.
        let _ = self.sender.try_send(Written {
            level: self.level,
            bytes: Bytes::copy_from_slice(
                &bytes[..bytes.len().min(CHUNK)],
            ),
        });
        Ok(())
    }

    fn flush(&mut self) -> StreamResult<()> {
        Ok(())
    }

    fn check_write(&mut self) -> StreamResult<usize> {
        Ok(CHUNK)
    }
}

/// The monotonic clock of a plugin in a call, whose waits end by the
/// call's deadline.
struct Clock<'a> {
    view: WasiClocksCtxView<'a>,
    deadline: Option<Instant>,
}

struct Clocked;

impl HasData for Clocked {
    type Data<'a> = Clock<'a>;
}

impl State {
    /// The plugin's monotonic clock in the call.
    fn clock(&mut self) -> Clock<'_> {
        let deadline = self.wasi.deadline;
        Clock {
            view: self.clocks(),
            deadline,
        }
    }
}

impl Clock<'_> {
    /// A wait of `nanos`, cut at the deadline.
    fn wait(
        &mut self,
        nanos: u64,
    ) -> wasmtime::Result<Resource<DynPollable>> {
        let left = self.deadline.map_or(u64::MAX, |deadline| {
            let left =
                deadline.saturating_duration_since(Instant::now());
            u64::try_from(left.as_nanos()).unwrap_or(u64::MAX)
        });
        monotonic_clock::Host::subscribe_duration(
            &mut self.view,
            nanos.min(left),
        )
    }
}

impl monotonic_clock::Host for Clock<'_> {
    fn now(&mut self) -> wasmtime::Result<u64> {
        monotonic_clock::Host::now(&mut self.view)
    }

    fn resolution(&mut self) -> wasmtime::Result<u64> {
        monotonic_clock::Host::resolution(&mut self.view)
    }

    fn subscribe_instant(
        &mut self,
        when: u64,
    ) -> wasmtime::Result<Resource<DynPollable>> {
        let now = monotonic_clock::Host::now(&mut self.view)?;
        self.wait(when.saturating_sub(now))
    }

    fn subscribe_duration(
        &mut self,
        when: u64,
    ) -> wasmtime::Result<Resource<DynPollable>> {
        self.wait(when)
    }
}
