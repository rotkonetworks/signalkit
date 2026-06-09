# AGENTS.md — signalkit

Linux-only Rust CLI that reads Signal Desktop history and sends/receives via
presage (linked secondary device). Stdout is **JSON by default**; pass
`--pretty` for human output.

## Binary

```
./target/debug/signalkit  (or ./target/release/signalkit after `cargo build --release`)
```

## Mental model

- **History & search** → read from Signal Desktop's SQLCipher DB (`list`, `read`, `search`). No network. Fast.
- **Sending and live receive** → presage, talks to Signal servers. Requires one-time `link` (QR scan with phone).
- Messages you `send` via presage appear in the Desktop DB shortly after (Signal Desktop syncs as another linked device).

## Environment

| Var                | Default                                    | Purpose                                   |
| ------------------ | ------------------------------------------ | ----------------------------------------- |
| `SIGNAL_DIR`       | `~/.config/Signal` (or Flatpak path)       | Signal Desktop data dir (for read ops)    |
| `SIGNALKIT_STORE`  | `~/.local/share/signalkit/presage.sqlite3` | presage protocol/state store (for live)   |
| `RUST_LOG`         | unset                                      | `info`, `debug`, etc. — logs go to stderr |

## Commands

All commands accept `--pretty` to switch from JSON (default) to human output.

### `list [--include-empty]`
Lists chats, sorted by recent activity. JSON: array of `Chat`.
```
signalkit list                # JSON array
signalkit list --pretty       # "<msg-count>  <name>  <conv-id>"
```

### `read <chat> [--limit N] [--offset N]`
Newest-first messages from one chat. `<chat>` is the display name from `list`.
```
signalkit read "Freddy" --limit 20
signalkit read "Mikko Niemi" --limit 200 --offset 0
```

### `search <query> [--chat <name>] [--limit N]`
Substring search (case-insensitive). Omit `--chat` to search all conversations.
```
signalkit search "kusama"
signalkit search "invoice" --chat "Eric" --limit 50
```

### `link [--device-name NAME]`
One-time. Prints a QR code; scan with Signal on phone → Settings → Linked devices → +.
Blocks until phone approves. Writes credentials to `SIGNALKIT_STORE`.

### `whoami`
Network call. Returns the linked account's `aci`, `pni`, `number`.
```
signalkit whoami --pretty
# aci:    8cbb89b9-1c1a-4791-a322-75688d87f691
# pni:    8970f645-4c3e-4d43-9d57-476f054be612
# number: E164(358404423267)
```

### `send <recipient-aci-uuid> <body>`
Send a 1:1 text message. Recipient is an ACI UUID, **not** a phone number.
Sent message also lands in Desktop DB after sync.
```
signalkit send 8cbb89b9-1c1a-4791-a322-75688d87f691 "ping"
```
- Self-send (your own ACI) goes to Note to Self.

### `send-group <master-key-b64> <body>`
Send a text message to a Signal group v2. The master key is the group's 32-byte
GroupV2 master key, base64-encoded. There's no built-in lookup helper yet —
extract it from your Signal Desktop / presage store manually, or wait for
`list_groups`.
```
signalkit send-group "AAA...32bytes-base64..." "lunch?"
```

### `recv`
Subscribes to the live message stream. Prints one JSON object per message
until Ctrl-C. Format: `{"from": "...", "body": "...", "timestamp": <ms>, "has_attachments": <bool>}`.
First run after time away may take ~10-30s to drain backlog.

### `serve`
Runs as an MCP (Model Context Protocol) server over stdio. Speaks JSON-RPC 2.0,
one message per line. Exposes 5 tools: `list_chats`, `read_chat`, `search`,
`whoami`, `send`. Intended to be driven by Claude Desktop or any MCP client.

Claude Desktop config (`~/.config/Claude/claude_desktop_config.json`):
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

## Output schemas

`Chat`:
```json
{
  "id": "01989cad-...",
  "name": "Freddy" | null,
  "profile_name": "..." | null,
  "profile_full_name": "..." | null,
  "e164": "+358..." | null,
  "service_id": "uuid" | null,
  "group_id": "..." | null,
  "kind": "private" | "group" | null,
  "active_at": 1780000000000 | null,
  "total_messages": 30733
}
```

`MessageRow` (from `read` / `search`):
```json
{
  "id": "msg-uuid",
  "conversation_id": "...",
  "sent_at": 1780601019215 | null,
  "received_at": 1780601019300 | null,
  "body": "..." | null,
  "source": "+358404423267" | null,
  "kind": "incoming" | "outgoing" | "...",
  "has_attachments": false
}
```

## MCP tools

`signalkit serve` exposes 6 tools over MCP stdio:

| Tool             | Purpose                                                      | Needs link? |
| ---------------- | ------------------------------------------------------------ | ----------- |
| `list_chats`     | All chats with metadata + message counts                     | no          |
| `read_chat`      | Messages in a chat (date range optional)                     | no          |
| `search`         | Substring search across messages                             | no          |
| `find_recipient` | Resolve a name to an ACI UUID (sorted by message count)      | no          |
| `whoami`         | The linked account's ACI / PNI / number                      | yes         |
| `send`           | Send a 1:1 text message                                      | yes         |
| `send_group`     | Send to a group v2 by master key (base64; lookup TBD)        | yes         |

Typical agent flow for "send X to Alice":
```
1. find_recipient { name: "Alice" }       → { service_id, display_name, ... }
2. send { to: <service_id>, body: "X" }   → { timestamp }
```

## Common CLI recipes

Find a contact's ACI to send to (CLI side):
```
signalkit list | jq -r '.[] | select(.name | test("freddy"; "i")) | .service_id'
# the agent-facing equivalent is the find_recipient MCP tool
```

Search then send a reply:
```
LATEST=$(signalkit search "from PM" --chat "Eric" --limit 1)
echo "$LATEST" | jq -r '.[0].body'
ACI=$(signalkit list | jq -r '.[] | select(.name=="Eric") | .service_id')
signalkit send "$ACI" "got it, thanks"
```

Watch for new messages in a script:
```
signalkit recv | while read line; do
  echo "$line" | jq -r '"\(.from): \(.body)"'
done
```

## Errors

- `Signal Desktop directory not found` — set `SIGNAL_DIR` or install Signal Desktop.
- `libsecret has no entry for application="Signal"` — Signal Desktop hasn't run on this user, or libsecret/gnome-keyring isn't available.
- `presage: load: …` — store not linked. Run `signalkit link` first.
- `bundle not opened` (Tauri only) — frontend called a command before `open_bundle` resolved.

## License

AGPL-3.0-only (inherited from presage). Distributing the binary or exposing it
over a network triggers source-disclosure obligations.
