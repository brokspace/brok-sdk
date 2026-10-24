//! The `shard.json` manifest.
//!
//! A manifest declares what a shard *is* (name, version, entrypoint) and what
//! it is *allowed to do* (capabilities). The runtime enforces the event
//! capabilities on the bus and uses the rest to build the process sandbox.
//!
//! ```json
//! {
//!   "name": "brok-security-auditor",
//!   "version": "1.0.0",
//!   "entrypoint": "bin/security-auditor",
//!   "runtime": "native",
//!   "permissions": {
//!     "filesystem": { "read": ["src/"], "write": [] },
//!     "network": false
//!   },
//!   "subscribes_to": ["file:changed"],
//!   "publishes_to": ["security:alert"]
//! }
//! ```

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::env::RESERVED_PREFIX;
use crate::error::ManifestError;
use crate::topic::{SYSTEM_NAMESPACE, TopicPattern};

/// File name of the manifest at the root of a shard repository.
pub const MANIFEST_FILE: &str = "shard.json";

/// A parsed `shard.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShardManifest {
    /// Optional JSON schema reference for editor support.
    #[serde(rename = "$schema", default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    /// Unique shard name: `^[a-z][a-z0-9-]{1,63}$`.
    pub name: String,
    /// Semantic version.
    pub version: semver::Version,
    /// One line description.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Authors.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub authors: Vec<String>,
    /// SPDX license expression.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    /// Source repository URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    /// Compatible `brok` versions, e.g. `">=0.1, <0.3"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brok: Option<semver::VersionReq>,
    /// Executable path relative to the shard directory. On Windows `.exe` is
    /// appended automatically when missing. Omitted by skill-only shards.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entrypoint: Option<String>,
    /// Directories (relative to the shard) holding agent skills: each
    /// direct subfolder with a `SKILL.md` is one skill. A shard may ship only
    /// skills, only a process, or both.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<String>,
    /// Extra command line arguments.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    /// Execution runtime.
    #[serde(default)]
    pub runtime: Runtime,
    /// Requested capabilities.
    #[serde(default)]
    pub permissions: Permissions,
    /// Topics the shard may subscribe to.
    #[serde(default)]
    pub subscribes_to: Vec<TopicPattern>,
    /// Topics the shard may publish to.
    #[serde(default)]
    pub publishes_to: Vec<TopicPattern>,
    /// How to build the shard from source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<BuildSpec>,
    /// Prebuilt binaries per platform (`linux-x86_64`, `macos-aarch64`,
    /// `windows-x86_64`, ...). See [`current_platform`].
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub releases: BTreeMap<String, ReleaseAsset>,
}

/// How a shard is executed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Runtime {
    /// A native executable.
    #[default]
    Native,
}

/// Capabilities requested by a shard. Everything defaults to *denied*.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Permissions {
    /// Workspace relative paths the shard may read and write.
    #[serde(default)]
    pub filesystem: FsPermissions,
    /// Whether the shard may open network connections.
    #[serde(default)]
    pub network: bool,
    /// Executables the shard may spawn (e.g. `git`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub process: Vec<String>,
    /// Environment variables forwarded to the shard. A trailing `*` acts as
    /// a prefix wildcard (`OPENAI_*`); a leading `*` as a suffix wildcard
    /// (`*_API_KEY`). Everything else is scrubbed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env: Vec<String>,
}

/// File system capabilities, as workspace relative paths.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FsPermissions {
    /// Readable paths.
    #[serde(default)]
    pub read: Vec<String>,
    /// Writable paths.
    #[serde(default)]
    pub write: Vec<String>,
}

/// Build-from-source instructions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildSpec {
    /// Program and arguments, executed in the shard repository without a
    /// shell, e.g. `["cargo", "build", "--release", "--locked"]`.
    pub command: Vec<String>,
    /// Path of the produced executable, relative to the repository.
    pub artifact: String,
}

/// A prebuilt, checksum-pinned binary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseAsset {
    /// HTTPS download URL.
    pub url: String,
    /// Lowercase hex SHA-256 of the file.
    pub sha256: String,
}

/// The platform key used in [`ShardManifest::releases`], e.g. `linux-x86_64`.
pub fn current_platform() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

impl ShardManifest {
    /// Parses and validates a manifest from JSON text.
    pub fn from_json(json: &str) -> Result<Self, ManifestError> {
        let manifest: Self = serde_json::from_str(json)?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Reads, parses and validates `dir/shard.json`.
    pub fn load_from_dir(dir: &Path) -> Result<Self, ManifestError> {
        let path = dir.join(MANIFEST_FILE);
        let json = std::fs::read_to_string(&path).map_err(|source| ManifestError::Io { path, source })?;
        Self::from_json(&json)
    }

    /// Serializes the manifest as pretty JSON.
    pub fn to_json_pretty(&self) -> String {
        serde_json::to_string_pretty(self).expect("manifest serialization is infallible")
    }

    /// Absolute path of the executable for a shard installed in `shard_dir`,
    /// or `None` for skill-only shards.
    pub fn entrypoint_path(&self, shard_dir: &Path) -> Option<PathBuf> {
        let path = shard_dir.join(self.entrypoint.as_deref()?);
        Some(if cfg!(windows) && path.extension().is_none() { path.with_extension("exe") } else { path })
    }

    /// Whether the shard runs a process (as opposed to only shipping skills).
    pub fn has_process(&self) -> bool {
        self.entrypoint.is_some()
    }

    /// Checks every rule and reports all violations at once.
    pub fn validate(&self) -> Result<(), ManifestError> {
        let mut errors = Vec::new();

        if !is_valid_name(&self.name) {
            errors.push(format!(
                "name `{}` must match ^[a-z][a-z0-9-]{{1,63}}$ (lowercase, digits and dashes)",
                self.name
            ));
        }
        match &self.entrypoint {
            Some(entrypoint) => check_relative("entrypoint", entrypoint, &mut errors),
            None if self.skills.is_empty() => {
                errors.push("a shard needs an `entrypoint`, `skills`, or both".to_owned());
            }
            None => {
                if self.build.is_some() || !self.releases.is_empty() {
                    errors.push("`build` and `releases` require an `entrypoint`".to_owned());
                }
                if !self.subscribes_to.is_empty() || !self.publishes_to.is_empty() {
                    errors.push(
                        "topic permissions require an `entrypoint` (skill-only shards run no process)".to_owned(),
                    );
                }
            }
        }
        for dir in &self.skills {
            check_relative("skills", dir, &mut errors);
        }

        for path in &self.permissions.filesystem.read {
            check_relative("permissions.filesystem.read", path, &mut errors);
        }
        for path in &self.permissions.filesystem.write {
            check_relative("permissions.filesystem.write", path, &mut errors);
        }
        for program in &self.permissions.process {
            if program.trim().is_empty() || program.contains(['/', '\\']) {
                errors.push(format!("permissions.process: `{program}` must be a bare program name"));
            }
        }
        for var in &self.permissions.env {
            let bare = var.trim_matches('*');
            if bare.is_empty() || !bare.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                errors.push(format!("permissions.env: `{var}` is not a valid variable name or pattern"));
            } else if var.to_ascii_uppercase().starts_with(RESERVED_PREFIX) {
                errors.push(format!("permissions.env: `{var}` uses the reserved {RESERVED_PREFIX} prefix"));
            }
        }

        for pattern in &self.publishes_to {
            if pattern.touches_namespace(SYSTEM_NAMESPACE) {
                errors.push(format!("publishes_to: `{pattern}` overlaps the reserved `{SYSTEM_NAMESPACE}:` namespace"));
            }
        }

        if let Some(build) = &self.build {
            if build.command.is_empty() {
                errors.push("build.command must not be empty".to_owned());
            }
            check_relative("build.artifact", &build.artifact, &mut errors);
        }
        for (platform, asset) in &self.releases {
            if !platform.contains('-') {
                errors.push(format!("releases: key `{platform}` must look like `<os>-<arch>`"));
            }
            if !asset.url.starts_with("https://") {
                errors.push(format!("releases.{platform}.url must use https://"));
            }
            if asset.sha256.len() != 64 || !asset.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
                errors.push(format!("releases.{platform}.sha256 must be 64 hex characters"));
            }
        }

        if errors.is_empty() { Ok(()) } else { Err(ManifestError::Invalid(errors)) }
    }

    /// Human readable list of the requested capabilities, used to ask the
    /// user for consent at install time.
    pub fn capability_report(&self) -> Vec<String> {
        let p = &self.permissions;
        let list = |items: &[String]| if items.is_empty() { "none".to_owned() } else { items.join(", ") };
        let topics = |items: &[TopicPattern]| {
            if items.is_empty() {
                "none".to_owned()
            } else {
                items.iter().map(TopicPattern::as_str).collect::<Vec<_>>().join(", ")
            }
        };
        let mut report = vec![
            format!("read files:      {}", list(&p.filesystem.read)),
            format!("write files:     {}", list(&p.filesystem.write)),
            format!("network access:  {}", if p.network { "yes" } else { "no" }),
            format!("run programs:    {}", list(&p.process)),
            format!("env variables:   {}", list(&p.env)),
            format!("subscribes to:   {}", topics(&self.subscribes_to)),
            format!("publishes to:    {}", topics(&self.publishes_to)),
            format!("agent skills:    {}", list(&self.skills)),
            format!("runs a process:  {}", if self.has_process() { "yes" } else { "no (skills only)" }),
        ];
        if self.subscribes_to.iter().any(|t| t.as_str() == "*") {
            report.push("warning: subscribes to every topic on the bus".to_owned());
        }
        if p.filesystem.write.iter().any(|w| w.trim_matches(['.', '/']).is_empty()) {
            report.push("warning: requests write access to the whole workspace".to_owned());
        }
        report
    }
}

/// Whether `name` is a valid shard name.
pub fn is_valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    (2..=64).contains(&name.len())
        && chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn check_relative(field: &str, value: &str, errors: &mut Vec<String>) {
    let path = Path::new(value);
    if value.is_empty() {
        errors.push(format!("{field} must not be empty"));
    } else if path.is_absolute() || value.starts_with(['/', '\\']) || path.has_root() {
        errors.push(format!("{field}: `{value}` must be a relative path"));
    } else if path.components().any(|c| matches!(c, Component::ParentDir | Component::Prefix(_))) {
        errors.push(format!("{field}: `{value}` must not escape its directory"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
      "name": "brok-security-auditor",
      "version": "1.0.0",
      "entrypoint": "bin/security-auditor",
      "runtime": "native",
      "permissions": {
        "filesystem": { "read": ["src/"], "write": [] },
        "network": false
      },
      "subscribes_to": ["file:changed"],
      "publishes_to": ["security:alert"]
    }"#;

    #[test]
    fn parses_the_reference_manifest() {
        let m = ShardManifest::from_json(SAMPLE).unwrap();
        assert_eq!(m.name, "brok-security-auditor");
        assert_eq!(m.version, semver::Version::new(1, 0, 0));
        assert_eq!(m.permissions.filesystem.read, vec!["src/"]);
        assert!(!m.permissions.network);
        assert_eq!(m.publishes_to[0].as_str(), "security:alert");
    }

    #[test]
    fn reports_every_violation() {
        let json = r#"{
          "name": "Bad Name",
          "version": "1.0.0",
          "entrypoint": "../escape",
          "permissions": { "filesystem": { "read": ["/etc"] }, "env": ["BROK_TOKEN"] },
          "publishes_to": ["system:*"],
          "releases": { "linux-x86_64": { "url": "http://x", "sha256": "abc" } }
        }"#;
        let Err(ManifestError::Invalid(errors)) = ShardManifest::from_json(json) else {
            panic!("manifest should be invalid");
        };
        assert_eq!(errors.len(), 7, "{errors:#?}");
    }

    #[test]
    fn rejects_unknown_fields() {
        let json = SAMPLE.replace("\"runtime\"", "\"runtim\"");
        assert!(matches!(ShardManifest::from_json(&json), Err(ManifestError::Parse(_))));
    }

    #[test]
    fn rejects_publishing_everything() {
        let json = SAMPLE.replace("\"security:alert\"", "\"*\"");
        assert!(ShardManifest::from_json(&json).is_err());
    }

    #[test]
    fn capability_report_lists_permissions() {
        let m = ShardManifest::from_json(SAMPLE).unwrap();
        let report = m.capability_report().join("\n");
        assert!(report.contains("read files:      src/"));
        assert!(report.contains("network access:  no"));
    }

    #[test]
    fn skill_only_shards() {
        let json = r#"{ "name": "acme-skills", "version": "0.1.0", "skills": ["skills"] }"#;
        let m = ShardManifest::from_json(json).unwrap();
        assert!(!m.has_process());
        assert_eq!(m.entrypoint_path(Path::new("/x")), None);
        assert!(m.capability_report().iter().any(|l| l.contains("skills only")));

        let neither = r#"{ "name": "acme-skills", "version": "0.1.0" }"#;
        assert!(ShardManifest::from_json(neither).is_err());
        let topics_without_process =
            r#"{ "name": "acme-skills", "version": "0.1.0", "skills": ["s"], "subscribes_to": ["file:changed"] }"#;
        assert!(ShardManifest::from_json(topics_without_process).is_err());
    }

    #[test]
    fn names() {
        assert!(is_valid_name("brok-agent"));
        assert!(!is_valid_name("a"));
        assert!(!is_valid_name("-agent"));
        assert!(!is_valid_name("Agent"));
    }
}
