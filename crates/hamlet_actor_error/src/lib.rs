// This file was changed for Hamlet. See NOTICE in this crate.
mod error;
mod schema;

pub use error::{
    ActorError, ActorErrorKind, ActorSpecifier,
    expose_internal_errors,
};
pub use hamlet_actor_error_macros::ActorError;
pub use schema::{
    ActorErrorSchema, ActorErrorSchemaWithMeta, MacroMarker,
};

pub static INTERNAL_ERROR: ActorErrorSchema =
    ActorErrorSchema::__internal_new(
        "core",
        "internal_error",
        "An internal error occurred",
    );

#[doc(hidden)]
pub use indoc;
