//! The workspace's making of an image to fit a model's limits.

use std::sync::Arc;

use anyhow::Result;
use hamlet_actor::{Ctx, Handler};
use hamlet_messages::address::Agent;
use hamlet_messages::bytes::Base64;
use hamlet_messages::sender::sender;
use hamlet_messages::workspace::files::Image;
use hamlet_messages::workspace::images::{FitAnswer, FitImage};
use hamlet_workspace_wire::image::{Bounds, Format, Unusable, fit};

use super::Workspace;

impl Handler<FitImage> for Workspace {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        asked: FitImage,
    ) -> Result<FitAnswer> {
        let Agent(_) = sender(&ctx, &asked)?;
        let FitImage {
            image: Base64(bytes),
            limits,
        } = asked;
        let bounds = Bounds {
            width: limits.width,
            height: limits.height,
            bytes: u64::from(limits.bytes),
        };
        let fitted = tokio::task::spawn_blocking(move || {
            let format =
                Format::detect(&bytes).ok_or(Unusable::NotAnImage)?;
            fit(&bytes, format, bounds)
        })
        .await?;
        Ok(match fitted {
            Ok(fitted) => FitAnswer::Image(Image {
                mime: fitted.format.mime().into(),
                data: Base64(fitted.bytes),
            }),
            Err(unusable) => FitAnswer::Unusable {
                words: unusable.to_string(),
            },
        })
    }
}
