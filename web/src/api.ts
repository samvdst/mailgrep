// Shared types, fetch helper, formatters.

export interface Addr {
  email: string | null;
  name: string | null;
}

export interface FacetData {
  senders: { email: string; name: string | null; count: number }[];
  orgs: { org: string; count: number }[];
  years: { year: number; count: number }[];
  exts: { ext: string; count: number }[];
}

export interface SearchRow {
  id: number;
  account_id: number;
  account: string;
  subject: string;
  from: Addr[];
  date: number;
  date_offset_mins: number;
  date_source: string;
  skew: boolean;
  has_attach: boolean;
  thread_id: string;
  thread_size: number;
  folders: string[];
  snippet: string;
  score: number;
}

export interface SearchResponse {
  results: SearchRow[];
  total: number;
  cross_account?: boolean;
  facets: FacetData;
  accounts: { id: number; name: string }[];
}

export interface SyncProgress {
  running: boolean;
  folder: string;
  discovered: number;
  processed: number;
  new_msgs: number;
  removed: number;
  failed: number;
  error: string | null;
}

export interface StatusAccount {
  id: number;
  name: string;
  kind: string;
  host: string;
  username: string;
  message_count: number;
  index_size_bytes: number;
  last_sync_at: number | null;
  last_sync_status: string | null;
  sync_interval_mins: number;
  excluded_folders: string[];
  progress: SyncProgress | null;
}

export interface Attachment {
  path: string;
  filename: string | null;
  mime: string;
  size: number;
  content_id: string | null;
  inline: boolean;
}

export interface Detail {
  id: number;
  account_id: number;
  msgid: string | null;
  subject: string;
  subject_norm: string;
  from: Addr[];
  to: Addr[];
  cc: Addr[];
  dates: {
    canonical: number;
    source: string;
    offset_mins: number;
    received_top: number | null;
    date_header: number | null;
    internaldate: number | null;
    skew: boolean;
  };
  thread_id: string;
  thread_size: number;
  body_text: string;
  fresh_text: string;
  has_html: boolean;
  attachments: Attachment[];
  folders: { folder: string; uid: number }[];
  images_allowed: boolean;
}

export interface ThreadMsg {
  id: number;
  subject: string;
  from: Addr[];
  date: number;
  skew: boolean;
  snippet: string;
}

export interface Contact {
  email: string;
  display_name: string | null;
  org: string | null;
  is_role: boolean;
  msg_count: number;
}

export interface MergeOp {
  op: string;
  a: string;
  b: string;
  at: number;
}

/** Fetch JSON; on failure throw an Error carrying the server's message. */
export async function api<T = unknown>(path: string, init?: RequestInit): Promise<T> {
  const r = await fetch(path, init);
  if (!r.ok) {
    let msg = `${r.status} ${r.statusText}`;
    try {
      const j = await r.json();
      if (j && typeof j.error === "string") msg = j.error;
    } catch {
      /* not json */
    }
    const e = new Error(msg) as Error & { status?: number };
    e.status = r.status;
    throw e;
  }
  return r.json() as Promise<T>;
}

export function post(path: string, body?: unknown): Promise<unknown> {
  return api(path, {
    method: "POST",
    ...(body !== undefined
      ? { headers: { "content-type": "application/json" }, body: JSON.stringify(body) }
      : {}),
  });
}

export function esc(s: string | null | undefined): string {
  if (!s) return "";
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}

/** Format a UTC unix timestamp in the message's original offset: "2021-03-10 09:14". */
export function fmtDate(secs: number | null | undefined, offsetMins = 0): string {
  if (secs == null) return "—";
  const d = new Date((secs + offsetMins * 60) * 1000);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getUTCFullYear()}-${p(d.getUTCMonth() + 1)}-${p(d.getUTCDate())} ${p(d.getUTCHours())}:${p(d.getUTCMinutes())}`;
}

/** Local-time format for machine events (sync times). */
export function fmtLocal(secs: number | null | undefined): string {
  if (!secs) return "never";
  const d = new Date(secs * 1000);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
}

export function humanSize(n: number): string {
  if (n < 1024) return `${n} B`;
  let v = n;
  for (const u of ["KB", "MB", "GB", "TB"]) {
    v /= 1024;
    if (v < 1024) return `${v.toFixed(v < 10 ? 1 : 0)} ${u}`;
  }
  return `${v.toFixed(0)} PB`;
}

export function addrLabel(a: Addr | undefined): string {
  if (!a) return "(unknown)";
  return a.name || a.email || "(unknown)";
}
