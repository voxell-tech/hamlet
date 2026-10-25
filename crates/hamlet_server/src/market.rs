//! The Agent Bazaar's server, where chiefs buy and
//! sell agents, hamlets and tools: a package bought settled into the
//! home as its parts come, what was bought, and what the chief packs
//! to sell. The window talks to that server itself; the server
//! handles only the home's side.

mod pack;
mod package;
mod settle;

use std::collections::HashMap;
use std::time::{Duration, Instant};

use anyhow::Result;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use hamlet_protocol::client::{
    PACKAGE_BYTES, PACKAGE_PART, PackagePart, Packing, ReadResult,
    RequestId, ServerMessage, Settle, SettlePart,
};
pub(crate) use pack::Unpacked;
pub(crate) use settle::Unsettled;

use crate::harness::Harnesses;
use crate::protocol::{Refusal, reply};
use crate::service::Service;

/// How long a package being received waits for its next part.
const PATIENCE: Duration = Duration::from_secs(120);

/// The packages being received, by their uploads.
#[derive(Default)]
pub(crate) struct Staging {
    pending: HashMap<String, Staged>,
}

/// A package whose parts are coming.
struct Staged {
    connection: u64,
    settle: Settle,
    bytes: Vec<u8>,
    touched: Instant,
}

impl Staging {
    /// Drops what connection `connection` was sending.
    pub fn gone(&mut self, connection: u64) {
        self.pending.retain(|_, s| s.connection != connection);
    }

    /// Drops the packages whose parts stopped coming.
    pub fn expire(&mut self) {
        self.pending.retain(|_, s| s.touched.elapsed() < PATIENCE);
    }
}

/// The answer to `settle` of connection `connection`: what it already
/// came to, or the upload its parts are sent to.
pub(crate) fn begin(
    service: &mut Service,
    connection: u64,
    settle: &Settle,
) -> Result<ServerMessage> {
    if let Some(settled) = settle::settled(service, settle)? {
        return Ok(reply(&settle.id, settled));
    }
    let hex = settle
        .sha256
        .bytes()
        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    if !hex || settle.size == 0 || settle.size > PACKAGE_BYTES {
        return Ok(reply(
            &settle.id,
            Err(Refusal::Invalid(
                "A settle names a package of at most 10 MB by its \
                 size and lowercase SHA-256."
                    .into(),
            )),
        ));
    }
    let upload = uuid::Uuid::new_v4().to_string();
    service.market.pending.insert(
        upload.clone(),
        Staged {
            connection,
            settle: settle.clone(),
            bytes: Vec::new(),
            touched: Instant::now(),
        },
    );
    let mut answer = reply(
        &settle.id,
        Ok("Ready for the package's parts.".into()),
    );
    if let ServerMessage::Reply(answer) = &mut answer {
        answer.upload = Some(upload);
    }
    Ok(answer)
}

/// The answer to `part` of connection `connection`: received, or,
/// with the last, what settling the package came to.
pub(crate) async fn part(
    service: &mut Service,
    harnesses: &mut Harnesses,
    connection: u64,
    part: &SettlePart,
) -> Result<ServerMessage> {
    let refused = |words: &str| {
        Ok(reply(&part.id, Err(Refusal::Invalid(words.into()))))
    };
    let Some(staged) = service
        .market
        .pending
        .get_mut(&part.upload)
        .filter(|staged| staged.connection == connection)
    else {
        return refused(
            "No package is being received under that upload. Settle \
             it again.",
        );
    };
    let data = match STANDARD.decode(&part.data) {
        Ok(data) if data.len() <= PACKAGE_PART => data,
        _ => {
            return refused(
                "A part holds at most 512 KiB of the package, in \
                 base64.",
            );
        }
    };
    let end = staged.bytes.len() as u64 + data.len() as u64;
    if part.offset != staged.bytes.len() as u64
        || end > staged.settle.size
    {
        service.market.pending.remove(&part.upload);
        return refused(
            "That part does not follow the parts before it. Settle \
             the package again.",
        );
    }
    staged.bytes.extend(data);
    staged.touched = Instant::now();
    if end < staged.settle.size {
        return Ok(reply(&part.id, Ok("Received the part.".into())));
    }
    let Some(staged) = service.market.pending.remove(&part.upload)
    else {
        return refused("The package went missing.");
    };
    let settled = settle::settle(
        service,
        harnesses,
        &staged.settle,
        &staged.bytes,
    )
    .await?;
    Ok(reply(&part.id, settled))
}

/// The answer to a `bought` read.
pub(crate) fn bought(
    service: &Service,
    id: &RequestId,
) -> Result<ServerMessage> {
    Ok(ServerMessage::Read(ReadResult::Bought {
        bought: settle::all_bought(service.db())?,
        to: id.clone(),
    }))
}

/// The answer to a `tools` read.
pub(crate) fn tools(
    service: &Service,
    id: &RequestId,
) -> ServerMessage {
    ServerMessage::Read(ReadResult::Tools {
        tools: pack::tools(service.home.layout().root()),
        to: id.clone(),
    })
}

/// The answers to a `package` read: the package in parts, in order,
/// or why it was not packed.
pub(crate) async fn package(
    service: &mut Service,
    id: &RequestId,
    packing: &Packing,
) -> Result<Vec<ServerMessage>> {
    let bytes = match pack::pack(service, packing).await? {
        Ok(bytes) => bytes,
        Err(refusal) => return Ok(vec![reply(id, Err(refusal))]),
    };
    let size = bytes.len() as u64;
    Ok(bytes
        .chunks(PACKAGE_PART)
        .scan(0_u64, |offset, chunk| {
            let part = PackagePart {
                data: STANDARD.encode(chunk),
                offset: *offset,
                size,
            };
            *offset += chunk.len() as u64;
            Some(ServerMessage::Read(ReadResult::Package {
                package: part,
                to: id.clone(),
            }))
        })
        .collect())
}

#[cfg(test)]
mod tests;
