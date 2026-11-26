//! The making of an image to fit a model's limits.

use hamlet_actor::Action;
use serde::{Deserialize, Serialize};

use super::files::Image;
use crate::Message;
use crate::address::{Agent, Workspace};
use crate::bytes::Base64;

/// Makes `image` fit `limits`: as it is when it does, else turned
/// upright, made smaller and encoded again until it does.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FitImage {
    /// A PNG, JPEG, WebP or GIF.
    pub image: Base64,
    pub limits: ImageLimits,
}

impl Action for FitImage {
    type Output = FitAnswer;
    const NAME: &'static str = "fit_image";
}

impl Message for FitImage {
    type To = Workspace;
    type From = Agent;
}

/// The most pixels of an image's width and height, and its most
/// bytes, that a model takes.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
pub struct ImageLimits {
    pub width: u32,
    pub height: u32,
    pub bytes: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FitAnswer {
    Image(Image),
    /// The image cannot be given to the model.
    Unusable {
        words: String,
    },
}
