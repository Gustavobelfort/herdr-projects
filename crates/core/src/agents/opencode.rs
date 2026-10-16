//! OpenCode as a kind of its own: the launch file each start is given, and a copy of its conversation the board's
//! readers can read. OpenCode (2.0) takes its model and its permission rules from a config file named by
//! `OPENCODE_CONFIG` (its interactive program has no model flag), read on top of its own global config, where its
//! providers and their keys stay: the board declares no provider and handles no key. So a start writes one small file
//! (the model with the effort as its variant, the access rules of the role, no self-update) and names it in the
//! program's environment. OpenCode keeps no file per conversation: every message is a row in one SQLite database, so a
//! read copies the conversation's rows into one JSON-lines file (`data/opencode/<session>.jsonl`), which the readers in
//! read_turn.rs, read_tokens.rs and talk.rs take as they take any other program's file. Checked on 2.0.25
//! (docs/opencode.md).
use super::homes::opencode_db;
use crate::err::Res;
use crate::json::*;
use crate::store::files::{data_dir, read_obj, write_json, write_private};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

// ---- the launch file -----------------------------------------------------------------------------------------------
/// The permission rules of a role, laid over OpenCode's own (its `read *.env: ask` among them): `full` is OpenCode's
/// --auto, and the file says nothing, so its own rules stand; `ask` asks before an edit, a command, a fetch and anything
/// outside the folder, as the Coordinator does on Claude Code. Never a catch-all `allow`: that would come after
/// OpenCode's own rules and override them, and an agent in `ask` mode would read a secrets file without asking.
fn rules(access: &str) -> Value {
    let rule = |action: &str| json!({ "action": action, "resource": "*", "effect": "ask" });
    if access == "ask" { json!([rule("edit"), rule("shell"), rule("webfetch"), rule("external_directory")]) } else { json!([]) }
}
/// The file one start is given: the model with its effort as its variant (what a new conversation runs on, and what a
/// reopened one is switched to; none named: OpenCode's own default), the role's permission rules, and no self-update
/// under a running agent. A model of an OpenAI-compatible gateway that OpenCode's config does not list by name is
/// declared here under its provider (OpenCode runs only the models its config names; the user's config is not touched).
/// Mode 0600, in the folder named.
pub fn launch_file(dir: &Path, role: &str, model: &str, effort: &str, access: &str) -> Res<PathBuf> {
    let mut cfg = obj(json!({ "$schema": "https://opencode.ai/config.json", "update": "disable" }));
    let r = rules(access);
    if !r.as_array().map(|a| a.is_empty()).unwrap_or(true) { cfg.put("permissions", r); }
    if let Some(m) = model_arg(model, effort) { cfg.put("model", m); }
    if let Some((provider, id)) = gateway_model(model) { cfg.put("providers", json!({ provider: { "models": { id.clone(): { "name": id } } } })); }
    let file = dir.join(format!("opencode-{role}.json"));
    write_private(&file, &serde_json::to_string_pretty(&Value::Object(cfg))?)?;
    Ok(file)
}
/// A model of one of OpenCode's gateways, as its inventory names it: its provider and its id, declared at start (an
/// entry OpenCode's config has too merges into the same one; one it lacks is what makes the model runnable).
fn gateway_model(model: &str) -> Option<(String, String)> {
    let m = model.split('#').next().unwrap_or("");
    let (provider, id) = m.split_once('/')?;
    let served = super::opencode_models::gateways().iter().any(|(n, _, _)| n == provider) && super::opencode_models::names().iter().any(|x| x == m);
    served.then(|| (provider.to_string(), id.to_string()))
}
/// The environment a start needs: the launch file's text inline (`OPENCODE_CONFIG_CONTENT` is read after the repo's own
/// `opencode.json`, where `OPENCODE_CONFIG` would be read before it and lose), and no self-update under a running agent.
pub fn launch_env(file: &Path) -> Vec<(String, String)> {
    let text = std::fs::read_to_string(file).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).map(|v| v.to_string()).unwrap_or_default();
    vec![("OPENCODE_CONFIG_CONTENT".into(), text), ("OPENCODE_DISABLE_AUTOUPDATE".into(), "1".into())]
}
/// `<provider>/<model>#<effort>`, the launch file's `model`: the effort is the model's variant, none for `default`.
/// None for no model: OpenCode's own default.
pub fn model_arg(model: &str, effort: &str) -> Option<String> {
    if model.is_empty() { return None; }
    Some(if effort.is_empty() || model.contains('#') { model.to_string() } else { format!("{model}#{effort}") })
}

// ---- a conversation, copied out of the database -------------------------------------------------------------------
fn mirror_dir() -> PathBuf { data_dir().join("opencode") }
pub fn mirror_file(session: &str) -> PathBuf { mirror_dir().join(format!("{session}.jsonl")) }
fn sql(query: &str) -> Option<String> {
    let db = opencode_db();
    if !db.exists() { return None; }
    let out = Command::new("sqlite3").args(["-readonly", "-json", &db.display().to_string(), query]).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}
fn rows(query: &str) -> Vec<Value> { sql(query).and_then(|t| serde_json::from_str::<Value>(t.trim()).ok()).and_then(|v| v.as_array().cloned()).unwrap_or_default() }
fn q(s: &str) -> String { s.replace('\'', "''") }
/// The conversation's rows as one line each ({ type, seq, at, data }), written again only when the database has more
/// or newer rows than the copy (`HERDR_PROJECTS_OPENCODE_MIRROR=0`: never asked, tests write the copy themselves).
pub fn freshen(session: &str) -> Option<PathBuf> {
    let file = mirror_file(session);
    if std::env::var("HERDR_PROJECTS_OPENCODE_MIRROR").as_deref() == Ok("0") || !session.starts_with("ses_") || session.contains(|c: char| !c.is_ascii_alphanumeric() && c != '_') { return file.exists().then_some(file); }
    let kept = read_obj(&mirror_dir().join(format!("{session}.at.json")));
    let head = rows(&format!("select count(*) as n, max(time_updated) as t from session_message where session_id='{}'", q(session)));
    let (n, t) = head.first().map(|r| (r.i("n"), r.i("t"))).unwrap_or((0, 0));
    if n == 0 { return file.exists().then_some(file); }
    if kept.as_ref().map(|k| k.i("n") == n && k.i("t") == t).unwrap_or(false) && file.exists() { return Some(file); }
    let list = rows(&format!("select type, seq, time_created, data from session_message where session_id='{}' order by seq", q(session)));
    if list.is_empty() { return file.exists().then_some(file); }
    let text: String = list.iter().map(|r| { let data = serde_json::from_str::<Value>(r.s("data")).unwrap_or(Value::Null); format!("{}\n", json!({ "type": r.s("type"), "seq": r.i("seq"), "at": r.i("time_created"), "data": data })) }).collect();
    crate::store::files::write_atomic(&file, text.as_bytes()).ok()?;
    let _ = write_json(&mirror_dir().join(format!("{session}.at.json")), &json!({ "n": n, "t": t }));
    Some(file)
}
/// The newest top-level conversation OpenCode saved for a folder since a moment (its id and when it was made).
pub fn latest_in(dir: &Path, was: Option<&Path>, since: i64) -> Option<String> {
    let dirs: Vec<String> = [Some(dir), was].into_iter().flatten().map(|d| q(&d.display().to_string())).collect();
    let r = rows(&format!("select id, time_created from session_v2 where parent_id is null and directory in ('{}') order by time_created desc limit 1", dirs.join("','")));
    r.first().filter(|x| x.i("time_created") >= since).map(|x| x.t("id"))
}
