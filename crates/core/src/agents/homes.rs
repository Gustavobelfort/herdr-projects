//! Where each agent program keeps its own settings and files.
use crate::store::files::user_home;
use std::path::PathBuf;

fn env(k: &str) -> Option<PathBuf> { std::env::var(k).ok().filter(|v| !v.is_empty()).map(PathBuf::from) }
pub fn claude_file() -> PathBuf { env("HERDR_PROJECTS_CLAUDE_JSON").unwrap_or_else(|| user_home().join(".claude.json")) }
pub fn codex_file() -> PathBuf { env("HERDR_PROJECTS_CODEX_TOML").unwrap_or_else(|| codex_home().join("config.toml")) }
pub fn claude_home() -> PathBuf { env("CLAUDE_CONFIG_DIR").unwrap_or_else(|| user_home().join(".claude")) }
pub fn codex_home() -> PathBuf { env("CODEX_HOME").unwrap_or_else(|| user_home().join(".codex")) }
pub fn pi_home() -> PathBuf { env("PI_CODING_AGENT_DIR").unwrap_or_else(|| user_home().join(".pi").join("agent")) }
/// OpenCode keeps its settings under the XDG config folder, and its conversations in one database under the XDG data folder.
pub fn opencode_home() -> PathBuf { env("XDG_CONFIG_HOME").unwrap_or_else(|| user_home().join(".config")).join("opencode") }
pub fn opencode_db() -> PathBuf { env("OPENCODE_DB").unwrap_or_else(|| env("XDG_DATA_HOME").unwrap_or_else(|| user_home().join(".local").join("share")).join("opencode").join("opencode.db")) }
