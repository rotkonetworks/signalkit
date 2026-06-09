import { invoke } from "@tauri-apps/api/core";

export interface Chat {
  id: string;
  name: string | null;
  profile_name: string | null;
  profile_full_name: string | null;
  e164: string | null;
  service_id: string | null;
  group_id: string | null;
  kind: string | null;
  active_at: number | null;
  total_messages: number;
}

export interface MessageRow {
  id: string;
  conversation_id: string;
  sent_at: number | null;
  received_at: number | null;
  body: string | null;
  source: string | null;
  kind: string | null;
  has_attachments: boolean;
}

export interface DateRange {
  fromMs?: number | null;
  toMs?: number | null;
}

export const api = {
  openBundle: () => invoke<void>("open_bundle"),
  listChats: () => invoke<Chat[]>("list_chats"),
  readChat: (chatId: string, limit = 100, offset = 0, range: DateRange = {}) =>
    invoke<MessageRow[]>("read_chat", {
      chatId,
      limit,
      offset,
      fromMs: range.fromMs ?? null,
      toMs: range.toMs ?? null,
    }),
  search: (query: string, chatId: string | null, limit = 100, range: DateRange = {}) =>
    invoke<MessageRow[]>("search", {
      query,
      chatId,
      limit,
      fromMs: range.fromMs ?? null,
      toMs: range.toMs ?? null,
    }),
  signalSend: (to: string, body: string) =>
    invoke<number>("signal_send", { to, body }),
  signalWhoami: () =>
    invoke<{ aci: string; pni: string | null; number: string | null }>("signal_whoami"),
};

export function displayName(c: Chat): string {
  const name = c.name || c.profile_full_name || c.profile_name || c.e164;
  if (name) return name;
  if (c.service_id) return c.service_id.slice(0, 8) + "…";
  if (c.group_id) return "Group " + c.group_id.slice(0, 6);
  return c.id.slice(0, 8) + "…";
}
