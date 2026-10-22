//! The limits and the paths a sandboxed command is held to.

use std::path::PathBuf;

use sha2::{Digest, Sha256};

/// The resources one command may use.
#[derive(Debug, Clone)]
pub struct Limits {
    pub cpu_secs: u64,
    pub wall_secs: u64,
    pub max_output_bytes: usize,
    pub max_procs: u64,
    pub max_file_mb: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            cpu_secs: 60,
            wall_secs: 120,
            max_output_bytes: 64 * 1024,
            max_procs: 4096,
            max_file_mb: 512,
        }
    }
}
/// The reads, writes and reach of a sandboxed command.
#[derive(Debug, Clone, Default)]
pub struct Policy {
    /// Paths mounted read-only at the same location.
    pub ro: Vec<PathBuf>,
    /// Paths mounted read-write at the same location.
    pub rw: Vec<PathBuf>,
    /// The whole machine can be read, but for `hidden`.
    pub open: bool,
    /// Paths that stay unreadable when `open`. A path of `ro` or
    /// `rw` inside one is still granted.
    pub hidden: Vec<PathBuf>,
    /// Paths inside `rw` that cannot be written, renamed or removed.
    pub locked: Vec<PathBuf>,
    pub net: bool,
    /// The complete environment; everything else is cleared.
    pub env: Vec<(String, String)>,
    pub limits: Limits,
}

impl Policy {
    /// Short stable digest of the policy and sandbox name, for audit
    /// logs.
    pub fn hash(&self, sandbox: &str) -> String {
        let mut s = String::new();
        let mut field = |tag: &str, v: &str| {
            s.push_str(&format!("{tag}={}:{v}\n", v.len()));
        };
        field("sandbox", sandbox);
        for p in &self.ro {
            field("ro", &p.to_string_lossy());
        }
        for p in &self.rw {
            field("rw", &p.to_string_lossy());
        }
        if self.open {
            field("open", "true");
        }
        for p in &self.hidden {
            field("hidden", &p.to_string_lossy());
        }
        for p in &self.locked {
            field("locked", &p.to_string_lossy());
        }
        field("net", &self.net.to_string());
        for (k, v) in &self.env {
            field("envk", k);
            field("envv", v);
        }
        let l = &self.limits;
        field("cpu", &l.cpu_secs.to_string());
        field("wall", &l.wall_secs.to_string());
        field("out", &l.max_output_bytes.to_string());
        field("procs", &l.max_procs.to_string());
        field("file", &l.max_file_mb.to_string());
        let digest = Sha256::digest(s.as_bytes());
        digest.iter().take(6).map(|b| format!("{b:02x}")).collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::tests::policy;

    #[test]
    fn hash_is_stable_and_sensitive() {
        let p = policy();
        let h = p.hash("bwrap");
        assert_eq!(h.len(), 12);
        assert_eq!(h, p.hash("bwrap"));
        assert_ne!(h, p.hash("unsandboxed"));
        let mut q = p.clone();
        q.net = true;
        assert_ne!(h, q.hash("bwrap"));
        let mut q = p.clone();
        q.limits.cpu_secs += 1;
        assert_ne!(h, q.hash("bwrap"));
    }
}
