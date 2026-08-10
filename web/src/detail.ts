// Detail pane: one message, dates provenance, attachments, thread, body tabs.

import {
  Addr, Detail, ThreadMsg, api, post, esc, fmtDate, humanSize, addrLabel,
} from "./api";
import { appendQuery, setQuery } from "./app";

const pane = document.getElementById("detail") as HTMLElement;
const layout = document.getElementById("layout") as HTMLElement;

let current: Detail | null = null;
let tab: "html" | "text" | "raw" = "html";

export function isDetailOpen(): boolean {
  return !pane.hidden;
}

export function closeDetail(): void {
  pane.hidden = true;
  layout.classList.remove("withdetail");
  current = null;
}

export async function openDetail(id: number): Promise<void> {
  let d: Detail;
  try {
    d = await api<Detail>(`/api/message/${id}`);
  } catch (e) {
    pane.hidden = false;
    layout.classList.add("withdetail");
    pane.innerHTML = `<div class="err">${esc((e as Error).message)}</div>`;
    return;
  }
  current = d;
  tab = d.has_html ? "html" : "text";
  render(d);
  pane.hidden = false;
  layout.classList.add("withdetail");
  pane.scrollTop = 0;
  if (d.thread_size > 1) loadThread(d);
}

function addrChips(list: Addr[]): string {
  if (!list.length) return '<span class="dim">-</span>';
  return list
    .map((a) => {
      const label = esc(addrLabel(a));
      if (!a.email) return `<span class="addr noclick">${label}</span>`;
      return `<button class="addr" data-email="${esc(a.email)}" title="${esc(a.email)} (add contact: filter)">${label}</button>`;
    })
    .join(" ");
}

function sourceBadge(src: string): string {
  return `<span class="badge src" title="date source that won">${esc(src)}</span>`;
}

function render(d: Detail): void {
  const dt = d.dates;
  const skewBadge = dt.skew
    ? '<span class="badge danger" title="stored date differs from derived date">&#9888; skew</span>'
    : "";
  const atts = d.attachments
    .map(
      (a) => `<li>
        <a href="/api/message/${d.id}/part/${a.path}" target="_blank" rel="noopener">${esc(a.filename || `part ${a.path}`)}</a>
        <span class="dim">${esc(a.mime)} · ${humanSize(a.size)}</span>
      </li>`,
    )
    .join("");

  pane.innerHTML = `
    <div class="d-head">
      <button id="d-close" title="close (Esc)" aria-label="close">&times;</button>
      <h2 class="d-subject">${esc(d.subject) || "(no subject)"}</h2>
    </div>
    <div class="d-addrs">
      <div><span class="lbl">from</span> ${addrChips(d.from)}</div>
      <div><span class="lbl">to</span> ${addrChips(d.to)}</div>
      ${d.cc.length ? `<div><span class="lbl">cc</span> ${addrChips(d.cc)}</div>` : ""}
    </div>
    <div class="d-dates">
      <span class="mono">${fmtDate(dt.canonical, dt.offset_mins)}</span>
      ${sourceBadge(dt.source)} ${skewBadge}
      <button id="d-dates-toggle" class="linkish">candidates</button>
      <div id="d-candidates" hidden>
        <div><span class="lbl">received</span> <span class="mono">${fmtDate(dt.received_top, dt.offset_mins)}</span></div>
        <div><span class="lbl">date header</span> <span class="mono">${fmtDate(dt.date_header, dt.offset_mins)}</span></div>
        <div><span class="lbl">internaldate</span> <span class="mono">${fmtDate(dt.internaldate, dt.offset_mins)}</span></div>
      </div>
    </div>
    <div class="d-folders">
      ${d.folders.map((f) => `<span class="chip" title="uid ${f.uid}">${esc(f.folder)} · ${f.uid}</span>`).join(" ")}
    </div>
    ${atts ? `<ul class="d-atts">${atts}</ul>` : ""}
    ${d.thread_size > 1 ? `<div class="d-thread"><div class="t-head">thread · ${d.thread_size} messages <button id="d-thread-search" class="linkish">search this thread</button></div><div id="d-thread-list" class="dim">loading…</div></div>` : ""}
    <div class="d-tabs" role="tablist">
      <button data-tab="html">HTML</button>
      <button data-tab="text">Text</button>
      <button data-tab="raw">Raw</button>
    </div>
    <div id="d-imgbanner" hidden>
      Remote images blocked
      <button id="d-img-once">Load once</button>
      <button id="d-img-always">Always allow from this sender</button>
    </div>
    <div id="d-body"></div>
  `;

  (pane.querySelector("#d-close") as HTMLElement).onclick = closeDetail;
  const toggle = pane.querySelector("#d-dates-toggle") as HTMLElement;
  toggle.onclick = () => {
    const c = pane.querySelector("#d-candidates") as HTMLElement;
    c.hidden = !c.hidden;
  };
  pane.querySelectorAll<HTMLButtonElement>(".addr[data-email]").forEach((b) => {
    b.onclick = () => appendQuery(`contact:${b.dataset.email}`);
  });
  const ts = pane.querySelector("#d-thread-search") as HTMLButtonElement | null;
  if (ts) ts.onclick = () => setQuery(`thread:${d.thread_id}`);
  pane.querySelectorAll<HTMLButtonElement>(".d-tabs button").forEach((b) => {
    b.onclick = () => {
      tab = b.dataset.tab as typeof tab;
      renderBody(d);
    };
  });
  (pane.querySelector("#d-img-once") as HTMLButtonElement).onclick = () => {
    const f = pane.querySelector("iframe");
    if (f) f.src = `/api/message/${d.id}/html?images=1`;
    (pane.querySelector("#d-imgbanner") as HTMLElement).hidden = true;
  };
  (pane.querySelector("#d-img-always") as HTMLButtonElement).onclick = async () => {
    try {
      await post(`/api/message/${d.id}/allow_images`);
      d.images_allowed = true;
      const f = pane.querySelector("iframe");
      if (f) f.src = `/api/message/${d.id}/html`;
      (pane.querySelector("#d-imgbanner") as HTMLElement).hidden = true;
    } catch (e) {
      alert((e as Error).message);
    }
  };

  renderBody(d);
}

function renderBody(d: Detail): void {
  pane.querySelectorAll<HTMLButtonElement>(".d-tabs button").forEach((b) => {
    b.classList.toggle("active", b.dataset.tab === tab);
  });
  const body = pane.querySelector("#d-body") as HTMLElement;
  const banner = pane.querySelector("#d-imgbanner") as HTMLElement;
  banner.hidden = !(tab === "html" && d.has_html && !d.images_allowed);

  if (tab === "html") {
    body.innerHTML = `<iframe sandbox="allow-same-origin" src="/api/message/${d.id}/html" title="message body"></iframe>`;
  } else if (tab === "text") {
    body.innerHTML = `<pre class="d-text"></pre>`;
    (body.firstElementChild as HTMLElement).textContent = d.body_text || "(no text body)";
  } else {
    body.innerHTML = `<pre class="d-text dim">loading raw…</pre>`;
    fetch(`/api/message/${d.id}/raw`)
      .then((r) => (r.ok ? r.text() : Promise.reject(new Error(`${r.status}`))))
      .then((t) => {
        if (current?.id !== d.id || tab !== "raw") return;
        body.innerHTML = `<pre class="d-text"></pre>`;
        (body.firstElementChild as HTMLElement).textContent = t;
      })
      .catch((e) => {
        body.innerHTML = `<pre class="d-text err"></pre>`;
        (body.firstElementChild as HTMLElement).textContent = `failed to load raw: ${e.message}`;
      });
  }
}

async function loadThread(d: Detail): Promise<void> {
  const list = pane.querySelector("#d-thread-list") as HTMLElement | null;
  if (!list) return;
  try {
    const t = await api<{ messages: ThreadMsg[] }>(
      `/api/thread/${d.account_id}/${encodeURIComponent(d.thread_id)}`,
    );
    if (current?.id !== d.id) return;
    list.classList.remove("dim");
    list.innerHTML = t.messages
      .map(
        (m) => `<button class="t-msg${m.id === d.id ? " current" : ""}" data-id="${m.id}">
          <span class="t-from">${esc(addrLabel(m.from[0]))}</span>
          <span class="t-date mono">${fmtDate(m.date, d.dates.offset_mins)}</span>
          <span class="t-snip">${esc(m.snippet)}</span>
        </button>`,
      )
      .join("");
    list.querySelectorAll<HTMLButtonElement>(".t-msg").forEach((b) => {
      b.onclick = () => {
        const id = Number(b.dataset.id);
        if (id !== current?.id) void openDetail(id);
      };
    });
  } catch (e) {
    list.textContent = `thread failed to load: ${(e as Error).message}`;
  }
}
