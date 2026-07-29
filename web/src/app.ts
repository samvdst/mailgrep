// mailgrep SPA entry: URL-as-state search, results, facets, keyboard.

import {
  FacetData, SearchResponse, SearchRow, StatusAccount, api, esc, fmtDate, addrLabel,
} from "./api";
import { openDetail, closeDetail, isDetailOpen } from "./detail";
import { isSettingsOpen, closeSettings, openSettings, refreshStatus } from "./settings";

const qInput = document.getElementById("q") as HTMLInputElement;
const qError = document.getElementById("qerror") as HTMLElement;
const acctSel = document.getElementById("acctsel") as HTMLSelectElement;
const sortBtn = document.getElementById("sortbtn") as HTMLButtonElement;
const facetEl = document.getElementById("facets") as HTMLElement;
const metaEl = document.getElementById("meta") as HTMLElement;
const rowsEl = document.getElementById("rows") as HTMLElement;
const moreBtn = document.getElementById("more") as HTMLButtonElement;

const PAGE = 50;

const state = {
  q: "",
  account: "all",
  sort: "relevance" as "relevance" | "date",
  rows: [] as SearchRow[],
  total: 0,
  cross: false,
  offset: 0,
  lastPage: 0,
  selected: -1,
  haveAccounts: true,
};

let searchSeq = 0;
let debounceTimer: number | undefined;

// ---------- URL <-> state ----------

function readUrl(): void {
  const p = new URLSearchParams(location.search);
  state.q = p.get("q") ?? "";
  state.account = p.get("account") || "all";
  state.sort = p.get("sort") === "date" ? "date" : "relevance";
}

function urlFor(): string {
  const p = new URLSearchParams();
  if (state.q) p.set("q", state.q);
  if (state.account !== "all") p.set("account", state.account);
  if (state.sort === "date") p.set("sort", "date");
  const qs = p.toString();
  return qs ? `?${qs}` : location.pathname;
}

function writeUrl(): void {
  const u = urlFor();
  if (u !== location.search && !(u === location.pathname && !location.search)) {
    history.pushState(null, "", u);
  }
}

function reflectControls(): void {
  if (qInput.value !== state.q) qInput.value = state.q;
  acctSel.value = state.account;
  if (acctSel.value !== state.account) acctSel.value = "all"; // unknown id in URL
  sortBtn.innerHTML = `${state.sort} ⇅`;
}

// ---------- search ----------

async function runSearch(append = false): Promise<void> {
  const seq = ++searchSeq;
  if (!append) state.offset = 0;
  const p = new URLSearchParams({
    q: state.q,
    account: state.account,
    limit: String(PAGE),
    offset: String(state.offset),
  });
  if (state.sort === "date") p.set("sort", "date");
  let r: SearchResponse;
  try {
    r = await api<SearchResponse>(`/api/search?${p}`);
  } catch (e) {
    if (seq !== searchSeq) return;
    qError.textContent = (e as Error).message;
    qError.hidden = false;
    return;
  }
  if (seq !== searchSeq) return;
  qError.hidden = true;

  state.total = r.total;
  state.cross = !!r.cross_account;
  state.lastPage = r.results.length;
  if (append) {
    state.rows = state.rows.concat(r.results);
  } else {
    state.rows = r.results;
    state.selected = -1;
    renderFacets(r.facets);
  }
  renderResults();
}

function commit(): void {
  writeUrl();
  void runSearch();
}

/** Append a facet/filter term to the current query — never replaces it. */
export function appendQuery(term: string): void {
  const cur = qInput.value.trim();
  if (cur.split(/\s+/).includes(term)) return;
  state.q = cur ? `${cur} ${term}` : term;
  qInput.value = state.q;
  commit();
}

export function setQuery(q: string): void {
  state.q = q;
  qInput.value = q;
  commit();
}

// ---------- rendering ----------

function badgeHtml(r: SearchRow): string {
  let b = "";
  if (r.skew) b += '<span class="badge danger" title="stored date differs from derived date">&#9888;</span>';
  if (r.has_attach) b += '<span class="badge" title="has attachments">&#128206;</span>';
  if (r.thread_size > 1) b += `<span class="badge">${r.thread_size} in thread</span>`;
  return b;
}

function renderResults(): void {
  if (!state.haveAccounts) {
    metaEl.hidden = true;
    moreBtn.hidden = true;
    rowsEl.innerHTML = `<div class="empty">
      <p>No accounts yet.</p>
      <p>Open <button class="linkish" id="hint-settings">settings</button> and add an IMAP account to start indexing your archive.</p>
    </div>`;
    (document.getElementById("hint-settings") as HTMLElement).onclick = openSettings;
    return;
  }
  metaEl.hidden = false;
  metaEl.innerHTML =
    `${state.total.toLocaleString()} result${state.total === 1 ? "" : "s"}` +
    (state.cross
      ? ' <span class="dim">· results interleaved across accounts, ranked per account</span>'
      : "");

  if (!state.rows.length) {
    rowsEl.innerHTML = `<div class="empty"><p>no matches</p>${
      state.q ? `<p class="dim">query: <code>${esc(state.q)}</code></p>` : ""
    }</div>`;
    moreBtn.hidden = true;
    return;
  }

  rowsEl.innerHTML = state.rows
    .map(
      (r, i) => `<div class="row${i === state.selected ? " sel" : ""}" data-i="${i}">
        <div class="r1">
          <span class="subj">${esc(r.subject) || "(no subject)"}</span>
          <span class="badges">${badgeHtml(r)}</span>
          <span class="rdate mono">${fmtDate(r.date, r.date_offset_mins)}</span>
        </div>
        <div class="r2">
          <span class="from">${esc(addrLabel(r.from[0]))}</span>
          ${r.folders.map((f) => `<span class="chip">${esc(f)}</span>`).join("")}
          ${state.cross ? `<span class="chip acct">${esc(r.account)}</span>` : ""}
        </div>
        <div class="snip">${r.snippet}</div>
      </div>`,
    )
    .join("");
  moreBtn.hidden = !(state.rows.length < state.total && state.lastPage > 0);
}

function renderFacets(f: FacetData): void {
  const maxYear = Math.max(1, ...f.years.map((y) => y.count));
  const section = (title: string, body: string) =>
    body ? `<h3>${title}</h3><div class="fgroup">${body}</div>` : "";
  facetEl.innerHTML =
    section(
      "senders",
      f.senders
        .map(
          (s) => `<button class="facet" data-t="contact:${esc(s.email)}" title="${esc(s.email)}">
            <span class="flabel">${esc(s.name || s.email)}</span><span class="cnt">${s.count}</span>
          </button>`,
        )
        .join(""),
    ) +
    section(
      "organisations",
      f.orgs
        .map(
          (o) => `<button class="facet" data-t="org:${esc(o.org)}">
            <span class="flabel">${esc(o.org)}</span><span class="cnt">${o.count}</span>
          </button>`,
        )
        .join(""),
    ) +
    section(
      "years",
      f.years
        .map(
          (y) => `<button class="facet year" data-t="date:${y.year}">
            <span class="ylabel mono">${y.year}</span>
            <span class="bar"><span style="width:${Math.max(2, Math.round((y.count / maxYear) * 100))}%"></span></span>
            <span class="cnt">${y.count}</span>
          </button>`,
        )
        .join(""),
    ) +
    section(
      "attachments",
      f.exts
        .map(
          (x) => `<button class="facet" data-t="ext:${esc(x.ext)}">
            <span class="flabel">${esc(x.ext)}</span><span class="cnt">${x.count}</span>
          </button>`,
        )
        .join(""),
    );
}

facetEl.addEventListener("click", (e) => {
  const b = (e.target as HTMLElement).closest<HTMLButtonElement>(".facet");
  if (b?.dataset.t) appendQuery(b.dataset.t);
});

rowsEl.addEventListener("click", (e) => {
  const row = (e.target as HTMLElement).closest<HTMLElement>(".row");
  if (row) select(Number(row.dataset.i), true);
});

function select(i: number, open = false): void {
  if (i < 0 || i >= state.rows.length) return;
  state.selected = i;
  rowsEl.querySelectorAll(".row").forEach((el, j) => el.classList.toggle("sel", j === i));
  rowsEl.querySelector(".row.sel")?.scrollIntoView({ block: "nearest" });
  if (open) void openDetail(state.rows[i].id);
}

// ---------- accounts (from settings' status fetch) ----------

export function onAccountsChanged(accounts: StatusAccount[]): void {
  const hadNone = !state.haveAccounts;
  state.haveAccounts = accounts.length > 0;
  const prev = state.account;
  acctSel.innerHTML =
    '<option value="all">All accounts</option>' +
    accounts.map((a) => `<option value="${a.id}">${esc(a.name)}</option>`).join("");
  acctSel.value = prev;
  if (acctSel.value !== prev) {
    acctSel.value = "all";
    state.account = "all";
  }
  if (!state.haveAccounts || hadNone) {
    renderResults();
    if (state.haveAccounts) void runSearch();
  }
}

// ---------- header controls ----------

qInput.addEventListener("input", () => {
  clearTimeout(debounceTimer);
  debounceTimer = window.setTimeout(() => {
    state.q = qInput.value.trim();
    commit();
  }, 250);
});

acctSel.addEventListener("change", () => {
  state.account = acctSel.value;
  commit();
});

sortBtn.addEventListener("click", () => {
  state.sort = state.sort === "relevance" ? "date" : "relevance";
  reflectControls();
  commit();
});

moreBtn.addEventListener("click", () => {
  state.offset += PAGE;
  void runSearch(true);
});

window.addEventListener("popstate", () => {
  readUrl();
  reflectControls();
  void runSearch();
});

// ---------- keyboard ----------

document.addEventListener("keydown", (e) => {
  const t = e.target as HTMLElement;
  const typing = t instanceof HTMLInputElement || t instanceof HTMLTextAreaElement || t instanceof HTMLSelectElement;

  if (e.key === "/" && !typing) {
    e.preventDefault();
    qInput.focus();
    qInput.select();
    return;
  }
  if (e.key === "Escape") {
    if (isSettingsOpen()) closeSettings();
    else if (isDetailOpen()) closeDetail();
    else if (typing) t.blur();
    return;
  }
  // list navigation works from body or the search box, not from settings forms
  const navigable = !typing || t === qInput;
  if (!navigable) return;
  if (e.key === "ArrowDown") {
    e.preventDefault();
    select(Math.min(state.selected + 1, state.rows.length - 1));
  } else if (e.key === "ArrowUp") {
    e.preventDefault();
    select(Math.max(state.selected - 1, 0));
  } else if (e.key === "Enter" && state.selected >= 0 && (t === qInput || !typing)) {
    e.preventDefault();
    void openDetail(state.rows[state.selected].id);
  }
});

// ---------- boot ----------

readUrl();
reflectControls();
void refreshStatus().catch(() => {
  state.haveAccounts = false;
  renderResults();
});
void runSearch();
