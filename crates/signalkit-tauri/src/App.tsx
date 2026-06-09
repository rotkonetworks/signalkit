import { createEffect, createResource, createSignal, For, onCleanup, onMount, Show } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { listen, UnlistenFn } from "@tauri-apps/api/event";
import flatpickr from "flatpickr";
import "flatpickr/dist/flatpickr.min.css";
import { api, Chat, displayName, MessageRow } from "./api";

function startOfDayMs(d: Date | null): number | null {
  if (!d) return null;
  const s = new Date(d);
  s.setHours(0, 0, 0, 0);
  return s.getTime();
}
function endOfDayMs(d: Date | null): number | null {
  if (!d) return null;
  const e = new Date(d);
  e.setHours(23, 59, 59, 999);
  return e.getTime();
}

export function App() {
  const [opened, setOpened] = createSignal(false);
  const [openErr, setOpenErr] = createSignal<string | null>(null);
  const [openBusy, setOpenBusy] = createSignal(false);
  const [filter, setFilter] = createSignal("");
  const [selected, setSelected] = createSignal<Chat | null>(null);
  const [query, setQuery] = createSignal("");
  const [fromMs, setFromMs] = createSignal<number | null>(null);
  const [toMs, setToMs] = createSignal<number | null>(null);
  const [composeText, setComposeText] = createSignal("");
  const [sending, setSending] = createSignal(false);
  const [sendError, setSendError] = createSignal<string | null>(null);
  const [live, setLive] = createSignal(false);
  const [liveError, setLiveError] = createSignal<string | null>(null);

  async function doOpen() {
    setOpenBusy(true);
    setOpenErr(null);
    try {
      await api.openBundle();
      setOpened(true);
    } catch (e) {
      setOpenErr(String(e));
      setOpened(false);
    } finally {
      setOpenBusy(false);
    }
  }

  onMount(() => {
    void doOpen();
  });

  const [chats, { refetch: refetchChats }] = createResource(opened, async (isOpen) => {
    if (!isOpen) return [] as Chat[];
    return api.listChats();
  });

  // Auto-select the most recent chat once chats load (and nothing is selected yet).
  createEffect(() => {
    const list = chats();
    if (list && list.length > 0 && !selected()) {
      setSelected(list[0]);
    }
  });

  // Messages come from SQL newest-first; flip for chat-app order (newest at bottom).
  const orderedMessages = () => {
    const ms = messages();
    return ms ? [...ms].reverse() : [];
  };

  // Scroll to bottom whenever the message list changes (chat switch or new data).
  let msgsEl: HTMLDivElement | undefined;
  createEffect(() => {
    messages();
    selected();
    if (msgsEl) {
      queueMicrotask(() => {
        if (msgsEl) msgsEl.scrollTop = msgsEl.scrollHeight;
      });
    }
  });

  const filteredChats = () => {
    const f = filter().toLowerCase();
    const list = chats() ?? [];
    if (!f) return list;
    return list.filter((c) => displayName(c).toLowerCase().includes(f));
  };

  const [messages, { refetch: refetchMessages }] = createResource(
    () => ({ chat: selected(), q: query(), from: fromMs(), to: toMs() }),
    async ({ chat, q, from, to }): Promise<MessageRow[]> => {
      if (!chat) return [];
      const range = { fromMs: from, toMs: to };
      if (q.trim().length > 0) {
        return api.search(q, chat.id, 200, range);
      }
      return api.readChat(chat.id, 80, 0, range);
    }
  );

  async function toggleLive() {
    setLiveError(null);
    if (live()) {
      try {
        await invoke("signal_recv_stop");
      } catch (e) {
        setLiveError(String(e));
      }
      setLive(false);
    } else {
      try {
        await invoke("signal_recv_start");
        setLive(true);
      } catch (e) {
        setLiveError(String(e));
      }
    }
  }

  onMount(async () => {
    const unlisteners: UnlistenFn[] = [];
    unlisteners.push(
      await listen("signal-message", () => {
        // New live message arrived — refresh the current chat and chat counts.
        refetchMessages();
        refetchChats();
      })
    );
    unlisteners.push(
      await listen<string>("signal-recv-stopped", () => {
        setLive(false);
      })
    );
    unlisteners.push(
      await listen<string>("signal-recv-log", (e) => {
        // Surface non-JSON output (errors etc) once.
        const t = String(e.payload);
        if (t.toLowerCase().includes("error") || t.toLowerCase().includes("not linked")) {
          setLiveError(t);
        }
      })
    );
    onCleanup(() => unlisteners.forEach((u) => u()));
  });

  async function doSend() {
    const chat = selected();
    const text = composeText().trim();
    if (!chat || !chat.service_id || !text) return;
    setSending(true);
    setSendError(null);
    try {
      await api.signalSend(chat.service_id, text);
      setComposeText("");
      // Give Signal Desktop a beat to sync, then refresh.
      setTimeout(() => refetchMessages(), 800);
    } catch (e) {
      setSendError(String(e));
    } finally {
      setSending(false);
    }
  }

  function attachDatePicker(el: HTMLInputElement, onPick: (d: Date | null) => void) {
    flatpickr(el, {
      dateFormat: "Y-m-d",
      allowInput: true,
      onChange: (selected) => onPick(selected[0] ?? null),
    });
  }

  function fmtTime(ms: number | null): string {
    if (!ms) return "—";
    try {
      return new Date(ms).toISOString().replace("T", " ").slice(0, 19);
    } catch {
      return String(ms);
    }
  }

  return (
    <div class="app">
      <aside class="sidebar">
        <header>
          <div class="sidebar-row">
            <input
              placeholder="Filter chats…"
              value={filter()}
              onInput={(e) => setFilter(e.currentTarget.value)}
              disabled={!opened()}
            />
            <button
              class={"live-toggle " + (live() ? "on" : "")}
              onClick={() => void toggleLive()}
              title={
                liveError() ??
                (live()
                  ? "Live: subscribed to signalkit recv — messages stream in real time"
                  : "Live: off (click to start; requires signalkit link)")
              }
            >
              {live() ? "● Live" : "○ Live"}
            </button>
          </div>
        </header>

        <Show when={openErr()}>
          <div class="status error">
            <div><strong>Failed to open Signal Desktop DB</strong></div>
            <div>{openErr()}</div>
            <div class="hint">Make sure Signal Desktop has been launched at least once on this machine, and that <code>~/.config/Signal</code> (or the Flatpak path) exists.</div>
            <button onClick={() => void doOpen()} disabled={openBusy()}>Retry</button>
          </div>
        </Show>

        <Show when={!opened() && !openErr() && openBusy()}>
          <div class="status">Unlocking Signal Desktop DB…</div>
        </Show>

        <Show when={opened()}>
          <Show when={chats.loading}>
            <div class="status">Loading chats…</div>
          </Show>
          <Show when={!chats.loading && (chats() ?? []).length === 0}>
            <div class="status">
              No chats found.
              <button onClick={() => refetchChats()}>Reload</button>
            </div>
          </Show>
          <div class="chat-list">
            <For each={filteredChats()}>
              {(c) => (
                <div
                  class={"chat" + (selected()?.id === c.id ? " selected" : "")}
                  onClick={() => {
                    setSelected(c);
                    setQuery("");
                  }}
                >
                  <span class="name">{displayName(c)}</span>
                  <span class="count">{c.total_messages}</span>
                </div>
              )}
            </For>
          </div>
        </Show>
      </aside>

      <section class="pane">
        <header>
          <h2>{selected() ? displayName(selected()!) : "signalkit"}</h2>
          <Show when={selected()}>
            <input
              class="search"
              placeholder="Search in this chat…"
              value={query()}
              onInput={(e) => setQuery(e.currentTarget.value)}
            />
            <input
              class="date"
              placeholder="from"
              ref={(el) => attachDatePicker(el, (d) => setFromMs(startOfDayMs(d)))}
            />
            <input
              class="date"
              placeholder="to"
              ref={(el) => attachDatePicker(el, (d) => setToMs(endOfDayMs(d)))}
            />
            <Show when={fromMs() !== null || toMs() !== null}>
              <button
                class="clear"
                title="Clear date range"
                onClick={() => {
                  setFromMs(null);
                  setToMs(null);
                  document.querySelectorAll<HTMLInputElement>(".pane input.date").forEach((el) => {
                    const fp = (el as any)._flatpickr;
                    if (fp) fp.clear();
                  });
                }}
              >
                ×
              </button>
            </Show>
          </Show>
        </header>
        <Show
          when={selected()}
          fallback={
            <div class="empty">
              <Show when={opened()} fallback={<p>Waiting for DB…</p>}>
                <p>Pick a conversation on the left.</p>
                <p class="hint">
                  {chats()?.length ?? 0} chats loaded.
                </p>
              </Show>
            </div>
          }
        >
          <div class="messages" ref={(el) => (msgsEl = el)}>
            <Show when={messages.loading}>
              <div class="status">Loading…</div>
            </Show>
            <For each={orderedMessages()}>
              {(m) => {
                const outgoing = m.kind?.startsWith("outgoing");
                const sender = outgoing
                  ? "me"
                  : displayName(selected()!);
                return (
                  <div class={"msg " + (outgoing ? "out" : "in")}>
                    <div class="sender">{sender}</div>
                    <div class="body">{m.body ?? ""}</div>
                    <div class="meta">
                      {fmtTime(m.sent_at)}
                      {m.has_attachments ? " · 📎" : ""}
                    </div>
                  </div>
                );
              }}
            </For>
            <Show when={!messages.loading && (messages() ?? []).length === 0}>
              <div class="status">No messages.</div>
            </Show>
          </div>
          <Show when={selected()}>
            <div class="compose">
              <Show when={sendError()}>
                <div class="send-err">send failed: {sendError()}</div>
              </Show>
              <div class="compose-row">
                <input
                  placeholder={
                    selected()?.service_id
                      ? "Type a message — Enter to send"
                      : "(group chats not supported yet)"
                  }
                  value={composeText()}
                  onInput={(e) => setComposeText(e.currentTarget.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" && !e.shiftKey) {
                      e.preventDefault();
                      void doSend();
                    }
                  }}
                  disabled={!selected()?.service_id || sending()}
                />
                <button
                  onClick={() => void doSend()}
                  disabled={!selected()?.service_id || !composeText().trim() || sending()}
                >
                  {sending() ? "…" : "Send"}
                </button>
              </div>
            </div>
          </Show>
        </Show>
      </section>
    </div>
  );
}
