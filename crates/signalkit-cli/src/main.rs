use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use signalkit_core::{default_signal_dir, live, DesktopBundle};
use uuid::Uuid;

mod serve;

#[derive(Parser)]
#[command(name = "signalkit", version, about = "Signal CLI: read history + send/receive live")]
struct Cli {
    /// Override the Signal Desktop data directory (for history commands).
    #[arg(long, global = true, env = "SIGNAL_DIR")]
    signal_dir: Option<PathBuf>,

    /// Override the presage store path (for live commands).
    #[arg(long, global = true, env = "SIGNALKIT_STORE")]
    store: Option<PathBuf>,

    /// Human-friendly output (default is JSON for piping).
    #[arg(long, global = true)]
    pretty: bool,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// List Signal chats sorted by recent activity (reads Signal Desktop DB).
    List {
        #[arg(long)]
        include_empty: bool,
    },
    /// Print messages from a chat, newest first (reads Signal Desktop DB).
    Read {
        chat: String,
        #[arg(long, default_value_t = 50)]
        limit: u32,
        #[arg(long, default_value_t = 0)]
        offset: u32,
    },
    /// Search messages by substring (reads Signal Desktop DB).
    Search {
        query: String,
        #[arg(long)]
        chat: Option<String>,
        #[arg(long, default_value_t = 100)]
        limit: u32,
    },
    /// Link this app to your phone as a Signal secondary device (presage).
    Link {
        #[arg(long, default_value = "signalkit")]
        device_name: String,
    },
    /// Send a 1:1 text message via presage. Recipient is an ACI UUID.
    Send {
        /// Recipient ACI UUID (e.g. 01989cad-fbdb-73ad-85e3-5ffa1df88a94).
        to: Uuid,
        /// Message body.
        body: String,
    },
    /// Send a text message to a Signal group v2.
    SendGroup {
        /// Group master key as base64 (32 bytes decoded).
        master_key_b64: String,
        /// Message body.
        body: String,
    },
    /// Run the receive loop, printing incoming messages until Ctrl-C.
    Recv,
    /// Print the linked account's own ACI / PNI / number.
    Whoami,
    /// Run as an MCP (Model Context Protocol) server over stdio. For AI agents.
    Serve,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let local = tokio::task::LocalSet::new();
    local.block_on(&rt, run(cli))
}

async fn run(cli: Cli) -> Result<()> {
    let Cli { signal_dir, store, pretty, cmd } = cli;
    match cmd {
        Cmd::List { include_empty } => {
            let bundle = open_desktop(signal_dir.clone()).await?;
            let chats = bundle.db.list_chats(include_empty)?;
            if pretty {
                for c in chats {
                    println!(
                        "{:>5}  {:<40}  {}",
                        c.total_messages,
                        truncate(c.display_name(), 40),
                        c.id
                    );
                }
            } else {
                emit_json(&chats)?;
            }
        }
        Cmd::Read { chat, limit, offset } => {
            let bundle = open_desktop(signal_dir.clone()).await?;
            let chat_row = bundle
                .db
                .find_chat_by_name(&chat)?
                .with_context(|| format!("no chat matched name {chat:?}"))?;
            let msgs = bundle
                .db
                .get_messages(&chat_row.id, Some(limit), offset, None, None)?;
            print_messages(&msgs, pretty, Some(chat_row.display_name()))?;
        }
        Cmd::Search { query, chat, limit } => {
            let bundle = open_desktop(signal_dir.clone()).await?;
            let cid = if let Some(name) = chat.as_deref() {
                Some(
                    bundle
                        .db
                        .find_chat_by_name(name)?
                        .with_context(|| format!("no chat matched name {name:?}"))?
                        .id,
                )
            } else {
                None
            };
            let msgs = bundle
                .db
                .search_messages(cid.as_deref(), &query, Some(limit), None, None)?;
            print_messages(&msgs, pretty, chat.as_deref())?;
        }
        Cmd::Link { device_name } => {
            let store_path = match store.clone() {
                Some(p) => p,
                None => live::default_store_path()?,
            };
            eprintln!("opening presage store at {}", store_path.display());
            let store = live::open_store(&store_path).await?;
            eprintln!("requesting provisioning URL — scan the QR with your phone (Signal → Settings → Linked devices → Link new device):\n");
            live::link(store, device_name, |url| {
                render_qr(url.as_str());
                eprintln!("\nor open this URL on your phone:\n  {url}\n");
            })
            .await?;
            eprintln!("linked!");
        }
        Cmd::Send { to, body } => {
            let store_path = match store.clone() {
                Some(p) => p,
                None => live::default_store_path()?,
            };
            let store = live::open_store(&store_path).await?;
            let mut manager = live::load_registered(store).await?;
            let ts = live::send_text(&mut manager, to, body).await?;
            if pretty {
                println!("sent (timestamp {ts})");
            } else {
                println!("{}", serde_json::json!({ "timestamp": ts }));
            }
        }
        Cmd::SendGroup { master_key_b64, body } => {
            let master_key = live::decode_master_key(&master_key_b64)?;
            let store_path = match store.clone() {
                Some(p) => p,
                None => live::default_store_path()?,
            };
            let store = live::open_store(&store_path).await?;
            let mut manager = live::load_registered(store).await?;
            let ts = live::send_text_to_group(&mut manager, &master_key, body).await?;
            if pretty {
                println!("sent to group (timestamp {ts})");
            } else {
                println!("{}", serde_json::json!({ "timestamp": ts }));
            }
        }
        Cmd::Serve => {
            let bundle = open_desktop(signal_dir.clone()).await?;
            serve::run(bundle).await?;
        }
        Cmd::Whoami => {
            let store_path = match store.clone() {
                Some(p) => p,
                None => live::default_store_path()?,
            };
            let store = live::open_store(&store_path).await?;
            let mut manager = live::load_registered(store).await?;
            let me = live::whoami(&mut manager).await?;
            if pretty {
                println!("aci:    {}", me.aci);
                if let Some(p) = me.pni { println!("pni:    {p}"); }
                if let Some(n) = me.number { println!("number: {n}"); }
            } else {
                emit_json(&me)?;
            }
        }
        Cmd::Recv => {
            let store_path = match store.clone() {
                Some(p) => p,
                None => live::default_store_path()?,
            };
            let store = live::open_store(&store_path).await?;
            let mut manager = live::load_registered(store).await?;
            eprintln!("receiving — Ctrl-C to stop");
            live::receive_into(&mut manager, |m| {
                if pretty {
                    println!("[{}] {}: {}", m.timestamp, m.from, m.body);
                } else {
                    let _ = serde_json::to_writer(std::io::stdout(), &m);
                    println!();
                }
            })
            .await?;
        }
    }
    Ok(())
}

async fn open_desktop(signal_dir: Option<PathBuf>) -> Result<DesktopBundle> {
    let dir = match signal_dir {
        Some(p) => p,
        None => default_signal_dir()?,
    };
    Ok(DesktopBundle::open(dir).await?)
}

fn print_messages(
    msgs: &[signalkit_core::MessageRow],
    pretty: bool,
    peer: Option<&str>,
) -> Result<()> {
    if pretty {
        for m in msgs {
            let outgoing = m
                .kind
                .as_deref()
                .map(|k| k.starts_with("outgoing"))
                .unwrap_or(false);
            let sender = if outgoing {
                "me".to_string()
            } else {
                peer.map(str::to_owned)
                    .or_else(|| m.source.clone())
                    .unwrap_or_else(|| m.conversation_id.clone())
            };
            let arrow = if outgoing { "→" } else { "←" };
            let attach = if m.has_attachments { " 📎" } else { "" };
            println!(
                "{}  {} {:<24}{}  {}",
                fmt_time(m.sent_at),
                arrow,
                truncate(&sender, 24),
                attach,
                m.body.as_deref().unwrap_or("")
            );
        }
    } else {
        emit_json(&msgs)?;
    }
    Ok(())
}

fn fmt_time(ms: Option<i64>) -> String {
    use time::format_description::well_known::Rfc3339;
    use time::OffsetDateTime;
    match ms {
        Some(t) => {
            let nanos = (t as i128) * 1_000_000;
            OffsetDateTime::from_unix_timestamp_nanos(nanos)
                .ok()
                .and_then(|dt| dt.format(&Rfc3339).ok())
                .unwrap_or_else(|| t.to_string())
        }
        None => "—".into(),
    }
}

fn emit_json<T: serde::Serialize>(v: &T) -> Result<()> {
    let s = serde_json::to_string(v)?;
    println!("{s}");
    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(max.saturating_sub(1)).collect();
        t.push('…');
        t
    }
}

fn render_qr(payload: &str) {
    use qrcode::render::unicode;
    use qrcode::QrCode;
    match QrCode::new(payload.as_bytes()) {
        Ok(code) => {
            let s = code
                .render::<unicode::Dense1x2>()
                .dark_color(unicode::Dense1x2::Light)
                .light_color(unicode::Dense1x2::Dark)
                .build();
            eprintln!("{s}");
        }
        Err(e) => eprintln!("(failed to render QR: {e})"),
    }
}
