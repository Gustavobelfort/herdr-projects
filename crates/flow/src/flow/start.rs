//! Starting an agent program in a pane, one way for every caller (a task's agent, a reviewer, the Coordinator):
//! herdr's own `agent start` where the program takes everything on its command line, and a command typed at the
//! pane's shell where it needs an environment of its own (OpenCode's launch file, agents/opencode.rs) or a read-only
//! mount (a plan's writer, plan/sandbox.rs). herdr then finds the agent in the pane by itself, and this waits for that.
//! Answers and fails as `agent start` does: a program whose start-up dialog is in the way is "agent_not_ready".
use crate::herdr::{cli::HerdrError, Herdr};
use crate::json::*;

pub fn q(s: &str) -> String {
    // A value the shell reads itself (`K=$(cat '<file>')`: OpenCode's launch config, too long for a typed line) stays
    // a shell word, in double quotes; any other is quoted whole.
    if let Some((k, v)) = s.split_once("=$(") { if k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && v.ends_with(')') && !v.contains('"') { return format!("{k}=\"$({v}\""); } }
    if !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "_@%+=:,./-#".contains(c)) { s.to_string() } else { crate::util::shq(s) }
}
fn herr(code: &str, msg: String) -> HerdrError { HerdrError { code: code.into(), message: msg } }
/// The command typed at the shell: `env HERDR_PROJECTS_AGENT=<name> K=V … <kind> <args>`. The name it would have had
/// from `agent start` rides along, so a fake herdr can give it the agent's name.
pub fn env_command(name: &str, kind: &str, args: &[String], env: &[(String, String)]) -> String {
    let mut all: Vec<String> = vec!["env".into(), format!("HERDR_PROJECTS_AGENT={name}")];
    all.extend(env.iter().map(|(k, v)| format!("{k}={v}")));
    all.push(crate::agents::kinds::program(kind).into());
    all.extend(args.iter().cloned());
    all.iter().map(|s| q(s)).collect::<Vec<_>>().join(" ")
}
/// Start the program. `command`: a whole command to type instead (a read-only mount); none, with an environment: the
/// env command; none and no environment: `agent start`.
pub fn program(api: &Herdr, name: &str, kind: &str, pane: &str, args: &[String], env: &[(String, String)], command: Option<String>) -> Result<(), HerdrError> {
    if command.is_none() && env.is_empty() { return api.agent_start(name, kind, pane, args, 60000).map(|_| ()); }
    api.integration_ready(kind)?;
    api.pane_run(pane, &command.unwrap_or_else(|| env_command(name, kind, args, env)))?;
    let step = std::env::var("HERDR_PROJECTS_READY_STEP_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(250u64);
    let until = crate::clock::now() + 60000;
    loop {
        if let Some(a) = api.agent_get(pane)? {
            if a.s("agent_status") == "blocked" { return Err(herr("agent_not_ready", format!("agent {name} is blocked during startup and is not ready for prompts"))); }
            return Ok(());
        }
        if crate::clock::now() >= until { return Err(herr("failed", format!("{kind} did not start in its pane within a minute. Look at the pane."))); }
        if step > 0 { std::thread::sleep(std::time::Duration::from_millis(step)); }
    }
}
