# Changelog

All notable changes to signalkit are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

## [0.1.0] — 2026-06-09

First public release. Linux only.

### Added

- **`signalkit-core`** library: Signal Desktop SQLCipher DB reader (libsecret
  OSCrypt v11 key derivation, pensieve-style), presage wrapper for
  send/receive over the real Signal protocol, and at-rest SQLCipher encryption
  of the presage store using a libsecret-derived random passphrase.
- **`signalkit` CLI** with subcommands:
  - `list`, `read`, `search` — query Signal Desktop history (no network).
  - `link` — register this app as a Signal secondary device via terminal QR.
  - `send`, `send-group` — send 1:1 and group v2 text messages.
  - `recv` — stream live incoming messages.
  - `whoami` — print the linked account's ACI / PNI / E.164.
  - `serve` — speak MCP (Model Context Protocol) over stdio for AI agents.
- **`signalkit-tauri`** desktop app (Tauri 2 + SolidJS):
  - Browse + per-chat search with flatpickr date range filter.
  - Compose box for 1:1 sends.
  - "Live" toggle that subprocesses `signalkit recv` and streams messages in
    real time via Tauri events.
- **MCP server (`signalkit serve`)** exposes seven tools:
  `list_chats`, `read_chat`, `search`, `find_recipient`, `whoami`, `send`,
  `send_group`.
- **GitHub Actions release workflow** that builds the CLI binary and the Tauri
  `.deb` / `.rpm` bundles on Linux/x86_64 on every `v*` tag push.

### Security

- presage's SQLite store is now SQLCipher-encrypted at rest. The passphrase is
  generated once (32 random bytes) and stored in libsecret under
  `application=signalkit, purpose=presage-store`. Existing plain stores are
  migrated transparently; the pre-migration file is kept as
  `presage.sqlite3.plain.bak` for one round-trip safety.

### Known limitations

- Linux only. macOS / Windows DB decryption paths not implemented.
- AppImage bundle disabled (linuxdeploy fails in the local build environment);
  `.deb` and `.rpm` are the supported install paths.
- No `list_groups` helper yet, so `send_group` requires you to provide the
  group's 32-byte master key manually.
- No attachment download.
- AGPL-3.0-only — inherited from [presage](https://github.com/whisperfish/presage).

[Unreleased]: https://github.com/rotkonetworks/signalkit/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/rotkonetworks/signalkit/releases/tag/v0.1.0
