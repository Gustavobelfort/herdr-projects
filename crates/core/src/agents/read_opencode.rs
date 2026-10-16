//! Reading the end of an OpenCode conversation, from the board's copy of its database rows (agents/opencode.rs).
//! One reader, in the shape of the ones in read_turn.rs: every row is one line, { type, seq, at, data }.
use super::read_turn::{judge, tail_lines, usage_of, Turn};
use crate::json::*;
use std::path::Path;

/// OpenCode's rows, as the board copies them out of its database (agents/opencode.rs): one line per message,
/// { type: user | assistant | idle | compaction, seq, at, data }. An assistant row carries its text and tool parts,
/// `finish` (stop, length, content-filter for a refused reply, tool-calls, error) and, cut off, `error: { type, message, status }`; an `idle` row ends a turn
/// (`outcome`: succeeded, failed); a failed compaction carries the provider's error too. A turn the human stopped is an
/// assistant row with `error.type: aborted` and no idle row after it.
pub fn read_opencode(file: &Path, bytes: u64) -> Option<Turn> {
    let rows = tail_lines(file, bytes).ok()?;
    #[derive(PartialEq)] enum K { Said, Tool, User, Idle, Error, Interrupt }
    let mut last: Option<(K, &Value, String)> = None; // kind, row, error text
    let (mut said, mut usage, mut model, mut began, mut id) = (String::new(), None::<(f64, f64)>, String::new(), 0, String::new());
    let err_of = |e: &Value| { let m = if e.b("message") { e.t("message") } else { text(e) }; if m.is_empty() { "error".into() } else { m } };
    for r in &rows {
        let d = r.at("data");
        match r.s("type") {
            "user" => { last = Some((K::User, r, String::new())); began = r.i("at"); }
            "assistant" => {
                let tx = d.arr("content").iter().filter(|c| c.s("type") == "text").map(|c| c.t("text")).collect::<Vec<_>>().join("").trim().to_string();
                if !tx.is_empty() { said = tx; }
                if d.at("model").b("id") { model = d.at("model").t("id"); }
                if d.at("tokens").is_object() { let u = d.at("tokens"); usage = Some((u.f("input") + u.at("cache").f("read") + u.at("cache").f("write"), u.f("output") + u.f("reasoning"))); }
                id = d.t("id");
                last = Some(match d.s("finish") {
                    "error" if d.at("error").s("type") == "aborted" => (K::Interrupt, r, String::new()), // the human stopped it (Esc): no idle row follows
                    "error" => (K::Error, r, err_of(d.at("error"))),
                    "stop" | "length" | "content-filter" => (K::Said, r, String::new()),
                    "" => (if d.at("time").b("completed") { K::Said } else { K::Tool }, r, String::new()),
                    _ => (K::Tool, r, String::new()),
                });
            }
            "compaction" if d.s("status") == "failed" && d.at("error").is_object() => { last = Some((K::Error, r, err_of(d.at("error")))); }
            "idle" => {
                let outcome = d.s("outcome");
                if let Some((K::Error | K::Interrupt | K::Said, _, _)) = &last { continue; } // the reply's own row says how it ended
                if outcome == "failed" { last = Some((K::Error, r, "the turn failed".into())); }
                else if outcome == "cancelled" || outcome == "aborted" { last = Some((K::Interrupt, r, String::new())); }
                else { last = Some((K::Idle, r, String::new())); }
            }
            _ => {}
        }
    }
    let (kind, r, err) = last?;
    let d = r.at("data");
    let status = d.at("error").i("status");
    let ended = rows.iter().rev().find(|x| x.s("type") == "idle").map(|x| x.i("at")).unwrap_or(0); // the turn's end, when its idle row came after
    let base = Turn { pos: r.i("at").max(d.at("time").i("completed")).max(ended), began, usage: usage.and_then(|(i, o)| usage_of(i, o, &model, None)), model: model.clone(), id: if id.is_empty() { r.i("seq").to_string() } else { id }, ..Default::default() };
    Some(match kind {
        K::Error => judge(Turn { said: said.clone(), stop: "error".into(), error: err, ..base }, d.at("error").s("type"), status, d.at("error").s("type") == "provider.auth"),
        K::Interrupt => Turn { said, stop: "interrupted".into(), interrupted: true, ..base },
        K::Idle | K::Said => { let stop = d.s("finish"); Turn { said: said.trim().to_string(), stop: if stop == "length" { "length".into() } else if stop == "content-filter" { "refusal".into() } else { "stop".into() }, finished: true, ..base } }
        _ => Turn { pending: true, said: said.trim().to_string(), ..base },
    })
}
