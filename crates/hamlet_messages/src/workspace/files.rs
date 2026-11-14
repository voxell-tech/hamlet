//! The calls that read an agent's project files through the
//! workspace, and their answer.
//!
//! Paths arrive absolute. A path in an agent's own filesystem goes to
//! that agent, never here.

use std::path::PathBuf;

use hamlet_actor::Action;
use serde::{Deserialize, Serialize};

use super::{AgentNumber, CallRefusal};

/// Reads a file: a text file's lines, or an image.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadFile {
    pub agent: AgentNumber,
    pub path: PathBuf,
    /// The first line to read, from 1.
    pub offset: Option<u64>,
    /// The most lines to read.
    pub limit: Option<u64>,
}

impl Action for ReadFile {
    type Output = FileAnswer;
    const NAME: &'static str = "read_file";
}

/// Finds the lines that match a regular expression in the text files
/// under a folder, or in one file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Search {
    pub agent: AgentNumber,
    pub under: PathBuf,
    pub pattern: String,
    /// A file name pattern, with `*` and `?`, that the files must
    /// match: their name, or their path under `under` when it holds
    /// a `/`.
    pub name: Option<String>,
    pub ignore_case: bool,
}

impl Action for Search {
    type Output = FileAnswer;
    const NAME: &'static str = "search";
}

/// Lists a folder, or finds the files under it by name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Find {
    pub agent: AgentNumber,
    pub under: PathBuf,
    /// As [`Search::name`]. Without it, the folder's own entries.
    pub name: Option<String>,
}

impl Action for Find {
    type Output = FileAnswer;
    const NAME: &'static str = "find";
}

/// The workspace's answer to a [`ReadFile`], a [`Search`] or a
/// [`Find`]. Every word in it has the realm's secrets taken out.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileAnswer {
    Text(Text),
    Image(Image),
    Refused(CallRefusal),
    /// The file could not be read, or the request could not be
    /// carried out, in words for the model.
    Failed(String),
}

/// What a read, a search or a find gives the model.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Text {
    pub text: String,
    pub notes: Vec<Note>,
    /// Set when a read left lines out.
    pub truncation: Option<Truncation>,
}

/// A remark beside a [`Text`], such as where a read was cut.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    pub severity: Severity,
    /// Empty for a remark with no code.
    pub code: String,
    pub message: String,
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
pub enum Severity {
    Info,
    Warn,
}

/// How a read was cut to its first lines.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Truncation {
    pub by: Option<CutBy>,
    pub total_lines: u64,
    pub total_bytes: u64,
    pub output_lines: u64,
    pub output_bytes: u64,
    pub first_line_exceeds_limit: bool,
}

/// The limit a read reached first.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
pub enum CutBy {
    Lines,
    Bytes,
}

/// An image file as it is on disk. The agent fits it to its model.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Image {
    /// As `image/png`.
    pub mime: String,
    pub bytes: Vec<u8>,
}
