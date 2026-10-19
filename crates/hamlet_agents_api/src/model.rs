//! The core's settings in force, the model they reach, the events
//! that change them, the catalogue of the models served and a model
//! call a plugin makes.

pub use hamlet_core::catalogue::ModelCatalogue;
pub use hamlet_core::core::{
    Link, ListModels, Listing, Model, SaveSettings, Settings, SignIn,
    Signing, TestConnection, Trial, TryModel,
};
pub use hamlet_core::model_call::{ModelCall, Role, call_model};
