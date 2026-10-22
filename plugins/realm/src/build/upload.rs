//! What the realm may spend: the bytes one call hands the engine, and
//! the time its building takes in a call and in a second. The engine
//! stops a plugin that hands over more than a call's bytes or keeps
//! going past a second's time, so the realm stays well within both.

use std::time::{Duration, Instant};

use hamlet_engine_sdk::scene::gpu::Buffer;
use hamlet_engine_sdk::scene::{Instances, Mesh, MeshData};

/// Bytes of meshes, buffers and copies one call hands over, of the
/// engine's `bytes_per_call`.
const CALL_BYTES: usize = 640 * 1024;

/// Time one call may build for, far within the engine's `call_time`,
/// and a second's calls together, so the frames stay smooth.
const CALL_TIME: Duration = Duration::from_millis(4);
const SECOND_TIME: Duration = Duration::from_millis(45);

/// What one call may still hand over and build for.
pub struct Upload {
    bytes: usize,
    until: Instant,
}

/// A mesh the engine was not handed.
pub enum Unsent {
    /// The call has no room left for it: hand it over in a later
    /// one.
    Full(MeshData),
    /// The engine refused it, saying why.
    Refused(String),
}

impl Upload {
    /// What a call begun now may spend, given what the second has
    /// spent.
    pub fn new(pace: &Pace) -> Self {
        let time = pace.left().min(CALL_TIME);
        Self {
            bytes: CALL_BYTES,
            until: Instant::now() + time,
        }
    }

    /// True while the call has time and bytes left to build with.
    pub fn left(&self) -> bool {
        self.bytes > 0 && Instant::now() < self.until
    }

    /// Hands `data` to the engine as a mesh.
    pub fn mesh(&mut self, data: MeshData) -> Result<Mesh, Unsent> {
        let bytes = 4
            * (data.positions.len()
                + data.normals.len()
                + data.uvs.len()
                + data.colors.len()
                + data.indices.len());
        if bytes > self.bytes {
            // A mesh larger than any call takes is refused, so that
            // it does not wait for ever.
            if bytes > CALL_BYTES {
                return Err(Unsent::Refused(format!(
                    "a mesh of {bytes} bytes is more than a call \
                     hands over"
                )));
            }
            self.bytes = 0;
            return Err(Unsent::Full(data));
        }
        self.bytes -= bytes;
        Mesh::build(&data).map_err(Unsent::Refused)
    }

    /// Writes `bytes` to the start of `buffer`. False when the call
    /// has no room left for them.
    pub fn write(
        &mut self,
        buffer: &Buffer,
        bytes: &[u8],
    ) -> Result<bool, String> {
        if bytes.len() > self.bytes {
            return Ok(false);
        }
        self.bytes -= bytes.len();
        buffer.write(0, bytes).map(|()| true)
    }

    /// Gives `instances` the copies of `floats`, ten each. False when
    /// the call has no room left for them.
    pub fn copies(
        &mut self,
        instances: &Instances,
        floats: &[f32],
    ) -> bool {
        let bytes = floats.len() * 4;
        if bytes > self.bytes {
            return false;
        }
        self.bytes -= bytes;
        if let Err(why) = instances.set(floats) {
            hamlet_engine_sdk::warn(&format!("copies: {why}"));
        }
        true
    }
}

/// The time the realm's calls spent building in the second under
/// way.
#[derive(Default)]
pub struct Pace {
    second: Option<Instant>,
    spent: Duration,
}

impl Pace {
    /// True when the second under way began more than a second ago.
    fn over(&self, now: Instant) -> bool {
        self.second.is_none_or(|second| {
            now - second >= Duration::from_secs(1)
        })
    }

    /// The time the second under way has left to build in.
    fn left(&self) -> Duration {
        if self.over(Instant::now()) {
            return SECOND_TIME;
        }
        SECOND_TIME.saturating_sub(self.spent)
    }

    /// Counts a call that began at `began` and ends now.
    pub fn spend(&mut self, began: Instant) {
        let now = Instant::now();
        if self.over(now) {
            self.second = Some(now);
            self.spent = Duration::ZERO;
        }
        self.spent += now - began;
    }
}
