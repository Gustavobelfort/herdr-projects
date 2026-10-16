//! OpenCode as a kind of its own (docs/opencode.md): how it is started (a small launch file with the model, the
//! effort as its variant and the role's permission rules laid over OpenCode's own, named in the program's environment
//! on top of OpenCode's own config; a conversation id of the board's own), how a model is checked against what
//! `opencode models` lists and what its OpenAI-compatible gateways serve (their inventory, read by the board), how
//! its conversation is read (a copy of its database rows: a finished turn, a login error, a provider error that
//! passes, the tokens, the words of the chat), and the Coordinator on it, end to end, with a model switch.
use crate::common;
use common::*;
use serde_json::{json, Value};
use std::path::Path;
use sw::agents::adapter::{launch_env, new_session, resume_args, session_args, transcript};
use sw::agents::opencode::{launch_file, mirror_file, model_arg};
use sw::agents::read_turn::read_turn;
use sw::agents::start::{start_args, StartOpts};
use sw::agents::talk::talk_of;
use sw::coordinator::{look::look, requests::{fresh, send}, state, who::pick_of};
use sw::json::*;
use sw::store::settings::change::set_value;
use sw::store::settings::lookup::Scope;

fn fixture(name: &str) -> std::path::PathBuf { Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/endings").join(format!("{name}.jsonl")) }
fn opts(model: &str, effort: &str, access: &str, resume: &str, session: &str) -> StartOpts { StartOpts { model: model.into(), effort: effort.into(), access: access.into(), resume: resume.into(), session: session.into(), ..Default::default() } }
/// The names `opencode models` printed, as the board keeps them (the tests never run a real program for them).
fn lists(names: &[&str]) { std::fs::write(sw::store::files::data_dir().join("models.json"), json!({ "opencode": { "at": sw::clock::now(), "taken": names, "shown": names } }).to_string()).unwrap(); }
const NAMES: &[&str] = &["anthropic/claude-opus-5-5", "anthropic/claude-haiku-5-5", "openai/gpt-6-sol", "ollama/gemma-4-e4b"];

#[test]
fn opencode_is_started_with_a_launch_file_that_carries_its_model_effort_and_rules() {
    let w = world();
    // The arguments: access and a conversation id OpenCode takes. The model is not a flag: its interactive program
    // has none, so the model and its effort (as the variant) are the launch file's.
    assert_eq!(start_args("opencode", &opts("openai/gpt-6-sol", "high", "full", "", "ses_abc")).unwrap(), vec!["--standalone", "--session", "ses_abc", "--auto"]);
    assert!(start_args("opencode", &opts("", "default", "ask", "", "")).unwrap().is_empty()); // ask: no --auto, the file's rules ask
    assert_eq!(start_args("opencode", &opts("ollama/gemma-4-e4b", "", "", "ses_old", "")).unwrap(), vec!["--standalone", "--session", "ses_old", "--auto"]); // a reopened conversation
    assert_eq!(model_arg("openai/gpt-6-sol", "high").as_deref(), Some("openai/gpt-6-sol#high")); assert_eq!(model_arg("", ""), None); // no model: OpenCode's own default
    assert_eq!(model_arg("anthropic/claude-opus-5-5#max", "high").as_deref(), Some("anthropic/claude-opus-5-5#max")); // a variant in the name stays
    let id = new_session("opencode").unwrap();
    assert!(id.starts_with("ses_") && id.len() == 30, "{id}"); assert_eq!(session_args("opencode", &id), resume_args("opencode", &id));
    // The launch file: the model, the rules of the role's access, no self-update; nothing of any provider or key
    // (those are OpenCode's own config's); only its owner may read it.
    let dir = w.base.join("launch");
    let f = launch_file(&dir, "coordinator", "anthropic/claude-opus-5-5", "high", "ask").unwrap();
    let j: Value = serde_json::from_str(&std::fs::read_to_string(&f).unwrap()).unwrap();
    assert_eq!(f, dir.join("opencode-coordinator.json"));
    use std::os::unix::fs::PermissionsExt; assert_eq!(std::fs::metadata(&f).unwrap().permissions().mode() & 0o777, 0o600);
    assert_eq!(j.s("model"), "anthropic/claude-opus-5-5#high"); assert_eq!(j.s("update"), "disable");
    // Never a catch-all allow: it would come after OpenCode's own rules (`read *.env: ask`) and override them.
    let asks = |v: &Value| v.arr("permissions").iter().map(|r| format!("{}:{}", r.s("action"), r.s("effect"))).collect::<Vec<_>>();
    assert_eq!(asks(&j), vec!["edit:ask", "shell:ask", "webfetch:ask", "external_directory:ask"]);
    // The same rules for each of OpenCode's agents: an agent's own rules come after the top-level ones, so a repo's
    // config giving one of them `shell: allow` would otherwise cancel the ask.
    let mut agents: Vec<&String> = j.at("agents").as_object().unwrap().keys().collect(); agents.sort();
    assert_eq!(agents, ["build", "compaction", "explore", "general", "plan", "scout", "summary", "title"]);
    assert!(j.at("agents").as_object().unwrap().values().all(|a| asks(a) == asks(&j)));
    let mut keys: Vec<&String> = j.as_object().unwrap().keys().collect(); keys.sort();
    assert_eq!(keys, ["$schema", "agents", "model", "permissions", "update"]);
    // Full access is --auto on the command line: the file says nothing of permissions, so OpenCode's own rules stand.
    let full: Value = serde_json::from_str(&std::fs::read_to_string(launch_file(&dir, "agent", "", "", "full").unwrap()).unwrap()).unwrap();
    assert!(!full.has("permissions") && !full.has("agents")); assert!(!full.b("model")); // no model named: the file names none
    // What a start is given: the file's text inline (read after the repo's own opencode.json, which would otherwise
    // replace the model and cancel the ask rules;
    // too long for a typed line, the pane's shell reads the file into the variable), and no self-update under a
    // running agent. Nothing for the other kinds.
    let env = launch_env("opencode", &dir, "agent", "openai/gpt-6-sol", "high", "ask").unwrap().unwrap();
    assert_eq!(env, vec![("OPENCODE_CONFIG_CONTENT".to_string(), format!("$(cat '{}')", dir.join("opencode-agent.json").display())), ("OPENCODE_DISABLE_AUTOUPDATE".into(), "1".into())]);
    assert_eq!(sw::flow::start::env_command("a1", "opencode", &["--auto".into()], &env), format!("env HERDR_PROJECTS_AGENT=a1 OPENCODE_CONFIG_CONTENT=\"$(cat '{}')\" OPENCODE_DISABLE_AUTOUPDATE=1 opencode --auto", dir.join("opencode-agent.json").display()));
    let inline: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("opencode-agent.json")).unwrap()).unwrap();
    assert_eq!(inline.s("model"), "openai/gpt-6-sol#high"); assert_eq!(inline.arr("permissions").len(), 4);
    assert!(launch_env("claude", &dir, "agent", "", "", "full").unwrap().is_none());
}

#[test]
fn a_model_after_opencode_must_be_one_opencode_lists() {
    let _w = world();
    let you = || Scope::default();
    lists(NAMES);
    assert_eq!(set_value("worker.agent", &json!("opencode/openai/gpt-6-sol"), &you()).unwrap().value, json!("opencode/openai/gpt-6-sol"));
    assert_eq!(set_value("coordinator.agent", &json!("OpenCode/anthropic/claude-haiku-5-5#high"), &you()).unwrap().value, json!("opencode/anthropic/claude-haiku-5-5#high")); // a variant in the name is a model name too
    assert_eq!(set_value("worker.agent", &json!("opencode"), &you()).unwrap().value, json!("opencode"));
    let e = set_value("worker.agent", &json!("opencode/openai/gpt-5.5"), &you()).unwrap_err().msg;
    assert!(e.contains("opencode has no model \"openai/gpt-5.5\".") && e.contains("Did you mean openai/gpt-6-sol?") && e.contains("ollama/gemma-4-e4b (from `opencode models`)"), "{e}");
    // opencode is in the list of the programs, and refuses `ask` nowhere.
    let e = set_value("worker.agent", &json!("gpt"), &you()).unwrap_err().msg;
    assert!(e.contains("It must be claude, codex, pi, opencode, "), "{e}");
    assert_eq!(set_value("worker.access", &json!("ask"), &you()).unwrap().value, json!("ask"));
    assert!(pick_of("opencode/ollama/gemma-4-e4b", "").unwrap().unwrap().s("model") == "ollama/gemma-4-e4b");
}

/// OpenCode's own config with an OpenAI-compatible gateway (as a user writes it, `.jsonc` with comments, the key by
/// `{file:…}`), and the gateway's inventory as the board keeps it (the tests never ask a gateway).
fn gateway_config(w: &World, inventory: &[&str]) {
    let home = std::path::PathBuf::from(std::env::var("XDG_CONFIG_HOME").unwrap()).join("opencode");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(home.join("gw.key"), "k-1\n").unwrap();
    std::fs::write(home.join("opencode.jsonc"), format!(r#"{{
  // my gateway
  "$schema": "https://opencode.ai/config.json",
  "providers": {{ "gw": {{ "name": "gw", "package": "@opencode/ai/providers/openai-compatible",
    "settings": {{ "baseURL": "https://gw.example/v1", "apiKey": "{{file:{}}}" }},
    "models": {{ "claude-haiku-5-5": {{ "name": "claude-haiku-5-5" }}, }} }} }},
}}"#, home.join("gw.key").display())).unwrap();
    let inv = w.base.join("inventory.json");
    std::fs::write(&inv, json!({ "names": inventory }).to_string()).unwrap();
    unsafe { std::env::set_var("HERDR_PROJECTS_OPENCODE_INVENTORY", &inv) };
}

#[test]
fn a_gateways_whole_inventory_is_selectable_not_only_what_opencodes_config_names() {
    let w = world();
    let you = || Scope::default();
    gateway_config(&w, &["gw/claude-haiku-5-5", "gw/claude-sonnet-4-6", "gw/gpt-image-2"]);
    // The config names one model: OpenCode lists that one; the gateway serves more, read with the config's own key file.
    assert_eq!(sw::agents::opencode_models::gateways(), vec![("gw".to_string(), "https://gw.example/v1".to_string(), "k-1".to_string())]);
    lists(&["gw/claude-haiku-5-5"]);
    // The list is widened with the inventory whenever it is read: its chat models are taken and shown, the image
    // model taken but not shown (it is listed, so it is not refused as unknown; it is never offered).
    let names = match sw::agents::models::names_of("opencode", false) { sw::agents::models::Listing::Names(n) => n, other => panic!("{other:?}") };
    assert!(names.taken.contains(&"gw/claude-sonnet-4-6".to_string()) && names.taken.contains(&"gw/gpt-image-2".to_string()), "{:?}", names.taken);
    assert!(names.shown.contains(&"gw/claude-sonnet-4-6".to_string()) && !names.shown.contains(&"gw/gpt-image-2".to_string()), "{:?}", names.shown);
    assert_eq!(set_value("worker.agent", &json!("opencode/gw/claude-sonnet-4-6"), &you()).unwrap().value, json!("opencode/gw/claude-sonnet-4-6"));
    let e = set_value("worker.agent", &json!("opencode/gw/nope-9"), &you()).unwrap_err().msg;
    assert!(e.contains("opencode has no model \"gw/nope-9\"") && e.contains("gw/claude-sonnet-4-6") && !e.contains("gpt-image-2"), "{e}");
    let e = set_value("worker.agent", &json!("opencode/gw/gpt-image-2"), &you()).unwrap_err().msg;
    assert!(e.contains("takes no conversation") && e.contains("gw/claude-sonnet-4-6"), "{e}");
    // The gateway refused its list (a 401, a quota): an unknown name's refusal says so, instead of a plain "no such model".
    let inv = sw::store::files::data_dir().join("opencode").join("inventory.json"); std::fs::create_dir_all(inv.parent().unwrap()).unwrap();
    std::fs::write(&inv, json!({ "gw": { "at": 1, "ids": ["claude-sonnet-4-6"], "failed": { "at": 2, "why": "HTTP 401: {\"error\":\"Invalid API key\"}" } } }).to_string()).unwrap();
    assert_eq!(sw::agents::opencode_models::failed(), vec!["gw: HTTP 401: {\"error\":\"Invalid API key\"}".to_string()]);
    let e = set_value("worker.agent", &json!("opencode/gw/nope-9"), &you()).unwrap_err().msg;
    assert!(e.contains("A gateway's inventory could not be read") && e.contains("gw: HTTP 401"), "{e}");
    // The launch file declares the chosen gateway model under its provider, so OpenCode will run it; nothing else of
    // the provider (its URL and key stay in OpenCode's own config), and nothing for a model of another provider.
    let dir = w.base.join("launch");
    let j: Value = serde_json::from_str(&std::fs::read_to_string(launch_file(&dir, "agent", "gw/claude-sonnet-4-6", "high", "full").unwrap()).unwrap()).unwrap();
    assert_eq!(j.s("model"), "gw/claude-sonnet-4-6#high");
    assert_eq!(j.at("providers"), &json!({ "gw": { "models": { "claude-sonnet-4-6": { "name": "claude-sonnet-4-6" } } } }));
    let j: Value = serde_json::from_str(&std::fs::read_to_string(launch_file(&dir, "agent", "anthropic/claude-opus-5-5", "", "full").unwrap()).unwrap()).unwrap();
    assert!(!j.has("providers"));
    unsafe { std::env::remove_var("HERDR_PROJECTS_OPENCODE_INVENTORY") };
}

#[test]
fn opencodes_conversation_is_read_from_the_boards_copy_of_its_rows() {
    let _w = world();
    let done = read_turn("opencode", &fixture("opencode-finished-made"), 1 << 20).unwrap();
    assert_eq!((done.finished, done.said.as_str(), done.id.as_str(), done.model.as_str(), done.stop.as_str()), (true, "Carried on and finished it.", "msg_141a2c444001PX84FxLgegaseV", "claude-haiku-5-5", "stop"));
    assert_eq!(done.usage.as_ref().map(|u| (u.inp, u.out)), Some((13336.0, 16.0)));
    let login = read_turn("opencode", &fixture("opencode-unauthorized"), 1 << 20).unwrap();
    assert_eq!((login.stop.as_str(), login.error.as_str(), login.auth, login.passing, login.finished), ("error", "Invalid API key", true, false, false));
    // The human stopped it (Esc): an assistant row with `error.type: aborted` and no idle row after it.
    let stopped = read_turn("opencode", &fixture("opencode-interrupted-made"), 1 << 20).unwrap();
    assert_eq!((stopped.stop.as_str(), stopped.interrupted, stopped.error.as_str(), stopped.finished, stopped.said.as_str()), ("interrupted", true, "", false, "Reading the file now."));
    let no_account = read_turn("opencode", &fixture("opencode-no-account-made"), 1 << 20).unwrap();
    assert!(no_account.error.starts_with("auth_unavailable: no auth available"), "{}", no_account.error);
    assert_eq!((no_account.passing, no_account.reason.as_str(), no_account.auth), (false, "", false)); // a provider's 503 for a model no account serves is lasting: its words say "not supported"
    // A turn still going: the last row is the human's, or a reply that called a tool.
    let dir = std::env::temp_dir().join(format!("sw-oc-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let rows = |list: &[Value]| { let f = dir.join("c.jsonl"); std::fs::write(&f, list.iter().map(|r| format!("{r}\n")).collect::<String>()).unwrap(); f };
    let user = json!({ "type": "user", "seq": 1, "at": 1000, "data": { "text": "Fix it", "time": { "created": 1000 } } });
    let tool = json!({ "type": "assistant", "seq": 2, "at": 2000, "data": { "id": "m2", "time": { "created": 2000, "completed": 2500 }, "model": { "id": "gpt-6-sol", "providerID": "openai" }, "content": [{ "type": "text", "text": "Looking." }, { "type": "tool", "id": "t1", "name": "shell", "state": { "status": "completed", "input": { "command": "ls" } } }], "finish": "tool-calls", "tokens": { "input": 10, "output": 5, "reasoning": 0, "cache": { "read": 0, "write": 0 } } } });
    let t = read_turn("opencode", &rows(&[user.clone()]), 1 << 20).unwrap();
    assert!(t.pending && !t.finished); assert_eq!(t.began, 1000);
    let t = read_turn("opencode", &rows(&[user.clone(), tool.clone()]), 1 << 20).unwrap();
    assert!(t.pending); assert_eq!(t.said, "Looking.");
    let idle = json!({ "type": "idle", "seq": 3, "at": 3000, "data": { "time": { "created": 3000 }, "outcome": "succeeded" } });
    let t = read_turn("opencode", &rows(&[user.clone(), tool.clone(), idle.clone()]), 1 << 20).unwrap();
    assert!(t.finished); assert_eq!((t.said.as_str(), t.model.as_str(), t.pos), ("Looking.", "gpt-6-sol", 3000));
    // A reply cut off at its length, or refused (OpenCode's `content-filter`), keeps that ending through the idle row
    // OpenCode always writes after it (a failed one, for a refusal).
    let cut = json!({ "type": "assistant", "seq": 2, "at": 2000, "data": { "id": "m2", "time": { "created": 2000, "completed": 2500 }, "content": [{ "type": "text", "text": "Half of" }], "finish": "length" } });
    let t = read_turn("opencode", &rows(&[user.clone(), cut, idle.clone()]), 1 << 20).unwrap();
    assert_eq!((t.stop.as_str(), t.finished, t.pos), ("length", true, 3000));
    let refused = json!({ "type": "assistant", "seq": 2, "at": 2000, "data": { "id": "m2", "time": { "created": 2000, "completed": 2500 }, "content": [], "finish": "content-filter" } });
    let failed = json!({ "type": "idle", "seq": 3, "at": 3000, "data": { "time": { "created": 3000 }, "outcome": "failed" } });
    let t = read_turn("opencode", &rows(&[user.clone(), refused, failed]), 1 << 20).unwrap();
    assert_eq!((t.stop.as_str(), t.finished, t.error.as_str()), ("refusal", true, ""));
    // Whether a message went in: its words on a user row since a moment, by the row's own `at`.
    let f = rows(&[user.clone()]);
    assert!(sw::agents::read_turn::holds(&f, "Fix it", 0).unwrap() && sw::agents::read_turn::holds(&f, "Fix it", 1000).unwrap());
    assert!(!sw::agents::read_turn::holds(&f, "Fix it", 5000).unwrap() && !sw::agents::read_turn::holds(&f, "Something else", 0).unwrap());
    // The chat's words: what was said, by whom, nothing of the tools.
    let f = rows(&[user, tool, json!({ "type": "assistant", "seq": 4, "at": 4000, "data": { "id": "m4", "content": [{ "type": "text", "text": "Done." }], "finish": "stop", "time": { "created": 4000, "completed": 4100 } } })]);
    assert_eq!(talk_of("opencode", Some(&f), "", Path::new("/"), 10).unwrap().iter().map(|m| format!("{}: {}", if m.you { "you" } else { "them" }, m.text)).collect::<Vec<_>>(), vec!["you: Fix it", "them: Looking.", "them: Done."]);
    // The tokens: every reply's own use, cache included.
    let n = sw::agents::read_tokens::tokens("opencode", &f, &Value::Null).unwrap();
    assert_eq!(n.f("n"), 15.0);
    // Where its file is: the board's own copy, by the conversation's id.
    assert_eq!(transcript("opencode", "ses_none", Path::new("/")), None);
    let f = mirror_file("ses_abc"); std::fs::create_dir_all(f.parent().unwrap()).unwrap(); std::fs::write(&f, "").unwrap();
    assert_eq!(transcript("opencode", "ses_abc", Path::new("/")).as_ref(), Some(&f));
    let _ = std::fs::remove_dir_all(&dir);
}

fn coord_world(agent: &str) -> World {
    let w = world_with(Opts { repo: true, settings: json!({ "coordinator.agent": agent, "coordinator.access": "ask", "review.when": "never" }), ..Default::default() });
    lists(NAMES);
    let home = w.base.join("home-coord");
    std::fs::create_dir_all(&home).unwrap();
    unsafe { std::env::set_var("HERDR_PROJECTS_COORDINATOR_DIR", &home) };
    w
}
fn drain() { flush(); sw::runner::drain_jobs(&|job, id, extra| { sw::jobs::run_job(job, id, extra); flush(); }); }
fn agent(w: &World) -> Value { w.state().arr("panes").iter().find(|p| p.at("agent").is_object()).map(|p| p.at("agent").clone()).unwrap() }
fn strs(v: &Value) -> Vec<String> { v.as_array().map(|a| a.iter().map(text).collect()).unwrap_or_default() }

#[test]
fn the_coordinator_runs_on_opencode_asks_before_it_acts_and_a_new_one_switches_the_model() {
    let w = coord_world("opencode/anthropic/claude-haiku-5-5");
    let home = state::home(&w.socket);
    w.script(json!({ "coordinator": [{ "ends": "opencode-finished-made" }] }));
    send(&w.root, "list the tasks", false, "", &w.socket).unwrap(); drain();
    let a = agent(&w);
    // Started by a command at the pane's shell, with its launch file in the Coordinator's own folder and no --auto
    // (coordinator.access ask: the file's rules ask before an edit or a command).
    assert_eq!(a.s("kind"), "opencode"); assert!(!a.b("boxed"));
    let file = state::dir_of(&w.socket).join("opencode-coordinator.json");
    let env = strs(a.at("env")); // the fake pane read the file for the shell
    assert_eq!(env.len(), 2); assert_eq!(env[1], "OPENCODE_DISABLE_AUTOUPDATE=1");
    assert_eq!(serde_json::from_str::<Value>(env[0].strip_prefix("OPENCODE_CONFIG_CONTENT=").unwrap()).unwrap().s("model"), "anthropic/claude-haiku-5-5"); // the file's text, inline
    let args = strs(a.at("args"));
    assert_eq!(&args[..2], &["--standalone", "--session"]); assert!(args[2].starts_with("ses_")); assert_eq!(args.len(), 3);
    let j: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(j.s("model"), "anthropic/claude-haiku-5-5"); // the file carries the model: no flag
    assert_eq!(j.arr("permissions").iter().filter(|r| r.s("effect") == "ask").count(), 4); assert!(j.arr("permissions").iter().all(|r| r.s("effect") == "ask"));
    // Its rules are AGENTS.md in its folder, as for pi: no line before the first message.
    assert!(std::fs::read_to_string(home.join("AGENTS.md")).unwrap().contains("board add \"<short title"));
    assert_eq!(strs(a.at("prompts")), vec!["list the tasks"]);
    // Its answer is read from the board's copy of its rows (the fake wrote it as the board would).
    let c = state::get(&w.socket).unwrap();
    assert!(c.s("session").starts_with("ses_")); assert!(mirror_file(c.s("session")).exists());
    let v = look(&w.socket);
    assert_eq!(v.said, "Carried on and finished it."); assert!(v.readable);
    assert_eq!(v.talk.iter().map(|m| m.text.clone()).collect::<Vec<_>>(), vec!["list the tasks", "Carried on and finished it."]);
    // A new Coordinator on another model, with an effort: the file is written again and carries both.
    w.script(json!({ "coordinator": [{ "ends": "opencode-finished-made" }] }));
    fresh(pick_of("opencode/openai/gpt-6-sol", "high").unwrap(), &w.socket).unwrap(); drain();
    send(&w.root, "and now?", false, "", &w.socket).unwrap(); drain();
    let j: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(j.s("model"), "openai/gpt-6-sol#high");
    assert_eq!(look(&w.socket).run.unwrap().s("model"), "openai/gpt-6-sol");
    // A model OpenCode does not list is refused before anything starts.
    let e = pick_of("opencode/openai/gpt-5.5", "").unwrap_err().msg;
    assert!(e.contains("opencode has no model \"openai/gpt-5.5\""), "{e}");
    unsafe { std::env::remove_var("HERDR_PROJECTS_COORDINATOR_DIR") };
}

#[test]
fn a_task_on_opencode_is_started_the_same_way_and_a_login_error_in_its_conversation_waits_for_you() {
    let w = world_with(Opts { repo: true, settings: json!({ "worker.agent": "opencode/ollama/gemma-4-e4b", "review.when": "never" }), ..Default::default() });
    lists(NAMES);
    let id = w.add("Something");
    w.script(json!({ "worker": [{ "ends": "opencode-unauthorized" }] }));
    delegate(&id);
    let t = settle(&id);
    let a = w.pane_of(&t).at("agent").clone();
    assert_eq!(a.s("kind"), "opencode");
    let args = strs(a.at("args"));
    assert_eq!(&args[..2], &["--standalone", "--session"]); assert!(args.contains(&"--auto".to_string()));
    let file = sw::store::files::dir_of(&id).join("opencode-agent.json");
    assert!(strs(a.at("env"))[0].starts_with("OPENCODE_CONFIG_CONTENT={"));
    assert_eq!(serde_json::from_str::<Value>(&std::fs::read_to_string(&file).unwrap()).unwrap().s("model"), "ollama/gemma-4-e4b");
    assert_eq!((t.s("stage"), t.at("failure").b("auth")), ("failed", true));
    assert!(t.s("msg").contains("Needs a login"), "{}", t.s("msg"));
}
