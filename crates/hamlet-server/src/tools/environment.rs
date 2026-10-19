//! File operations in an agent's local or virtual filesystem.

use serde_json::json;

use super::Place;
use crate::sandbox::{Env, FileError, FileInfo};
use crate::workspace::Remote;

pub(super) enum Files<'a> {
    Local(&'a Env),
    Remote(&'a Remote),
}

impl Files<'_> {
    pub fn cwd(&self) -> &str {
        match self {
            Self::Local(env) => env.cwd(),
            Self::Remote(env) => env.cwd(),
        }
    }

    pub fn absolute(&self, path: &str) -> String {
        match self {
            Self::Local(env) => env.absolute(path),
            Self::Remote(env) => env.absolute(path),
        }
    }

    pub async fn footing(
        &self,
        path: &str,
        place: &Place,
    ) -> Result<String, FileError> {
        match self {
            Self::Local(_) => Ok(super::footed(path, place)),
            Self::Remote(env) => env.footing(path).await,
        }
    }

    pub async fn exists(
        &self,
        path: &str,
    ) -> Result<bool, FileError> {
        match self {
            Self::Local(env) => env.exists(path),
            Self::Remote(env) => env.exists(path).await,
        }
    }

    pub async fn read_file(
        &self,
        path: &str,
    ) -> Result<Vec<u8>, FileError> {
        match self {
            Self::Local(env) => env.read_file(path),
            Self::Remote(env) => env.read(path, false).await,
        }
    }

    pub async fn read_text(
        &self,
        path: &str,
    ) -> Result<String, FileError> {
        match self {
            Self::Local(env) => env.read_text(path),
            Self::Remote(env) => Ok(String::from_utf8_lossy(
                &env.read(path, false).await?,
            )
            .into_owned()),
        }
    }

    pub async fn readable(
        &self,
        path: &str,
    ) -> Result<(), FileError> {
        match self {
            Self::Local(env) => env.open_file(path).map(|_| ()),
            Self::Remote(env) => {
                env.read(path, false).await.map(|_| ())
            }
        }
    }

    pub async fn file_info(
        &self,
        path: &str,
    ) -> Result<FileInfo, FileError> {
        match self {
            Self::Local(env) => env.file_info(path),
            Self::Remote(env) => env.info(path).await,
        }
    }

    pub async fn list_dir(
        &self,
        path: &str,
    ) -> Result<Vec<FileInfo>, FileError> {
        match self {
            Self::Local(env) => env.list_dir(path),
            Self::Remote(env) => env.list(path).await,
        }
    }

    pub async fn canonical_path(
        &self,
        path: &str,
    ) -> Result<String, FileError> {
        match self {
            Self::Local(env) => env.canonical_path(path),
            Self::Remote(env) => env.call(json!({"kind":"canonical", "path":env.absolute(path)})).await.map_err(Into::into),
        }
    }

    pub async fn write_file(
        &self,
        path: &str,
        content: &str,
    ) -> Result<(), FileError> {
        match self {
            Self::Local(env) => env.write_file(path, content),
            Self::Remote(env) => {
                let parent = env.absolute(&format!("{path}/.."));
                env.call::<()>(json!({"kind":"mkdir", "path":parent, "recursive":true})).await?;
                env.write(path, content.as_bytes()).await
            }
        }
    }
}
