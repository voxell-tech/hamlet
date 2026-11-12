// This file was changed for Hamlet. See NOTICE in this crate.
use std::marker::PhantomData;

use serde::Serialize;

use crate::error::{ActorError, ActorErrorKind};

/// Private marker type that proves an error was created through the
/// macro
#[derive(Debug)]
pub struct MacroMarker {
    pub _private: (),
}

/// Can only be created through the hamlet_actor_error macro.
#[derive(Debug)]
pub struct ActorErrorSchema {
    pub group: &'static str,
    pub code: &'static str,
    pub default_message: &'static str,
    #[allow(dead_code)]
    pub meta_type: Option<&'static str>,
    /// Private marker that ensures this was created by the macro
    pub _macro_marker: MacroMarker,
}

/// Can only be created through the hamlet_actor_error macro.
#[derive(Debug)]
pub struct ActorErrorSchemaWithMeta<T> {
    pub schema: ActorErrorSchema,
    pub message_fn: fn(&T) -> String,
    pub _phantom: PhantomData<T>,
}

impl ActorErrorSchema {
    /// Internal constructor for built-in errors only (like
    /// INTERNAL_ERROR)
    #[doc(hidden)]
    pub(crate) const fn __internal_new(
        group: &'static str,
        code: &'static str,
        message: &'static str,
    ) -> Self {
        Self {
            group,
            code,
            default_message: message,
            meta_type: None,
            _macro_marker: MacroMarker { _private: () },
        }
    }

    /// Builds an anyhow::Error from this schema
    pub fn build(&'static self) -> anyhow::Error {
        let error = ActorError {
            kind: ActorErrorKind::Static(self),
            meta: None,
            message: None,
            actor: None,
        };
        anyhow::Error::new(error)
    }

    pub(crate) fn build_internal(
        &'static self,
        error: &anyhow::Error,
    ) -> ActorError {
        ActorError::build_internal(error)
    }
}

impl<T: Serialize> ActorErrorSchemaWithMeta<T> {
    /// Builds an anyhow::Error from this schema with the provided
    /// metadata
    pub fn build_with(&'static self, meta: T) -> anyhow::Error {
        let message = (self.message_fn)(&meta);
        let meta_json = serde_json::value::to_raw_value(&meta).ok();

        let error = ActorError {
            kind: ActorErrorKind::Static(&self.schema),
            meta: meta_json,
            message: Some(message),
            actor: None,
        };
        anyhow::Error::new(error)
    }
}

impl From<&'static ActorErrorSchema> for ActorError {
    fn from(value: &'static ActorErrorSchema) -> Self {
        ActorError {
            kind: ActorErrorKind::Static(value),
            meta: None,
            message: None,
            actor: None,
        }
    }
}
