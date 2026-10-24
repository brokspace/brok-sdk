//! Typed payloads for the well-known Brokspace topics.
//!
//! Every struct implements [`Payload`], which binds it to its topic so that
//! [`crate::Event::from_payload`] and [`crate::Event::decode`] stay type safe.
//! Shards are free to define their own payloads for their own topics.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// A serializable payload bound to a topic.
pub trait Payload: Serialize + DeserializeOwned {
    /// The topic this payload is published on.
    const TOPIC: &'static str;
}

macro_rules! payload {
    ($ty:ty => $topic:expr) => {
        impl Payload for $ty {
            const TOPIC: &'static str = $topic;
        }
    };
}

/// Well-known topic names.
pub mod topics {
    /// Workspace files changed on disk ([`super::FileChanged`]).
    pub const FILE_CHANGED: &str = "file:changed";
    /// Search request handled by the index service ([`super::SearchRequest`]).
    pub const INDEX_SEARCH: &str = "index:search";
    /// Search reply ([`super::SearchResults`]).
    pub const INDEX_SEARCH_RESULT: &str = "index:search.result";
    /// The index finished an update ([`super::IndexUpdated`]).
    pub const INDEX_UPDATED: &str = "index:updated";
    /// Question for the agent ([`super::AskRequest`]).
    pub const AGENT_ASK: &str = "agent:ask";
    /// Streamed answer fragment ([`super::AskChunk`]).
    pub const AGENT_ASK_CHUNK: &str = "agent:ask.chunk";
    /// Answer complete ([`super::AskDone`]).
    pub const AGENT_ASK_DONE: &str = "agent:ask.done";
    /// Answer failed ([`super::AskFailed`]).
    pub const AGENT_ASK_ERROR: &str = "agent:ask.error";
    /// Pre-indexation review outcome ([`super::ReviewResult`]).
    pub const REVIEW_RESULT: &str = "review:result";
    /// The runtime is shutting down ([`super::Shutdown`]).
    pub const SYSTEM_SHUTDOWN: &str = "system:shutdown";
}

/// Files changed in the workspace. Paths are relative to the workspace root
/// and always use `/` as separator.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileChanged {
    /// Added or modified files.
    #[serde(default)]
    pub changed: Vec<String>,
    /// Removed files.
    #[serde(default)]
    pub deleted: Vec<String>,
}
payload!(FileChanged => topics::FILE_CHANGED);

/// Hybrid search request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchRequest {
    /// Free text query.
    pub query: String,
    /// Maximum number of hits.
    #[serde(default = "default_limit")]
    pub limit: usize,
    /// Restrict (or prioritise) a partition, e.g. a submodule path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus: Option<String>,
}
payload!(SearchRequest => topics::INDEX_SEARCH);

fn default_limit() -> usize {
    8
}

/// A single search hit: one AST chunk plus its metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchHit {
    /// Stable chunk id.
    pub chunk_id: String,
    /// Partition (root workspace or submodule) the chunk belongs to.
    pub partition: String,
    /// Workspace relative file path.
    pub path: String,
    /// Detected language.
    pub language: String,
    /// Syntactic kind (`function`, `class`, `section`, ...).
    pub kind: String,
    /// Symbol name when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    /// Signature line(s) when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    /// 1-based first line.
    pub start_line: u32,
    /// 1-based last line (inclusive).
    pub end_line: u32,
    /// Fused relevance score (higher is better).
    pub score: f32,
    /// Chunk source text.
    pub content: String,
}

/// Reply to a [`SearchRequest`].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SearchResults {
    /// Ranked hits.
    pub hits: Vec<SearchHit>,
    /// Whether vector search contributed (false = BM25 only).
    #[serde(default)]
    pub semantic: bool,
}
payload!(SearchResults => topics::INDEX_SEARCH_RESULT);

/// Emitted after the index applied an update.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexUpdated {
    /// Files (re)indexed.
    pub files_indexed: usize,
    /// Files removed from the index.
    pub files_removed: usize,
    /// Chunks written.
    pub chunks: usize,
    /// Wall clock time spent.
    pub duration_ms: u64,
}
payload!(IndexUpdated => topics::INDEX_UPDATED);

/// A question for the agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AskRequest {
    /// The user prompt.
    pub prompt: String,
    /// LLM provider override (a key of `llm.providers` in `brok.yml`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// Number of context chunks to retrieve.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_chunks: Option<usize>,
    /// Run the request under a skill's instructions (`brok skill run`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill: Option<SkillInvocation>,
}
payload!(AskRequest => topics::AGENT_ASK);

/// A skill applied to an [`AskRequest`]: its instructions steer the model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillInvocation {
    /// Skill name, e.g. `code-review`.
    pub name: String,
    /// The body of the skill's `SKILL.md` (frontmatter removed), plus any
    /// reference files the runner chose to inline.
    pub instructions: String,
}

/// A streamed fragment of an answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AskChunk {
    /// Text to append to the answer.
    pub text: String,
}
payload!(AskChunk => topics::AGENT_ASK_CHUNK);

/// A source used to build the answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceRef {
    /// Workspace relative path.
    pub path: String,
    /// 1-based first line.
    pub start_line: u32,
    /// 1-based last line.
    pub end_line: u32,
    /// Symbol name when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
}

/// The answer finished successfully.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AskDone {
    /// Provider key used.
    pub provider: String,
    /// Model used.
    pub model: String,
    /// Context sources fed to the model.
    #[serde(default)]
    pub sources: Vec<SourceRef>,
}
payload!(AskDone => topics::AGENT_ASK_DONE);

/// The answer failed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AskFailed {
    /// What went wrong.
    pub message: String,
}
payload!(AskFailed => topics::AGENT_ASK_ERROR);

/// Verdict of a pre-indexation review.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReviewVerdict {
    /// Nothing to report.
    Pass,
    /// Worth a look.
    Warn,
    /// Likely defect or security problem.
    Fail,
}

/// Outcome of the agent's pre-indexation review of a file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewResult {
    /// Workspace relative path.
    pub path: String,
    /// Verdict.
    pub verdict: ReviewVerdict,
    /// Short human readable summary.
    pub summary: String,
}
payload!(ReviewResult => topics::REVIEW_RESULT);

/// The runtime is shutting down; shards should exit promptly.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shutdown {
    /// Optional reason.
    #[serde(default)]
    pub reason: String,
}
payload!(Shutdown => topics::SYSTEM_SHUTDOWN);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::topic::Topic;

    #[test]
    fn all_topics_are_valid() {
        for topic in [
            FileChanged::TOPIC,
            SearchRequest::TOPIC,
            SearchResults::TOPIC,
            IndexUpdated::TOPIC,
            AskRequest::TOPIC,
            AskChunk::TOPIC,
            AskDone::TOPIC,
            AskFailed::TOPIC,
            ReviewResult::TOPIC,
            Shutdown::TOPIC,
        ] {
            Topic::new(topic).unwrap();
        }
    }

    #[test]
    fn search_request_defaults() {
        let req: SearchRequest = serde_json::from_str(r#"{"query":"bus"}"#).unwrap();
        assert_eq!(req.limit, 8);
        assert_eq!(req.focus, None);
    }
}
