//! Topic addressing.
//!
//! Topics follow the `namespace:name` convention, e.g. `file:changed` or
//! `agent:ask.chunk`. Both parts use lowercase ASCII letters, digits, `-`, `_`
//! and `.` (the dot creates sub-topics).
//!
//! A [`TopicPattern`] is either an exact topic or a prefix terminated by `*`:
//!
//! | pattern        | matches                                  |
//! |----------------|------------------------------------------|
//! | `file:changed` | only `file:changed`                      |
//! | `agent:ask.*`  | `agent:ask.chunk`, `agent:ask.done`, ... |
//! | `file:*`       | every topic in the `file` namespace      |
//! | `*`            | everything                               |

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::TopicError;

/// Namespace reserved for events emitted by the `brok` runtime itself.
pub const SYSTEM_NAMESPACE: &str = "system";

const MAX_TOPIC_LEN: usize = 128;

fn valid_segment_char(c: char) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_' | '.')
}

fn err(value: &str, reason: &'static str) -> TopicError {
    TopicError { value: value.to_owned(), reason }
}

fn validate_topic(value: &str) -> Result<(), TopicError> {
    if value.is_empty() || value.len() > MAX_TOPIC_LEN {
        return Err(err(value, "must be between 1 and 128 characters"));
    }
    let Some((ns, name)) = value.split_once(':') else {
        return Err(err(value, "must look like `namespace:name`"));
    };
    if ns.is_empty() || name.is_empty() {
        return Err(err(value, "namespace and name must not be empty"));
    }
    if !ns.chars().chain(name.chars()).all(valid_segment_char) {
        return Err(err(value, "only lowercase letters, digits, `-`, `_` and `.` are allowed"));
    }
    Ok(())
}

/// A validated, concrete topic such as `file:changed`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Topic(String);

impl Topic {
    /// Validates and creates a topic.
    pub fn new(value: impl Into<String>) -> Result<Self, TopicError> {
        let value = value.into();
        validate_topic(&value)?;
        Ok(Self(value))
    }

    /// The topic as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The part before the colon.
    pub fn namespace(&self) -> &str {
        self.0.split_once(':').map_or("", |(ns, _)| ns)
    }
}

impl fmt::Display for Topic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for Topic {
    type Err = TopicError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl TryFrom<String> for Topic {
    type Error = TopicError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<Topic> for String {
    fn from(t: Topic) -> Self {
        t.0
    }
}

impl AsRef<str> for Topic {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// A subscription or permission pattern: an exact topic or a `prefix*` wildcard.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct TopicPattern(String);

impl TopicPattern {
    /// Validates and creates a pattern.
    pub fn new(value: impl Into<String>) -> Result<Self, TopicError> {
        let value = value.into();
        if value == "*" {
            return Ok(Self(value));
        }
        match value.strip_suffix('*') {
            Some(prefix) => {
                if prefix.contains('*') {
                    return Err(err(&value, "`*` is only allowed at the end"));
                }
                let (ns, rest) = prefix
                    .split_once(':')
                    .ok_or_else(|| err(&value, "wildcards must include a namespace, e.g. `file:*`"))?;
                if ns.is_empty() || !ns.chars().chain(rest.chars()).all(valid_segment_char) {
                    return Err(err(&value, "only lowercase letters, digits, `-`, `_` and `.` are allowed"));
                }
            }
            None => validate_topic(&value)?,
        }
        Ok(Self(value))
    }

    /// Pattern matching every topic.
    pub fn any() -> Self {
        Self("*".to_owned())
    }

    /// The pattern as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether the pattern ends with a wildcard.
    pub fn is_wildcard(&self) -> bool {
        self.0.ends_with('*')
    }

    /// Whether this pattern covers the given topic.
    pub fn matches(&self, topic: &Topic) -> bool {
        match self.0.strip_suffix('*') {
            Some(prefix) => topic.as_str().starts_with(prefix),
            None => self.0 == topic.as_str(),
        }
    }

    /// Whether every topic matched by `self` is also matched by `other`.
    ///
    /// Used to check that a requested subscription stays inside the
    /// permissions granted by a shard manifest.
    pub fn is_covered_by(&self, other: &TopicPattern) -> bool {
        match other.0.strip_suffix('*') {
            Some(grant) => self.0.trim_end_matches('*').starts_with(grant),
            None => self.0 == other.0,
        }
    }

    /// Whether this pattern can match anything in the given namespace.
    pub fn touches_namespace(&self, namespace: &str) -> bool {
        match self.0.strip_suffix('*') {
            Some(prefix) => {
                let probe = format!("{namespace}:");
                probe.starts_with(prefix) || prefix.starts_with(&probe)
            }
            None => self.0.split_once(':').is_some_and(|(ns, _)| ns == namespace),
        }
    }
}

impl fmt::Display for TopicPattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for TopicPattern {
    type Err = TopicError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl TryFrom<String> for TopicPattern {
    type Error = TopicError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<TopicPattern> for String {
    fn from(p: TopicPattern) -> Self {
        p.0
    }
}

impl From<Topic> for TopicPattern {
    fn from(t: Topic) -> Self {
        Self(t.0)
    }
}

/// Returns `true` when any pattern in `patterns` matches `topic`.
pub fn any_matches<'a>(patterns: impl IntoIterator<Item = &'a TopicPattern>, topic: &Topic) -> bool {
    patterns.into_iter().any(|p| p.matches(topic))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> Topic {
        Topic::new(s).unwrap()
    }
    fn p(s: &str) -> TopicPattern {
        TopicPattern::new(s).unwrap()
    }

    #[test]
    fn validates_topics() {
        assert!(Topic::new("file:changed").is_ok());
        assert!(Topic::new("agent:ask.chunk").is_ok());
        for bad in ["", "file", ":x", "x:", "File:changed", "file:ch anged", "a:*"] {
            assert!(Topic::new(bad).is_err(), "{bad} should be rejected");
        }
    }

    #[test]
    fn validates_patterns() {
        for good in ["*", "file:*", "agent:ask.*", "file:changed"] {
            assert!(TopicPattern::new(good).is_ok(), "{good} should be accepted");
        }
        for bad in ["fi*le:x", "file*", "*:x", "file:a*b"] {
            assert!(TopicPattern::new(bad).is_err(), "{bad} should be rejected");
        }
    }

    #[test]
    fn matches_topics() {
        assert!(p("*").matches(&t("file:changed")));
        assert!(p("file:*").matches(&t("file:changed")));
        assert!(!p("file:*").matches(&t("files:changed")));
        assert!(p("agent:ask.*").matches(&t("agent:ask.chunk")));
        assert!(!p("agent:ask.*").matches(&t("agent:ask")));
        assert!(p("file:changed").matches(&t("file:changed")));
        assert!(!p("file:changed").matches(&t("file:changed.v2")));
    }

    #[test]
    fn coverage_rules() {
        assert!(p("file:changed").is_covered_by(&p("file:*")));
        assert!(p("file:*").is_covered_by(&p("*")));
        assert!(p("agent:ask.*").is_covered_by(&p("agent:*")));
        assert!(!p("file:*").is_covered_by(&p("file:changed")));
        assert!(!p("*").is_covered_by(&p("file:*")));
    }

    #[test]
    fn namespace_detection() {
        assert!(p("*").touches_namespace("system"));
        assert!(p("system:*").touches_namespace("system"));
        assert!(!p("file:*").touches_namespace("system"));
        assert!(p("system:shutdown").touches_namespace("system"));
    }

    #[test]
    fn serde_roundtrip_validates() {
        let ok: Topic = serde_json::from_str("\"file:changed\"").unwrap();
        assert_eq!(ok.as_str(), "file:changed");
        assert!(serde_json::from_str::<Topic>("\"nope\"").is_err());
    }
}
