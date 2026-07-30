// Settings slide-over: accounts, sync controls, add account, contacts browser.

import {
  Contact, MergeOp, StatusAccount, api, post, esc, fmtLocal, humanSize,
} from "./api";
import { onAccountsChanged, setQuery } from "./app";

const panel = document.getElementById("settings") as HTMLElement;
const acctList = document.getElementById("acctlist") as HTMLElement;
const sErr = document.getElementById("s-err") as HTMLElement;
const addErr = document.getElementById("adderr") as HTMLElement;
const cAcct = document.getElementById("c-acct") as HTMLSelectElement;
const cQ = document.getElementById("c-q") as HTMLInputElement;
const cMerge = document.getElementById("c-merge") as HTMLButtonElement;
const contactList = document.getElementById("contactlist") as HTMLElement;
const mergeList = document.getElementById("mergelist") as HTMLElement;

export let accounts: StatusAccount[] = [];
let pollTimer: number | undefined;
const selectedContacts = new Set<string>();

export function isSettingsOpen(): boolean {
  return !panel.hidden;
}

export function toggleSettings(): void {
  panel.hidden ? openSettings() : closeSettings();
}

export function openSettings(): void {
  panel.hidden = false;
  void refreshStatus();
  void loadContacts();
}

export function closeSettings(): void {
  panel.hidden = true;
}

function showErr(el: HTMLElement, msg: string): void {
  el.textContent = msg;
  el.hidden = false;
}

/** Fetch /api/status, re-render, keep polling every 2s while a sync runs. */
export async function refreshStatus(): Promise<StatusAccount[]> {
  const s = await api<{ accounts: StatusAccount[] }>("/api/status");
  accounts = s.accounts;
  onAccountsChanged(accounts);
  renderAccountOptions();
  if (isSettingsOpen()) renderAccounts();
  clearTimeout(pollTimer);
  if (accounts.some((a) => a.progress?.running)) {
    pollTimer = window.setTimeout(() => void refreshStatus(), 2000);
  }
  return accounts;
}

function renderAccountOptions(): void {
  const prev = cAcct.value;
  cAcct.innerHTML = accounts
    .map((a) => `<option value="${a.id}">${esc(a.name)}</option>`)
    .join("");
  if (prev && accounts.some((a) => String(a.id) === prev)) cAcct.value = prev;
}

function renderAccounts(): void {
  if (!accounts.length) {
    acctList.innerHTML = '<div class="dim">No accounts yet — add one below.</div>';
    return;
  }
  acctList.innerHTML = accounts
    .map((a) => {
      const p = a.progress;
      const prog = p?.running
        ? `<div class="acct-prog">syncing <b>${esc(p.folder) || "…"}</b> — ${p.processed}/${p.discovered}${p.failed ? ` · ${p.failed} failed` : ""}</div>`
        : p?.error
          ? `<div class="err">${esc(p.error)}</div>`
          : "";
      return `<div class="acct" data-id="${a.id}">
        <div class="acct-head"><b>${esc(a.name)}</b> <button data-act="rename" class="linkish" title="rename account">✎</button> <span class="dim">${esc(a.kind)} · ${esc(a.host)}</span></div>
        <div class="dim">${a.message_count.toLocaleString()} messages · ${humanSize(a.index_size_bytes)} index</div>
        <div class="dim">last sync ${fmtLocal(a.last_sync_at)}${a.last_sync_status ? ` · ${esc(a.last_sync_status)}` : ""}</div>
        ${prog}
        <div class="acct-actions">
          <button data-act="sync">Sync now</button>
          <button data-act="bounded">Bounded sync…</button>
          <button data-act="rebuild">Rebuild derived data</button>
          <button data-act="folders">Folders…</button>
          <button data-act="images">Images…</button>
          <button data-act="delete" class="danger">Delete</button>
        </div>
        <div class="acct-interval">
          <label>sync every <input type="number" min="0" class="interval" value="${a.sync_interval_mins}"> min</label>
          <button data-act="interval">Set</button>
        </div>
        <div class="acct-folders" hidden></div>
        <div class="acct-images" hidden></div>
      </div>`;
    })
    .join("");
}

acctList.addEventListener("click", (e) => {
  const btn = (e.target as HTMLElement).closest<HTMLButtonElement>("button[data-act]");
  if (!btn) return;
  const card = btn.closest<HTMLElement>(".acct")!;
  const id = Number(card.dataset.id);
  void accountAction(btn.dataset.act!, id, card, btn);
});

async function accountAction(act: string, id: number, card: HTMLElement, btn?: HTMLButtonElement): Promise<void> {
  sErr.hidden = true;
  try {
    switch (act) {
      case "sync":
        await post(`/api/accounts/${id}/sync`);
        break;
      case "bounded": {
        const n = prompt("Max messages per folder?");
        if (!n) return;
        const max = parseInt(n, 10);
        if (!Number.isFinite(max) || max <= 0) return showErr(sErr, "enter a positive number");
        await post(`/api/accounts/${id}/sync`, { max_per_folder: max });
        break;
      }
      case "rebuild":
        await post(`/api/accounts/${id}/rebuild`);
        break;
      case "delete": {
        const a = accounts.find((x) => x.id === id);
        if (!confirm(`Delete account "${a?.name}" and its index?`)) return;
        await api(`/api/accounts/${id}`, { method: "DELETE" });
        break;
      }
      case "interval": {
        const input = card.querySelector<HTMLInputElement>(".interval")!;
        await post(`/api/accounts/${id}/interval`, { minutes: parseInt(input.value, 10) || 0 });
        break;
      }
      case "rename": {
        const a = accounts.find((x) => x.id === id);
        const name = prompt("New account name:", a?.name ?? "");
        if (!name || !name.trim()) return;
        await post(`/api/accounts/${id}/rename`, { name: name.trim() });
        break;
      }
      case "folders":
        return toggleFolders(id, card);
      case "images":
        return toggleImages(id, card);
      case "revokeimg": {
        const sender = btn?.dataset.sender;
        if (!sender) return;
        await api(`/api/accounts/${id}/image_allowances`, {
          method: "DELETE",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ sender }),
        });
        return toggleImages(id, card, true);
      }
      case "savefolders": {
        const excluded = [...card.querySelectorAll<HTMLInputElement>(".acct-folders input:checked")]
          .map((c) => c.value);
        await api(`/api/accounts/${id}/folders`, {
          method: "PUT",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ excluded }),
        });
        card.querySelector<HTMLElement>(".acct-folders")!.hidden = true;
        break;
      }
    }
  } catch (e) {
    return showErr(sErr, (e as Error).message);
  }
  await refreshStatus();
}

/// Per-sender remote-image allowances: list + revoke.
async function toggleImages(id: number, card: HTMLElement, forceOpen = false): Promise<void> {
  const box = card.querySelector<HTMLElement>(".acct-images")!;
  if (!box.hidden && !forceOpen) {
    box.hidden = true;
    return;
  }
  try {
    const d = await api<{ allowances: { sender: string; at: number }[] }>(
      `/api/accounts/${id}/image_allowances`,
    );
    box.innerHTML = d.allowances.length
      ? `<div class="dim">Remote images always allowed from:</div>` +
        d.allowances
          .map(
            (a) =>
              `<div class="imgallow"><span>${esc(a.sender)}</span> <span class="dim">${fmtLocal(a.at)}</span> <button data-act="revokeimg" data-sender="${esc(a.sender)}" class="danger" title="block again">×</button></div>`,
          )
          .join("")
      : '<div class="dim">No senders allowed — remote images are blocked everywhere.</div>';
    box.hidden = false;
  } catch (e) {
    showErr(sErr, (e as Error).message);
  }
}

async function toggleFolders(id: number, card: HTMLElement): Promise<void> {
  const box = card.querySelector<HTMLElement>(".acct-folders")!;
  if (!box.hidden) {
    box.hidden = true;
    return;
  }
  box.hidden = false;
  box.innerHTML = '<span class="dim">loading folders…</span>';
  try {
    const r = await api<{ folders: { name: string; excluded: boolean }[] }>(
      `/api/accounts/${id}/folders`,
    );
    box.innerHTML =
      "<div class='dim'>checked = excluded from sync</div>" +
      r.folders
        .map(
          (f) => `<label class="folder-row"><input type="checkbox" value="${esc(f.name)}"${f.excluded ? " checked" : ""}> ${esc(f.name)}</label>`,
        )
        .join("") +
      `<button data-act="savefolders">Save exclusions</button>`;
  } catch (e) {
    box.innerHTML = `<span class="err">${esc((e as Error).message)}</span>`;
  }
}

// ---------- add account ----------

(document.getElementById("addimap") as HTMLFormElement).addEventListener("submit", (e) => {
  e.preventDefault();
  const f = e.target as HTMLFormElement;
  const fd = new FormData(f);
  void addAccount(f, {
    name: String(fd.get("name")),
    host: String(fd.get("host")),
    port: parseInt(String(fd.get("port")), 10) || 993,
    username: String(fd.get("username")),
    password: String(fd.get("password")),
  });
});

(document.getElementById("addfixture") as HTMLFormElement).addEventListener("submit", (e) => {
  e.preventDefault();
  const f = e.target as HTMLFormElement;
  const fd = new FormData(f);
  void addAccount(f, {
    name: String(fd.get("name")),
    fixture_dir: String(fd.get("fixture_dir")),
  });
});

async function addAccount(form: HTMLFormElement, body: unknown): Promise<void> {
  addErr.hidden = true;
  const btn = form.querySelector("button")!;
  btn.disabled = true;
  btn.textContent = "verifying…";
  try {
    await post("/api/accounts", body);
    form.reset();
    await refreshStatus();
    void loadContacts();
  } catch (e) {
    showErr(addErr, (e as Error).message);
  } finally {
    btn.disabled = false;
    btn.textContent = form.id === "addimap" ? "Add IMAP account" : "Add fixture account";
  }
}

// ---------- contacts browser ----------

let cTimer: number | undefined;
cQ.addEventListener("input", () => {
  clearTimeout(cTimer);
  cTimer = window.setTimeout(() => void loadContacts(), 250);
});
cAcct.addEventListener("change", () => void loadContacts());

async function loadContacts(): Promise<void> {
  if (!cAcct.value) {
    contactList.innerHTML = '<div class="dim">add an account first</div>';
    mergeList.textContent = "none";
    return;
  }
  const acct = cAcct.value;
  selectedContacts.clear();
  updateMergeBtn();
  try {
    const [c, m] = await Promise.all([
      api<{ contacts: Contact[] }>(
        `/api/contacts?account=${acct}&q=${encodeURIComponent(cQ.value)}`,
      ),
      api<{ ops: MergeOp[] }>(`/api/merges?account=${acct}`),
    ]);
    renderContacts(c.contacts);
    renderMerges(m.ops);
  } catch (e) {
    contactList.innerHTML = `<div class="err">${esc((e as Error).message)}</div>`;
  }
}

function renderContacts(list: Contact[]): void {
  if (!list.length) {
    contactList.innerHTML = '<div class="dim">no contacts</div>';
    return;
  }
  contactList.innerHTML = list
    .map(
      (c) => `<div class="contact" data-email="${esc(c.email)}">
        <input type="checkbox" title="select for merge">
        <span class="c-name">${esc(c.display_name || c.email)}${c.is_role ? ' <span title="role address">&#129302;</span>' : ""}</span>
        <span class="dim c-email">${esc(c.email)}</span>
        ${c.org ? `<span class="chip">${esc(c.org)}</span>` : ""}
        <span class="cnt">${c.msg_count}</span>
      </div>`,
    )
    .join("");
  contactList.querySelectorAll<HTMLElement>(".contact").forEach((row) => {
    const email = row.dataset.email!;
    const cb = row.querySelector("input")!;
    cb.onclick = (e) => {
      e.stopPropagation();
      cb.checked ? selectedContacts.add(email) : selectedContacts.delete(email);
      updateMergeBtn();
    };
    row.onclick = () => {
      setQuery(`contact:${email}`);
      closeSettings();
    };
  });
}

function updateMergeBtn(): void {
  cMerge.hidden = selectedContacts.size !== 2;
}

cMerge.addEventListener("click", async () => {
  const [a, b] = [...selectedContacts];
  try {
    await post("/api/merge", { account: Number(cAcct.value), a, b });
    await loadContacts();
  } catch (e) {
    showErr(sErr, (e as Error).message);
  }
});

function renderMerges(ops: MergeOp[]): void {
  const merges = ops.filter((o) => o.op === "merge");
  if (!merges.length) {
    mergeList.textContent = "none";
    return;
  }
  mergeList.innerHTML = merges
    .map(
      (o, i) => `<div class="merge-row">
        <span>${esc(o.a)} &harr; ${esc(o.b)}</span>
        <button data-i="${i}" class="linkish">undo</button>
      </div>`,
    )
    .join("");
  mergeList.querySelectorAll<HTMLButtonElement>("button[data-i]").forEach((b) => {
    b.onclick = async () => {
      const o = merges[Number(b.dataset.i)];
      try {
        await post("/api/unmerge", { account: Number(cAcct.value), a: o.a, b: o.b });
        await loadContacts();
      } catch (e) {
        showErr(sErr, (e as Error).message);
      }
    };
  });
}

// ---------- wiring ----------

(document.getElementById("s-close") as HTMLElement).addEventListener("click", closeSettings);
(document.getElementById("gearbtn") as HTMLElement).addEventListener("click", toggleSettings);
