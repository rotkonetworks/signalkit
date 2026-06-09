# signalkit

**Search and browse your Signal history**, locally on your machine. Then send
and receive messages too — signalkit can register as a Signal secondary device
(same mechanism Signal Desktop uses) via
[`presage`](https://github.com/whisperfish/presage).

Comes as three things, all sharing the same engine:

- A **Tauri desktop app** with a two-pane chat browser + date-range filter +
  compose box + live "Listen" toggle.
- A **CLI** (`signalkit`) that does the same operations from a terminal — handy
  for grep / jq pipelines.
- An **MCP server** (`signalkit serve`) so AI agents (Claude Desktop, Claude
  Code, anything speaking the Model Context Protocol) can drive Signal on
  your behalf — search for messages, find recipients, send replies.

See **[AGENTS.md](AGENTS.md)** for the agent-facing command surface.

**Status:** Linux only. macOS / Windows support not wired yet (decryption paths
differ — see Roadmap).

## What you get

| Surface           | What it does                                                       |
| ----------------- | ------------------------------------------------------------------ |
| `signalkit` CLI   | `list`, `read`, `search` against Desktop DB; `link`, `send`, `recv`, `whoami` via presage |
| Tauri desktop app | Browse + per-chat search UI + date filter + compose & send + "Live" polling toggle |
| MCP server        | `signalkit serve` over stdio — exposes `list_chats`, `read_chat`, `search`, `whoami`, `send` to agents like Claude Desktop |

## Requirements

- Rust 1.85+ (workspace uses 2024 edition transitively)
- Node 18+ and `pnpm` (only if you want the Tauri UI)
- Signal Desktop installed and signed in on this machine (so there's a DB to read)
- Linux with libsecret / gnome-keyring (for Signal Desktop's encrypted DB key)
- A C toolchain (for bundled SQLCipher)

## Build

```bash
git clone <this-repo>
cd signal-mcp-server
cargo build --release -p signalkit-cli
# binary at ./target/release/signalkit
```

For the desktop app:
```bash
cd crates/signalkit-tauri
pnpm install
pnpm tauri dev          # development, hot-reload UI
# or: pnpm tauri build  # bundle a release app
```

## First-time setup

### 1. Read-only history (works immediately)

If Signal Desktop is signed in on this machine, this works with no further setup:

```bash
./target/release/signalkit list --pretty
./target/release/signalkit read "Alice" --limit 50 --pretty
./target/release/signalkit search "invoice" --pretty
```

It decrypts the Signal Desktop database in place. Read-only, never writes.

### 2. Link as a secondary device (needed to send / live-receive)

```bash
./target/release/signalkit link
```

A QR code will print in your terminal. On your phone:

1. Open Signal.
2. **Settings → Linked devices → +** (Android: tap the plus; iOS: "Link New Device").
3. Scan the QR code in the terminal.
4. Tap **Link this device** on the phone.

The CLI prints `linked!` when done. Credentials live at
`~/.local/share/signalkit/presage.sqlite3`. Delete that file to unlink.

### 3. Send a test message to yourself

```bash
./target/release/signalkit whoami --pretty
# aci:    8cbb89b9-1c1a-4791-a322-75688d87f691
# ...

./target/release/signalkit send 8cbb89b9-1c1a-4791-a322-75688d87f691 "hi, me"
# sent (timestamp 1780601019215)
```

Check **Note to Self** on your phone — message should be there within a second.

### 4. Watch for live messages

```bash
./target/release/signalkit recv --pretty
# leave running; messages print as they arrive
```

### 5. Use as an MCP server for agents

Wire signalkit into Claude Desktop (or any MCP client). Example
`~/.config/Claude/claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "signalkit": {
      "command": "/absolute/path/to/signalkit",
      "args": ["serve"]
    }
  }
}
```

The agent will see five tools: `list_chats`, `read_chat`, `search`, `whoami`,
`send`. Read-only tools work as soon as Signal Desktop is signed in;
`whoami` and `send` require `signalkit link` first.

## Layout

```
crates/
  signalkit-core/       # library: Desktop DB reader + presage wrapper
  signalkit-cli/        # bin: `signalkit` (list/read/search/link/send/recv/whoami)
  signalkit-tauri/      # Tauri 2 + SolidJS desktop app
server.py               # legacy Python MCP server (will be retired by `signalkit serve`)
```

## Configuration

| Env var            | Default                                    | Purpose                              |
| ------------------ | ------------------------------------------ | ------------------------------------ |
| `SIGNAL_DIR`       | `~/.config/Signal` or Flatpak path         | Where Signal Desktop stores its data |
| `SIGNALKIT_STORE`  | `~/.local/share/signalkit/presage.sqlite3` | presage's protocol state             |
| `RUST_LOG`         | unset                                      | `info`, `debug`, ...                 |

## Troubleshooting

**`libsecret has no entry for application="Signal"`** — Signal Desktop hasn't
created its keyring entry yet. Launch Signal Desktop once and link it to your
phone before running signalkit.

**`presage: load: …`** — You haven't linked yet. Run `signalkit link`.

**`Signal Desktop directory not found`** — Set `SIGNAL_DIR=/path/to/Signal`,
or install/launch Signal Desktop.

**Hangs on `link`** — Check your phone has Signal open and is online. The
QR has a short TTL; if it expires, rerun `signalkit link` for a fresh one.

**`presage.sqlite3.plain.bak` appeared in `~/.local/share/signalkit/`** —
That's the pre-encryption backup of your presage store, left there the first
time you run a build with at-rest encryption. The new `presage.sqlite3` is
SQLCipher-encrypted with a 32-byte random key stored in libsecret. Verify
`signalkit whoami` still works, then delete the `.plain.bak`.

## Roadmap

- [x] Read Desktop DB (chats / messages / search / date-range filter)
- [x] presage link / send / receive (CLI)
- [x] Tauri + SolidJS UI (browse / search / compose-and-send / Live polling)
- [x] `signalkit serve` — MCP stdio server (read tools + whoami + send + find_recipient)
- [x] At-rest encryption for presage store (libsecret-derived passphrase, SQLCipher)
- [x] Tauri live receive (subprocess `recv` + Tauri events; sub-second latency)
- [x] Group send (CLI `send-group`, MCP `send_group` — requires master key for now)
- [ ] `list_groups` / `find_group` (walk presage store for master keys)
- [ ] macOS keychain decryption path
- [ ] Windows DPAPI decryption path
- [ ] Attachment download

## Credits

This project stands on the shoulders of:

- **[presage](https://github.com/whisperfish/presage)** by the Whisperfish team
  — the Rust Signal client (link / send / receive). Pulls in
  [libsignal-service-rs](https://github.com/whisperfish/libsignal-service-rs)
  and Signal's own
  [libsignal](https://github.com/signalapp/libsignal). Without these, sending
  anything would be a multi-year reverse-engineering project.
- **[pensieve](https://github.com/hdevalence/pensieve)** by Henry de Valence
  — pioneered the local-Signal-Desktop-DB browser approach. The
  Chromium-OSCrypt + SQLCipher path is a Rust port of the same key-derivation
  trick pensieve documented.
- **[signal-export](https://github.com/carderne/signal-export)** by Chris Arderne
  — the original Python project this fork started from; the Desktop DB schema
  understanding came from there.
- **[Signal Foundation](https://signal.org/)** — for the protocol, the
  reference clients, and `libsignal`.
- **[Tauri](https://tauri.app/)**, **[SolidJS](https://www.solidjs.com/)**,
  **[rusqlite](https://github.com/rusqlite/rusqlite)**,
  **[secret-service](https://crates.io/crates/secret-service)** — the bones of
  the desktop app and the keychain plumbing.

## License

AGPL-3.0-only — inherited from [presage](https://github.com/whisperfish/presage).
See `LICENSE`.
