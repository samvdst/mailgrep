import {
  createFileRoute,
  Link,
  Outlet,
  useNavigate,
  useRouterState,
} from "@tanstack/react-router";
import { infiniteQueryOptions, useInfiniteQuery, useQueryClient } from "@tanstack/react-query";
import {
  ArrowDownWideNarrow,
  ArrowUpDown,
  Command,
  Filter,
  Inbox,
  Menu,
  Moon,
  Paperclip,
  Search,
  Settings,
  Sun,
  TriangleAlert,
  X,
} from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { SearchResponse } from "@/generated/SearchResponse";
import type { SearchRow } from "@/generated/SearchRow";
import type { FacetData } from "@/generated/FacetData";
import { api } from "@/lib/api";
import { cn, formatDate } from "@/lib/utils";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from "@/components/ui/sheet";
import { Skeleton } from "@/components/ui/skeleton";
import { useUiStore } from "@/store/ui";
import { useMediaQuery } from "@/hooks/use-media-query";

const PAGE_SIZE = 50;
type SearchState = { q?: string; account?: string; sort?: "date" };

function searchQuery(q: string, account: string, sort: string) {
  return infiniteQueryOptions({
    queryKey: ["search", q, account, sort],
    initialPageParam: 0,
    queryFn: ({ pageParam, signal }) => {
      const params = new URLSearchParams({ q, account, limit: String(PAGE_SIZE), offset: String(pageParam) });
      if (sort === "date") params.set("sort", "date");
      return api<SearchResponse>(`/api/search?${params}`, { signal });
    },
    getNextPageParam: (last, pages, lastOffset) => {
      const loaded = pages.reduce((sum, page) => sum + page.results.length, 0);
      return loaded < last.total && last.results.length ? lastOffset + PAGE_SIZE : undefined;
    },
  });
}

export const Route = createFileRoute("/_search")({
  validateSearch: (search: Record<string, unknown>): SearchState => ({
    ...(typeof search.q === "string" && search.q ? { q: search.q } : {}),
    ...(typeof search.account === "string" && search.account !== "all" ? { account: search.account } : {}),
    ...(search.sort === "date" ? { sort: "date" as const } : {}),
  }),
  loaderDeps: ({ search }) => ({ q: search.q ?? "", account: search.account ?? "all", sort: search.sort ?? "relevance" }),
  loader: ({ context, deps }) => context.queryClient.prefetchInfiniteQuery(searchQuery(deps.q, deps.account, deps.sort)),
  component: SearchShell,
});

function SearchShell() {
  const search = Route.useSearch();
  const navigate = useNavigate({ from: Route.fullPath });
  const q = search.q ?? "";
  const account = search.account ?? "all";
  const sort = search.sort ?? "relevance";
  const query = useInfiniteQuery(searchQuery(q, account, sort));
  const pages = query.data?.pages;
  const firstPage = pages?.[0];
  const rows = pages?.flatMap((page) => page.results) ?? [];
  const detailOpen = useRouterState({ select: (state) => state.location.pathname.startsWith("/messages/") });
  const wideDetail = useMediaQuery("(min-width: 1280px)");
  const [draft, setDraft] = useState(q);
  const [activeIndex, setActiveIndex] = useState(-1);
  const searchRef = useRef<HTMLInputElement>(null);

  useEffect(() => setDraft(q), [q]);
  useEffect(() => {
    const timer = window.setTimeout(() => {
      const next = draft.trim();
      if (next !== q) void navigate({ search: (old) => ({ ...old, q: next || undefined }), replace: true });
    }, 180);
    return () => clearTimeout(timer);
  }, [draft, navigate, q]);
  useEffect(() => setActiveIndex(-1), [q, account, sort]);

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      const target = event.target as HTMLElement;
      const typing = /^(INPUT|TEXTAREA|SELECT)$/.test(target.tagName);
      if (event.key === "/" && !typing) {
        event.preventDefault();
        searchRef.current?.focus();
        searchRef.current?.select();
        return;
      }
      if (event.key === "Escape" && !detailOpen && document.activeElement === searchRef.current) {
        if (draft) setDraft("");
        else searchRef.current?.blur();
        return;
      }
      if (event.key === "Escape" && !detailOpen && !typing && q) {
        void navigate({ search: (old) => ({ ...old, q: undefined }) });
        return;
      }
      if ((target === document.body || target === searchRef.current) && (event.key === "ArrowDown" || event.key === "ArrowUp")) {
        if (!rows.length) return;
        event.preventDefault();
        setActiveIndex((current) => {
          const next = event.key === "ArrowDown" ? Math.min(current + 1, rows.length - 1) : Math.max(current - 1, 0);
          document.querySelector(`[data-result-index="${next}"]`)?.scrollIntoView({ block: "nearest" });
          return next;
        });
      }
      if (event.key === "Enter" && (target === document.body || target === searchRef.current) && activeIndex >= 0) {
        const row = rows[activeIndex];
        if (row) {
          event.preventDefault();
          void navigate({ to: "/messages/$messageId", params: { messageId: String(row.id) }, search });
        }
      }
    }
    addEventListener("keydown", onKeyDown);
    return () => removeEventListener("keydown", onKeyDown);
  }, [activeIndex, detailOpen, draft, navigate, q, rows, search]);

  const updateQuery = (token: string, replace = false) => {
    const next = replace ? token : q.split(/\s+/).includes(token) ? q : `${q} ${token}`.trim();
    void navigate({ search: (old) => ({ ...old, q: next || undefined }) });
  };

  return (
    <div className="flex h-dvh min-h-0 flex-col overflow-hidden bg-background">
      <Header
        draft={draft}
        setDraft={setDraft}
        searchRef={searchRef}
        account={account}
        sort={sort}
        accounts={firstPage?.accounts ?? []}
        onAccount={(value) => void navigate({ search: (old) => ({ ...old, account: value === "all" ? undefined : value }) })}
        onSort={() => void navigate({ search: (old) => ({ ...old, sort: sort === "date" ? undefined : "date" }) })}
        facets={firstPage?.facets}
        updateQuery={updateQuery}
      />
      {query.error && (
        <div className="flex items-center gap-2 border-b border-destructive/25 bg-destructive/8 px-4 py-2 text-sm text-destructive" role="alert">
          <TriangleAlert className="size-4" /> {query.error.message}
        </div>
      )}
      <main
        className={cn(
          "relative grid min-h-0 flex-1 overflow-hidden",
          detailOpen
            ? "lg:grid-cols-[15rem_minmax(22rem,32rem)] xl:grid-cols-[15rem_minmax(22rem,32rem)_minmax(0,1fr)]"
            : "lg:grid-cols-[16rem_minmax(0,1fr)]",
        )}
      >
        <aside className="scrollbar-thin hidden overflow-y-auto border-r bg-card/35 p-4 lg:block" inert={detailOpen && !wideDetail ? true : undefined} aria-hidden={detailOpen && !wideDetail ? true : undefined}>
          <Facets facets={firstPage?.facets} updateQuery={updateQuery} />
        </aside>
        <section className="scrollbar-thin min-w-0 overflow-y-auto bg-card/25" aria-label="Search results" inert={detailOpen && !wideDetail ? true : undefined} aria-hidden={detailOpen && !wideDetail ? true : undefined}>
          <Results
            rows={rows}
            total={firstPage?.total ?? 0}
            crossAccount={firstPage?.cross_account ?? false}
            queryText={q}
            loading={query.isLoading}
            fetching={query.isFetchingNextPage}
            hasAccounts={(firstPage?.accounts.length ?? 0) > 0}
            hasMore={query.hasNextPage}
            activeIndex={activeIndex}
            loadMore={() => void query.fetchNextPage()}
          />
        </section>
        <Outlet />
      </main>
    </div>
  );
}

type HeaderProps = {
  draft: string;
  setDraft: (value: string) => void;
  searchRef: React.RefObject<HTMLInputElement | null>;
  account: string;
  sort: string;
  accounts: { id: number; name: string }[];
  onAccount: (value: string) => void;
  onSort: () => void;
  facets?: FacetData;
  updateQuery: (token: string) => void;
};

function Header({ draft, setDraft, searchRef, account, sort, accounts, onAccount, onSort, facets, updateQuery }: HeaderProps) {
  const theme = useUiStore((state) => state.theme);
  const setTheme = useUiStore((state) => state.setTheme);
  const facetsOpen = useUiStore((state) => state.facetsOpen);
  const setFacetsOpen = useUiStore((state) => state.setFacetsOpen);

  return (
    <header className="relative z-30 shrink-0 border-b bg-background/88 px-3 py-2 backdrop-blur-xl md:px-4">
      <div className="mx-auto flex max-w-[1800px] flex-wrap items-center gap-2">
        <Link to="/" search={{}} className="group mr-1 flex h-9 items-center gap-2 rounded-md px-1.5 font-mono text-sm font-semibold tracking-tight outline-none focus-visible:ring-2 focus-visible:ring-ring">
          <span className="grid size-7 place-items-center rounded-lg bg-primary text-primary-foreground shadow-sm transition-transform group-hover:-rotate-3"><Inbox className="size-4" /></span>
          <span className="hidden sm:inline">mailgrep</span>
        </Link>
        <div className="order-3 flex w-full min-w-0 flex-1 items-center rounded-xl border border-input bg-card shadow-sm transition-shadow focus-within:border-ring focus-within:ring-2 focus-within:ring-ring/20 md:order-none md:w-auto">
          <Search className="ml-3 size-4 shrink-0 text-muted-foreground" />
          <input
            ref={searchRef}
            value={draft}
            onChange={(event) => setDraft(event.target.value)}
            type="search"
            placeholder="Search messages or try from:, date:, has:attachment…"
            autoComplete="off"
            spellCheck={false}
            className="h-10 min-w-0 flex-1 bg-transparent px-3 text-sm outline-none placeholder:text-muted-foreground"
            aria-label="Search archive"
          />
          {draft ? <button className="mr-1 rounded-md p-2 text-muted-foreground hover:bg-accent hover:text-foreground" onClick={() => setDraft("")} aria-label="Clear search"><X className="size-4" /></button> : <kbd className="mr-2 hidden rounded border bg-muted px-1.5 py-0.5 font-mono text-[10px] text-muted-foreground sm:inline-flex"><Command className="mr-0.5 size-3" />/</kbd>}
        </div>
        <select
          value={account}
          onChange={(event) => onAccount(event.target.value)}
          className="h-9 max-w-28 rounded-md border bg-background px-2 text-sm sm:max-w-40 outline-none focus-visible:ring-2 focus-visible:ring-ring"
          aria-label="Account"
        >
          <option value="all">All accounts</option>
          {accounts.map((item) => <option key={item.id} value={String(item.id)}>{item.name}</option>)}
        </select>
        <Button variant="outline" size="sm" onClick={onSort} title={`Sort by ${sort === "date" ? "relevance" : "date"}`}>
          {sort === "date" ? <ArrowDownWideNarrow /> : <ArrowUpDown />}<span className="hidden sm:inline">{sort === "date" ? "Newest" : "Relevant"}</span>
        </Button>
        <Button variant="outline" size="icon" className="lg:hidden" onClick={() => setFacetsOpen(true)} aria-label="Open filters"><Filter /></Button>
        <Button variant="ghost" size="icon" className="hidden sm:inline-flex" onClick={() => setTheme(theme === "dark" ? "light" : theme === "light" ? "system" : "dark")} title={`Theme: ${theme}`} aria-label="Change theme">
          {theme === "dark" ? <Moon /> : <Sun />}
        </Button>
        <Button variant="ghost" size="icon" asChild><Link to="/settings" search={{}} aria-label="Settings"><Settings /></Link></Button>
      </div>
      <Sheet open={facetsOpen} onOpenChange={setFacetsOpen}>
        <SheetContent side="left">
          <SheetHeader><SheetTitle>Refine results</SheetTitle><SheetDescription>Filters are added to your search.</SheetDescription></SheetHeader>
          <Facets facets={facets} updateQuery={(token) => { updateQuery(token); setFacetsOpen(false); }} />
        </SheetContent>
      </Sheet>
    </header>
  );
}

function Facets({ facets, updateQuery }: { facets?: FacetData; updateQuery: (token: string) => void }) {
  if (!facets) return <div className="space-y-6">{[1, 2, 3].map((n) => <Skeleton key={n} className="h-28" />)}</div>;
  const maxYear = Math.max(1, ...facets.years.map((item) => item.count));
  return (
    <nav className="space-y-7" aria-label="Search filters">
      <FacetGroup title="Senders">
        {facets.senders.slice(0, 10).map((item) => <FacetButton key={item.email} label={item.name || item.email} count={item.count} onClick={() => updateQuery(`contact:${item.email}`)} />)}
      </FacetGroup>
      <FacetGroup title="Organisations">
        {facets.orgs.slice(0, 10).map((item) => <FacetButton key={item.org} label={item.org} count={item.count} onClick={() => updateQuery(`org:${item.org}`)} />)}
      </FacetGroup>
      <FacetGroup title="Years">
        {facets.years.map((item) => (
          <button key={item.year} onClick={() => updateQuery(`date:${item.year}`)} className="group relative flex w-full items-center justify-between overflow-hidden rounded-md px-2 py-1.5 text-left text-xs hover:bg-accent">
            <span className="absolute inset-y-0 left-0 bg-primary/10 transition-colors group-hover:bg-primary/15" style={{ width: `${Math.max(3, item.count / maxYear * 100)}%` }} />
            <span className="relative font-mono">{item.year}</span><span className="relative tabular-nums text-muted-foreground">{item.count}</span>
          </button>
        ))}
      </FacetGroup>
      <FacetGroup title="Attachments">
        {facets.exts.map((item) => <FacetButton key={item.ext} label={item.ext.toUpperCase()} count={item.count} onClick={() => updateQuery(`ext:${item.ext}`)} />)}
      </FacetGroup>
    </nav>
  );
}

function FacetGroup({ title, children }: { title: string; children: React.ReactNode }) {
  return <section><h2 className="mb-2 px-2 text-[10px] font-semibold uppercase tracking-[0.16em] text-muted-foreground">{title}</h2><div className="space-y-0.5">{children}</div></section>;
}
function FacetButton({ label, count, onClick }: { label: string; count: number; onClick: () => void }) {
  return <button onClick={onClick} className="flex w-full items-center justify-between gap-2 rounded-md px-2 py-1.5 text-left text-xs hover:bg-accent"><span className="truncate">{label}</span><span className="tabular-nums text-muted-foreground">{count}</span></button>;
}

type ResultsProps = {
  rows: SearchRow[];
  total: number;
  crossAccount: boolean;
  queryText: string;
  loading: boolean;
  fetching: boolean;
  hasAccounts: boolean;
  hasMore: boolean;
  activeIndex: number;
  loadMore: () => void;
};

function Results({ rows, total, crossAccount, queryText, loading, fetching, hasAccounts, hasMore, activeIndex, loadMore }: ResultsProps) {
  const queryClient = useQueryClient();
  const selectedId = useRouterState({ select: (state) => Number(state.location.pathname.match(/^\/messages\/(\d+)/)?.[1]) });
  const density = useUiStore((state) => state.density);
  if (loading && !rows.length) return <div className="space-y-px p-3">{Array.from({ length: 8 }, (_, n) => <ResultSkeleton key={n} />)}</div>;
  if (!hasAccounts) return <Empty icon={<Inbox />} title="Your archive is empty" body="Add an IMAP account, then start a sync." action={<Button asChild><Link to="/settings" search={{}}>Add an account</Link></Button>} />;
  if (!rows.length) return <Empty icon={<Search />} title="No messages found" body={queryText ? `Nothing matched “${queryText}”. Try removing a filter.` : "There are no indexed messages yet."} />;
  return (
    <>
      <div className="sticky top-0 z-10 flex items-center justify-between border-b bg-background/90 px-4 py-2 text-xs text-muted-foreground backdrop-blur-md">
        <span><strong className="font-semibold text-foreground">{total.toLocaleString()}</strong> {total === 1 ? "message" : "messages"}{crossAccount ? " across accounts" : ""}</span>
        <span className="hidden font-mono text-[10px] md:inline">↑↓ navigate · enter open</span>
      </div>
      <div className="divide-y">
        {rows.map((row, index) => (
          <Link
            key={`${row.account_id}-${row.id}`}
            to="/messages/$messageId"
            params={{ messageId: String(row.id) }}
            search={(old) => old}
            data-result-index={index}
            preload="intent"
            onMouseEnter={() => void queryClient.prefetchQuery({ queryKey: ["message", row.id], queryFn: () => api(`/api/message/${row.id}`), staleTime: 60_000 })}
            className={cn("group block border-l-2 border-l-transparent px-4 outline-none [content-visibility:auto] [contain-intrinsic-size:auto_104px] transition-colors hover:bg-accent/45 focus-visible:bg-accent focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring", density === "compact" ? "py-2" : "py-3", (activeIndex === index || selectedId === row.id) && "border-l-primary bg-accent/65")}
          >
            <div className="flex items-start gap-3">
              <div className="min-w-0 flex-1">
                <div className="flex min-w-0 items-center gap-2">
                  <h3 className="truncate text-sm font-medium tracking-[-0.01em]">{row.subject || "(no subject)"}</h3>
                  {row.has_attach && <Paperclip className="size-3.5 shrink-0 text-muted-foreground" />}
                  {row.thread_size > 1 && <Badge variant="outline">{row.thread_size} in thread</Badge>}
                </div>
                <div className="mt-1 flex min-w-0 items-center gap-2 text-xs text-muted-foreground">
                  <span className="truncate text-foreground/75">{addressLabel(row.from[0])}</span>
                  {row.folders.slice(0, 2).map((folder) => <Badge key={folder} variant="secondary" className="max-w-28 truncate">{folder}</Badge>)}
                  {crossAccount && <Badge variant="outline">{row.account}</Badge>}
                  {row.skew && <Badge variant="destructive"><TriangleAlert className="size-3" />date</Badge>}
                </div>
                {/* Tantivy returns escaped text with server-owned <mark> highlights. */}
                <p className="mt-1.5 line-clamp-2 text-xs leading-5 text-muted-foreground [&_mark]:rounded-sm [&_mark]:bg-primary/15 [&_mark]:px-0.5 [&_mark]:text-foreground" dangerouslySetInnerHTML={{ __html: row.snippet }} />
              </div>
              <time className="shrink-0 font-mono text-[10px] text-muted-foreground sm:text-xs">{formatDate(row.date, row.date_offset_mins)}</time>
            </div>
          </Link>
        ))}
      </div>
      {hasMore && <div className="p-4 text-center"><Button variant="outline" disabled={fetching} onClick={loadMore}>{fetching ? "Loading…" : "Load more"}</Button></div>}
    </>
  );
}

function ResultSkeleton() {
  return <div className="space-y-2 border-b p-4"><div className="flex justify-between"><Skeleton className="h-4 w-2/5" /><Skeleton className="h-3 w-24" /></div><Skeleton className="h-3 w-1/4" /><Skeleton className="h-3 w-4/5" /></div>;
}
function Empty({ icon, title, body, action }: { icon: React.ReactNode; title: string; body: string; action?: React.ReactNode }) {
  return <div className="grid min-h-[60vh] place-items-center p-8 text-center"><div className="max-w-sm"><div className="mx-auto mb-4 grid size-12 place-items-center rounded-2xl bg-accent text-accent-foreground [&_svg]:size-5">{icon}</div><h2 className="font-semibold">{title}</h2><p className="mt-1.5 text-sm leading-6 text-muted-foreground">{body}</p>{action && <div className="mt-5">{action}</div>}</div></div>;
}

function addressLabel(address: { email: string | null; name: string | null } | undefined) {
  return address?.name || address?.email || "Unknown sender";
}
