//! The models an OpenAI-compatible provider of OpenCode's serves, read from the provider itself. OpenCode lists only
//! the models written under a provider in its config, so a gateway's new model is unknown to it until someone types it
//! in. The board asks each such provider for its inventory (`GET <baseURL>/models`, with the provider's own key) and
//! takes those names too, as `<provider>/<id>`; the launch file then declares the one chosen so OpenCode will run it.
//! The inventory is kept a day in `data/opencode/inventory.json` (`HERDR_PROJECTS_OPENCODE_INVENTORY` names a file to
//! read instead: tests). Nothing here is written to OpenCode's config: that stays the user's.
use super::homes::opencode_home;
use crate::clock::now;
use crate::json::*;
use crate::store::files::{data_dir, read_json, read_obj, read_text, user_home, write_private};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const PACKAGE: &str = "openai-compatible";
const KEPT: i64 = 86_400_000;
fn file() -> PathBuf { data_dir().join("opencode").join("inventory.json") }

/// OpenCode's own config, as it reads it: `opencode.json` or `opencode.jsonc` in its folder (comments allowed).
fn config() -> Value {
    for name in ["opencode.json", "opencode.jsonc"] {
        if let Some(t) = read_text(&opencode_home().join(name)) {
            if let Ok(v) = json5_lite(&t) { return v; }
        }
    }
    Value::Null
}
/// JSON with `//` and `/* */` comments and trailing commas taken out (what a `.jsonc` holds).
fn json5_lite(t: &str) -> Result<Value, serde_json::Error> {
    let (mut out, mut chars, mut in_str, mut esc) = (String::with_capacity(t.len()), t.chars().peekable(), false, false);
    while let Some(c) = chars.next() {
        if in_str { out.push(c); if esc { esc = false; } else if c == '\\' { esc = true; } else if c == '"' { in_str = false; } continue; }
        match c {
            '"' => { in_str = true; out.push(c); }
            '/' if chars.peek() == Some(&'/') => { for c in chars.by_ref() { if c == '\n' { out.push('\n'); break; } } }
            '/' if chars.peek() == Some(&'*') => { chars.next(); let mut last = ' '; for c in chars.by_ref() { if last == '*' && c == '/' { break; } last = c; } }
            ',' => { let rest: String = chars.clone().take_while(|x| x.is_whitespace() || *x == ']' || *x == '}').collect(); if rest.trim_start().starts_with([']', '}']) { continue; } out.push(c); }
            _ => out.push(c),
        }
    }
    serde_json::from_str(&out)
}
/// A setting's value with OpenCode's substitutions made: `{env:NAME}`, `{file:path}` (`~` the home folder).
fn substituted(v: &str) -> String {
    let mut s = v.to_string();
    while let Some(i) = s.find('{') {
        let Some(j) = s[i..].find('}') else { break };
        let inner = &s[i + 1..i + j];
        let got = if let Some(n) = inner.strip_prefix("env:") { std::env::var(n).unwrap_or_default() }
            else if let Some(p) = inner.strip_prefix("file:") { read_text(&expand(p)).map(|t| t.trim().to_string()).unwrap_or_default() }
            else { break };
        s.replace_range(i..i + j + 1, &got);
    }
    s
}
fn expand(p: &str) -> PathBuf { if let Some(rest) = p.strip_prefix("~/") { user_home().join(rest) } else { PathBuf::from(p) } }
/// The OpenAI-compatible providers OpenCode's config declares: name, base URL, key (substituted; "" when none).
pub fn gateways() -> Vec<(String, String, String)> {
    let c = config();
    let mut out = vec![];
    for key in ["providers", "provider"] {
        let Some(ps) = c.o(key) else { continue };
        for (name, p) in ps {
            let pkg = p.s("package").to_string() + p.s("npm");
            if !pkg.contains(PACKAGE) { continue; }
            let settings = if p.at("settings").is_object() { p.at("settings") } else { p.at("options") };
            let url = substituted(settings.s("baseURL")).trim_end_matches('/').to_string();
            if url.is_empty() { continue; }
            out.push((name.clone(), url, substituted(settings.s("apiKey"))));
        }
    }
    out
}
fn fetch(url: &str, key: &str) -> Option<Vec<String>> {
    // The key goes to curl on its standard input (`-H @-`), never on its command line, where any process could read it.
    let mut c = Command::new("curl");
    c.args(["-fsS", "--max-time", "20"]);
    if !key.is_empty() { c.args(["-H", "@-"]); }
    let mut child = c.arg(format!("{url}/models")).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    if let Some(mut stdin) = child.stdin.take() { use std::io::Write; if !key.is_empty() { let _ = stdin.write_all(format!("Authorization: Bearer {key}\nx-api-key: {key}\n").as_bytes()); } }
    let out = child.wait_with_output().ok()?;
    if !out.status.success() { return None; }
    let v: Value = serde_json::from_slice(&out.stdout).ok()?;
    let mut m: Vec<&Value> = v.arr("data").iter().collect();
    m.sort_by_key(|x| std::cmp::Reverse(x.i("created")));
    Some(m.iter().map(|x| x.t("id")).filter(|id| !id.is_empty()).collect())
}
/// Every `<provider>/<id>` the gateways serve, newest first, fetched at most once a day; a gateway that cannot be
/// asked keeps what it said last. Empty when OpenCode's config names no gateway.
pub fn names() -> Vec<String> {
    if let Ok(f) = std::env::var("HERDR_PROJECTS_OPENCODE_INVENTORY") { return read_json(Path::new(&f)).map(|v| strings(v.at("names"))).unwrap_or_default(); }
    let gw = gateways();
    if gw.is_empty() { return vec![]; }
    let (mut kept, mut changed, mut out) = (read_obj(&file()).unwrap_or_default(), false, vec![]);
    for (name, url, key) in gw {
        let had = kept.at(&name).clone();
        let fresh = had.at("ids").is_array() && now() - had.i("at") < KEPT;
        let ids = if fresh { strings(had.at("ids")) } else {
            match fetch(&url, &key) { Some(ids) if !ids.is_empty() => { kept.put(&name, json!({ "at": now(), "ids": ids })); changed = true; ids } _ => strings(had.at("ids")) }
        };
        out.extend(ids.into_iter().map(|id| format!("{name}/{id}")));
    }
    if changed { let _ = write_private(&file(), &Value::Object(kept).to_string()); } // ids only, but no wider than it need be
    out
}
/// Models the gateways list that take a chat: an image, embedding, speech or review-only model answers no conversation.
pub fn chat_model(name: &str) -> bool { let id = name.rsplit('/').next().unwrap_or(name); !(id.contains("image") || id.contains("embed") || id.contains("tts") || id.contains("whisper") || id.contains("auto-review")) }
