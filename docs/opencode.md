# OpenCode

`opencode` is a fourth program the board can run, for every role but the Watcher: `worker.agent`, `reviewer.agent`,
`coordinator.agent`, `plan.agent`, the fallbacks and `attempts.by` all take `opencode`, with a model after a slash as
for any other program. OpenCode names a model by its provider first, so the board does too:
`opencode/anthropic/claude-opus-5-5`, `opencode/openai/gpt-6-sol`, `opencode/ollama/gemma-4-e4b`. Checked on OpenCode
2.0.25 (`docs/checked-on-herdr.md`).

**The providers are yours.** The board declares no provider and holds no key: OpenCode's own config
(`~/.config/opencode/opencode.json`) and sign-ins (`opencode auth login`) say which providers it reaches, and
`opencode models` lists every `provider/model` they give. A model after `opencode/` must be one of those
(`herdr-projects models opencode` shows them); with nothing listed the name goes to OpenCode as written. No model
named: OpenCode's own default.

**A gateway's whole inventory is selectable.** For an OpenAI-compatible provider in that config (a gateway in front
of several vendors, say), OpenCode lists only the models written under it by hand. The board asks such a provider
itself for what it serves (`GET <baseURL>/models`, with the key the config names, substituted as OpenCode would:
`{env:…}`, `{file:…}`), at most once a day (`data/opencode/inventory.json`), and takes those names too, as
`<provider>/<id>`. The ones that answer a chat are offered; an image, embedding, speech or review-only model is
known but never offered. A gateway that refuses its list (a wrong key, a quota) keeps what it said last, and
`herdr-projects models opencode` and the refusal of an unknown name say why. A model chosen that way is declared under its provider in the launch file, so OpenCode
runs it; the config itself is never touched.

**One launch config per start.** OpenCode takes its model and its permission rules from config and from nothing on
its command line, so each start writes one small file (`opencode-<role>.json`, mode 0600, in the task's own folder or
the Coordinator's) and has the pane's shell hand its text to the program inline (`OPENCODE_CONFIG_CONTENT="$(cat …)"`:
a terminal cuts a typed line near a thousand bytes, so the text is not typed). OpenCode
layers its config: its own global file, then `OPENCODE_CONFIG`, then the repo's own `opencode.json`, then the inline
text; a board agent works inside a repo, whose own file would replace the chosen model and cancel the ask rules if the
board's came before it, so the board's comes last. Your providers stay; the config holds only the model, the rules of
the role's access, `update: disable` (no self-update under a running agent) and, for a gateway model your config does
not name, that one model entry:

| `<role>.access` | What the file says | And on the command line |
|---|---|---|
| `full` | nothing: OpenCode's own rules stand | `--auto` |
| `ask` | ask before an edit, a shell command, a fetch, anything outside the folder and a tool of an MCP server; hand work only to OpenCode's own two subagents; at the top and for each of OpenCode's agents; the conversation runs as OpenCode's `build`, kept a visible primary agent | nothing |

The config's rules come after OpenCode's own (among them: ask before reading a `.env` file), so it never writes a
catch-all `allow`: that would override those and let an agent in `ask` mode read a secrets file without asking. A
repo's own `opencode.json` is read before the board's config, but it has more than one way around the rules, each
closed: an agent's own rules (`agents.build.permissions`, say) come after the top-level ones, so the ask rules are
written for each of OpenCode's agents too (`build`, `plan`, `general`, `explore`, and the three it uses by itself); a
repo could make a primary agent of its own the default and hide `build`, so the conversation is pinned to `build`,
kept a visible primary agent; and a repo could add a subagent of its own with its own rules, so work is handed only
to OpenCode's own `general` and `explore` (OpenCode 2.0 checks the handing under `subagent`; its pages call it `task`;
both are written). The config holds only keys OpenCode's schema has: one it lacks makes OpenCode drop the whole
config, and the repo's with it, without a word.

So a Coordinator with `coordinator.access ask` asks in its own pane before it acts, as it does on Claude Code. The
program is started by a command typed at the pane's shell (`env OPENCODE_CONFIG_CONTENT="$(cat …)" opencode …`), the way a plan's
writer is started in its read-only mount, because herdr's `agent start` hands a program no environment; herdr then
finds the agent in the pane by itself. `--standalone` keeps every board agent out of OpenCode's shared background
service, so one agent's restart never touches another's.

**The model and the effort are the file's.** OpenCode's interactive program takes no model flag (2.0: only its
headless `run` does), so the launch file's `model` names `<provider>/<model>#<effort>`: the effort word is the model's
variant (`high`, `max`, `minimal`, …), which the model must have, or OpenCode refuses the turn and the task says so;
`default` adds no variant. The file is written again at every start, so the model the board chose is the one that
runs, also on a conversation reopened with another model: `coordinator new --agent opencode/<other>` switches it.

**Conversations.** OpenCode keeps no file per conversation: every message is a row in one SQLite database
(`~/.local/share/opencode/opencode.db`). The board chooses each conversation's id (`--session ses_…` makes one that
does not exist and reopens one that does) and, whenever it reads a conversation, copies its rows into one JSON-lines
file of its own (`data/opencode/<session>.jsonl`, written again only when the database has more or newer rows),
which its readers take as they take any other program's file: the last message, a finished or a pending turn, an
error with its status (a turn you stopped with Esc is an interrupt, not an error; a reply cut off at its length, or refused by the model's filter, says so; a `401` is a login; a provider's `503` for a model no account serves is lasting when its words
say the model is not supported; an overload or a dropped connection passes by itself and is retried), the tokens and
the model of each reply, and the words of the chat. The copy needs `sqlite3` on the machine (macOS has it; `apt
install sqlite3`). A new chat the program begins in the Coordinator's folder is followed as for the other programs:
the newest top-level conversation saved for that folder since the Coordinator began is its own.

**What it has not.** No turn-end hook: a turn's end is read from the saved conversation when herdr says the agent
is idle, as for a program with its hook marked off, and the closing note is still asked for. No plan mode of its
own (`plan.approve` runs it in the read-only mount like Codex and pi). No one-shot call: `opencode run` keeps every
answer as a conversation in its database, so it is not used for the Watcher. No fast mode. Its own `AGENTS.md` is
read from the folder it starts in, so the Coordinator's rules reach it the same way as pi's.
