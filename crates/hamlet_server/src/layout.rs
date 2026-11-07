//! Where everything of a realm lives in its folder.

use std::path::{Path, PathBuf};

use hamlet_store::ids::{AgentId, HamletId, ScholarId};

/// The paths of a realm's folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Layout {
    root: PathBuf,
}

impl Layout {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_owned(),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn database(&self) -> PathBuf {
        self.root.join("realm.sqlite")
    }

    /// The realm's identity, also kept in its database.
    pub fn identity(&self) -> PathBuf {
        self.root.join("realm.id")
    }

    pub fn models(&self) -> PathBuf {
        self.root.join("models.json")
    }

    /// The running server's address, for the CLI.
    pub fn server(&self) -> PathBuf {
        self.root.join("server.json")
    }

    /// Each runtime's own store.
    pub fn harnesses(&self) -> PathBuf {
        self.root.join("harnesses")
    }

    pub fn scholars(&self) -> PathBuf {
        self.root.join("scholars")
    }

    /// A scholar's repository, the Hand's included.
    pub fn repository(&self, scholar: ScholarId) -> PathBuf {
        self.scholars().join(format!("{scholar}.git"))
    }

    /// The Hand's lodge.
    pub fn hand(&self) -> PathBuf {
        self.root.join("hand")
    }

    pub fn hamlets(&self) -> PathBuf {
        self.root.join("hamlets")
    }

    /// The folder of a hamlet's lodges.
    pub fn hamlet(&self, hamlet: &HamletId) -> PathBuf {
        self.hamlets().join(hamlet.as_str())
    }

    /// A hire's lodge, in its hamlet's folder under its agent's id.
    pub fn lodge(&self, hamlet: &HamletId, hire: AgentId) -> PathBuf {
        self.hamlet(hamlet).join(hire.to_string())
    }

    /// The folder of the workers' scratch folders.
    pub fn workers(&self) -> PathBuf {
        self.root.join("workers")
    }

    /// A worker's scratch folder while it runs.
    pub fn worker(&self, worker: AgentId) -> PathBuf {
        self.workers().join(worker.to_string())
    }

    /// The library's repository.
    pub fn library_repository(&self) -> PathBuf {
        self.root.join("library.git")
    }

    /// The library's checkout, read-only to agents.
    pub fn library(&self) -> PathBuf {
        self.root.join("library")
    }

    /// Tools a chief adds by hand (C17).
    pub fn tools(&self) -> PathBuf {
        self.root.join("tools")
    }

    /// The images chiefs sent and those the agents read, each kept
    /// once under the hash of its bytes, out of agents' reach (S29).
    pub fn images(&self) -> PathBuf {
        self.root.join("images")
    }

    /// Uploads and work in flight, out of agents' reach.
    pub fn staging(&self) -> PathBuf {
        self.root.join("staging")
    }
}
