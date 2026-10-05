import { createFileRoute, Link, useNavigate } from "@tanstack/react-router";
import { queryOptions, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  AlertTriangle,
  ArrowLeft,
  Check,
  ChevronDown,
  Clock3,
  ContactRound,
  Database,
  FolderCog,
  HardDrive,
  LogOut,
  ImageOff,
  Inbox,
  LoaderCircle,
  MailPlus,
  Merge,
  Moon,
  Palette,
  RefreshCw,
  Search,
  Server,
  Settings2,
  Sun,
  Trash2,
  Undo2,
  Users,
} from "lucide-react";
import { useDeferredValue, useEffect, useState, type FormEvent } from "react";
import type { StatusResponse } from "@/generated/StatusResponse";
import type { StatusAccount } from "@/generated/StatusAccount";
import type { FoldersResponse } from "@/generated/FoldersResponse";
import type { ImageAllowancesResponse } from "@/generated/ImageAllowancesResponse";
import type { ContactsResponse } from "@/generated/ContactsResponse";
import type { MergesResponse } from "@/generated/MergesResponse";
import type { NewAccount } from "@/generated/NewAccount";
import type { SyncBody } from "@/generated/SyncBody";
import type { FolderExclusion } from "@/generated/FolderExclusion";
import type { IntervalBody } from "@/generated/IntervalBody";
import type { RenameBody } from "@/generated/RenameBody";
import type { RevokeBody } from "@/generated/RevokeBody";
import type { MergeBody } from "@/generated/MergeBody";
import { api, del, post, put } from "@/lib/api";
import { authQuery } from "@/lib/auth";
import { cn, formatBytes, formatLocalDate } from "@/lib/utils";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { useUiStore } from "@/store/ui";

type SettingsTab = "accounts" | "contacts" | "appearance";
type SettingsSearch = { tab?: SettingsTab };
const statusQuery = queryOptions({ queryKey: ["status"], queryFn: ({ signal }) => api<StatusResponse>("/api/status", { signal }), staleTime: 5_000 });

export const Route = createFileRoute("/settings")({
  validateSearch: (search: Record<string, unknown>): SettingsSearch => ({ ...(search.tab === "contacts" || search.tab === "appearance" ? { tab: search.tab } : {}) }),
  loader: ({ context }) => context.queryClient.prefetchQuery(statusQuery),
  component: SettingsPage,
});

function SettingsPage() {
  const { tab = "accounts" } = Route.useSearch();
  const navigate = useNavigate({ from: Route.fullPath });
  const status = useQuery({
    ...statusQuery,
    refetchInterval: (query) => query.state.data?.accounts.some((account) => account.progress?.running) ? 2_000 : false,
  });
  const auth = useQuery(authQuery);
  const logout = () => void post("/api/logout").finally(() => location.assign("/"));
  const setTab = (next: SettingsTab) => void navigate({ search: next === "accounts" ? {} : { tab: next }, replace: true });

  useEffect(() => {
    const close = (event: KeyboardEvent) => { if (event.key === "Escape") history.back(); };
    addEventListener("keydown", close);
    return () => removeEventListener("keydown", close);
  }, []);

  return (
    <div className="min-h-dvh bg-background">
      <header className="sticky top-0 z-30 border-b bg-background/90 backdrop-blur-xl">
        <div className="mx-auto flex h-14 max-w-6xl items-center gap-3 px-4">
          <Button variant="ghost" size="sm" asChild><Link to="/" search={{}}><ArrowLeft />Search</Link></Button>
          <div className="h-5 w-px bg-border" />
          <div className="flex min-w-0 items-center gap-2"><span className="grid size-7 place-items-center rounded-lg bg-primary text-primary-foreground"><Inbox className="size-4" /></span><strong className="truncate font-mono text-sm">mailgrep settings</strong></div>
          {status.data && <Badge variant="outline" className="ml-auto">v{status.data.version}</Badge>}
          {auth.data?.required && <Button variant="ghost" size="sm" className={status.data ? "" : "ml-auto"} onClick={logout}><LogOut />Sign out</Button>}
        </div>
      </header>
      <main className="mx-auto max-w-6xl px-4 py-6 sm:py-10">
        <div className="mb-8">
          <h1 className="text-2xl font-semibold tracking-tight sm:text-3xl">Settings</h1>
          <p className="mt-1 text-sm text-muted-foreground">Manage your archive, identities, and workspace.</p>
        </div>
        <div className="mb-7 flex gap-1 overflow-x-auto rounded-xl border bg-muted/50 p-1 sm:w-fit">
          <Tab active={tab === "accounts"} onClick={() => setTab("accounts")} icon={<Server />}>Accounts</Tab>
          <Tab active={tab === "contacts"} onClick={() => setTab("contacts")} icon={<Users />}>Contacts</Tab>
          <Tab active={tab === "appearance"} onClick={() => setTab("appearance")} icon={<Palette />}>Appearance</Tab>
        </div>
        {status.error && <ErrorBanner message={status.error.message} />}
        {tab === "accounts" && <AccountsPanel status={status.data} loading={status.isLoading} />}
        {tab === "contacts" && <ContactsPanel accounts={status.data?.accounts ?? []} />}
        {tab === "appearance" && <AppearancePanel />}
      </main>
    </div>
  );
}

function Tab({ active, onClick, icon, children }: { active: boolean; onClick: () => void; icon: React.ReactNode; children: React.ReactNode }) {
  return <button onClick={onClick} className={cn("flex h-9 items-center gap-2 rounded-lg px-3 text-sm font-medium text-muted-foreground transition-colors [&_svg]:size-4", active ? "bg-background text-foreground shadow-sm" : "hover:text-foreground")}>{icon}{children}</button>;
}

function AccountsPanel({ status, loading }: { status?: StatusResponse; loading: boolean }) {
  if (loading) return <div className="grid gap-4 lg:grid-cols-2"><Skeleton className="h-64" /><Skeleton className="h-64" /></div>;
  return (
    <div className="space-y-8">
      <section>
        <div className="mb-3 flex items-end justify-between"><div><h2 className="font-semibold">Mail accounts</h2><p className="text-sm text-muted-foreground">Sync and index each mailbox independently.</p></div><Badge variant="secondary">{status?.accounts.length ?? 0}</Badge></div>
        {status?.accounts.length ? <div className="grid items-start gap-4 lg:grid-cols-2">{status.accounts.map((account) => <AccountCard key={account.id} account={account} />)}</div> : <div className="surface-grid rounded-2xl border border-dashed p-10 text-center"><Server className="mx-auto mb-3 size-7 text-muted-foreground" /><h3 className="font-medium">No accounts yet</h3><p className="mt-1 text-sm text-muted-foreground">Connect IMAP below to start building your archive.</p></div>}
      </section>
      <AddAccount />
    </div>
  );
}

function AccountCard({ account }: { account: StatusAccount }) {
  const client = useQueryClient();
  const [advanced, setAdvanced] = useState(false);
  const [rename, setRename] = useState(account.name);
  const [bounded, setBounded] = useState("100");
  const invalidate = () => client.invalidateQueries({ queryKey: ["status"] });
  const action = useMutation({ mutationFn: ({ path, body }: { path: string; body?: unknown }) => post<unknown>(path, body), onSuccess: invalidate });
  const remove = useMutation({ mutationFn: () => del<unknown>(`/api/accounts/${account.id}`), onSuccess: () => { invalidate(); void client.invalidateQueries({ queryKey: ["search"] }); } });
  const progress = account.progress;
  const progressPercent = progress?.discovered ? Math.min(100, progress.processed / progress.discovered * 100) : 0;

  return (
    <article className="overflow-hidden rounded-2xl border bg-card shadow-sm">
      <div className="p-5">
        <div className="flex items-start gap-3">
          <span className="grid size-10 shrink-0 place-items-center rounded-xl bg-primary/10 text-primary"><Database className="size-5" /></span>
          <div className="min-w-0 flex-1"><div className="flex flex-wrap items-center gap-2"><h3 className="truncate font-semibold">{account.name}</h3><Badge variant={progress?.running ? "default" : "outline"}>{progress?.running ? "syncing" : account.kind}</Badge></div><p className="mt-0.5 truncate text-xs text-muted-foreground">{account.username ? `${account.username} · ` : ""}{account.host || "local fixture"}</p></div>
        </div>
        <div className="mt-5 grid grid-cols-2 gap-2 sm:grid-cols-3">
          <Metric icon={<Inbox />} value={account.message_count.toLocaleString()} label="Messages" />
          <Metric icon={<HardDrive />} value={formatBytes(account.index_size_bytes)} label="Index" />
          <Metric icon={<Clock3 />} value={formatLocalDate(account.last_sync_at)} label="Last sync" className="col-span-2 sm:col-span-1" />
        </div>
        {progress?.running && <div className="mt-4 rounded-xl bg-primary/7 p-3"><div className="mb-2 flex justify-between gap-3 text-xs"><span className="truncate font-medium">{progress.folder || "Preparing…"}</span><span className="tabular-nums text-muted-foreground">{progress.processed}/{progress.discovered}</span></div><div className="h-1.5 overflow-hidden rounded-full bg-primary/15"><div className="h-full rounded-full bg-primary transition-[width]" style={{ width: `${progressPercent}%` }} /></div><p className="mt-2 text-[11px] text-muted-foreground">{progress.new_msgs} new · {progress.removed} removed · {progress.failed} failed</p></div>}
        {(progress?.error || account.last_sync_status && account.last_sync_status !== "ok") && <p className="mt-3 flex items-start gap-2 rounded-lg bg-destructive/8 p-2 text-xs text-destructive"><AlertTriangle className="mt-0.5 size-3.5 shrink-0" />{progress?.error || account.last_sync_status}</p>}
        <div className="mt-5 flex flex-wrap gap-2">
          <Button size="sm" disabled={action.isPending || progress?.running} onClick={() => action.mutate({ path: `/api/accounts/${account.id}/sync` })}>{action.isPending ? <LoaderCircle className="animate-spin" /> : <RefreshCw />}Sync now</Button>
          <Button variant="outline" size="sm" onClick={() => setAdvanced(!advanced)}><Settings2 />Manage<ChevronDown className={cn("transition-transform", advanced && "rotate-180")} /></Button>
        </div>
      </div>
      {advanced && (
        <div className="space-y-5 border-t bg-muted/25 p-5">
          <SettingRow label="Bounded sync" description="Maximum messages per folder."><div className="flex gap-2"><Input className="w-24" type="number" min="1" value={bounded} onChange={(event) => setBounded(event.target.value)} /><Button variant="outline" size="sm" onClick={() => { const max = Number.parseInt(bounded); if (max > 0) action.mutate({ path: `/api/accounts/${account.id}/sync`, body: { max_per_folder: max } satisfies SyncBody }); }}>Start</Button></div></SettingRow>
          <SettingRow label="Schedule" description="Minutes between syncs; 0 is manual."><Input className="w-28" type="number" min="0" defaultValue={account.sync_interval_mins} onBlur={(event) => action.mutate({ path: `/api/accounts/${account.id}/interval`, body: { minutes: Math.max(0, Number(event.target.value) || 0) } satisfies IntervalBody })} /></SettingRow>
          <SettingRow label="Rename" description="Display name used in search."><div className="flex gap-2"><Input value={rename} onChange={(event) => setRename(event.target.value)} /><Button variant="outline" size="sm" disabled={!rename.trim() || rename.trim() === account.name} onClick={() => action.mutate({ path: `/api/accounts/${account.id}/rename`, body: { name: rename.trim() } satisfies RenameBody })}>Save</Button></div></SettingRow>
          <Folders accountId={account.id} />
          <ImageAllowances accountId={account.id} />
          <div className="flex flex-wrap gap-2 border-t pt-4">
            <Button variant="outline" size="sm" onClick={() => action.mutate({ path: `/api/accounts/${account.id}/rebuild` })}><RefreshCw />Rebuild index</Button>
            <Button variant="destructive" size="sm" disabled={remove.isPending} onClick={() => { if (confirm(`Delete ${account.name} and its local index? Mail on the server is untouched.`)) remove.mutate(); }}><Trash2 />Delete account</Button>
          </div>
          {(action.error || remove.error) && <ErrorBanner message={(action.error || remove.error)?.message ?? "Action failed"} />}
          {!!account.recent_syncs.length && <details><summary className="text-xs font-medium text-muted-foreground">Recent syncs</summary><div className="mt-2 divide-y rounded-lg border bg-background">{account.recent_syncs.slice(0, 5).map((sync) => <div key={sync.started_at} className="flex items-center justify-between gap-3 px-3 py-2 text-xs"><span>{formatLocalDate(sync.started_at)}</span><span className="text-muted-foreground">+{sync.new} / −{sync.removed} / {sync.failed} failed</span></div>)}</div></details>}
        </div>
      )}
    </article>
  );
}

function Metric({ icon, value, label, className }: { icon: React.ReactNode; value: string; label: string; className?: string }) { return <div className={cn("rounded-xl bg-muted/60 p-3", className)}><div className="flex items-center gap-1.5 text-[10px] font-medium uppercase tracking-wider text-muted-foreground [&_svg]:size-3">{icon}{label}</div><div className="mt-1 truncate text-sm font-semibold">{value}</div></div>; }
function SettingRow({ label, description, children }: { label: string; description: string; children: React.ReactNode }) { return <div className="grid gap-2 sm:grid-cols-[1fr_minmax(12rem,1.2fr)] sm:items-center"><div><h4 className="text-sm font-medium">{label}</h4><p className="text-xs text-muted-foreground">{description}</p></div>{children}</div>; }

function Folders({ accountId }: { accountId: number }) {
  const [open, setOpen] = useState(false);
  const query = useQuery({ queryKey: ["folders", accountId], queryFn: () => api<FoldersResponse>(`/api/accounts/${accountId}/folders`), enabled: open, staleTime: 60_000 });
  const [excluded, setExcluded] = useState<Set<string>>(new Set());
  const save = useMutation({ mutationFn: (body: FolderExclusion) => put<unknown>(`/api/accounts/${accountId}/folders`, body), onSuccess: () => setOpen(false) });
  useEffect(() => { if (query.data) setExcluded(new Set(query.data.folders.filter((folder) => folder.excluded).map((folder) => folder.name))); }, [query.data]);
  return <div className="rounded-xl border bg-background"><button onClick={() => setOpen(!open)} className="flex w-full items-center gap-2 p-3 text-left text-sm font-medium"><FolderCog className="size-4" />Folder exclusions<ChevronDown className={cn("ml-auto size-4 transition-transform", open && "rotate-180")} /></button>{open && <div className="border-t p-3"><p className="mb-2 text-xs text-muted-foreground">Checked folders are excluded on the next sync.</p>{query.isLoading ? <Skeleton className="h-24" /> : <div className="max-h-48 space-y-1 overflow-y-auto scrollbar-thin">{query.data?.folders.map((folder) => <label key={folder.name} className="flex items-center gap-2 rounded-md px-2 py-1.5 text-xs hover:bg-accent"><input type="checkbox" checked={excluded.has(folder.name)} onChange={() => setExcluded((current) => { const next = new Set(current); if (next.has(folder.name)) next.delete(folder.name); else next.add(folder.name); return next; })} className="accent-primary" />{folder.name}</label>)}</div>}<Button className="mt-3" size="sm" disabled={save.isPending} onClick={() => save.mutate({ excluded: [...excluded] })}><Check />Save folders</Button>{(query.error || save.error) && <p className="mt-2 text-xs text-destructive">{(query.error || save.error)?.message}</p>}</div>}</div>;
}

function ImageAllowances({ accountId }: { accountId: number }) {
  const [open, setOpen] = useState(false);
  const client = useQueryClient();
  const query = useQuery({ queryKey: ["image-allowances", accountId], queryFn: () => api<ImageAllowancesResponse>(`/api/accounts/${accountId}/image_allowances`), enabled: open });
  const revoke = useMutation({ mutationFn: (body: RevokeBody) => del<unknown>(`/api/accounts/${accountId}/image_allowances`, body), onSuccess: () => client.invalidateQueries({ queryKey: ["image-allowances", accountId] }) });
  return <div className="rounded-xl border bg-background"><button onClick={() => setOpen(!open)} className="flex w-full items-center gap-2 p-3 text-left text-sm font-medium"><ImageOff className="size-4" />Remote image allowances<Badge variant="secondary" className="ml-auto">{query.data?.allowances.length ?? ""}</Badge><ChevronDown className={cn("size-4 transition-transform", open && "rotate-180")} /></button>{open && <div className="border-t p-3">{query.isLoading ? <Skeleton className="h-16" /> : query.data?.allowances.length ? <div className="space-y-1">{query.data.allowances.map((item) => <div key={item.sender} className="flex items-center gap-2 rounded-md px-2 py-1.5 text-xs"><span className="min-w-0 flex-1 truncate">{item.sender}</span><span className="text-muted-foreground">{formatLocalDate(item.at)}</span><Button variant="ghost" size="sm" onClick={() => revoke.mutate({ sender: item.sender })}>Revoke</Button></div>)}</div> : <p className="text-xs text-muted-foreground">No senders are allowed.</p>}</div>}</div>;
}

function AddAccount() {
  const client = useQueryClient();
  const add = useMutation({ mutationFn: (body: NewAccount) => post<unknown>("/api/accounts", body), onSuccess: (_, variables) => { client.invalidateQueries({ queryKey: ["status"] }); if (!variables.fixture_dir) (document.getElementById("imap-form") as HTMLFormElement | null)?.reset(); } });
  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const data = new FormData(event.currentTarget);
    add.mutate({ name: String(data.get("name")), host: String(data.get("host")), port: Number(data.get("port")), username: String(data.get("username")), password: String(data.get("password")), security: String(data.get("security")), fixture_dir: null });
  }
  function submitFixture(event: FormEvent<HTMLFormElement>) { event.preventDefault(); const data = new FormData(event.currentTarget); add.mutate({ name: String(data.get("name")), host: "", port: 0, username: "", password: "", security: "ssl", fixture_dir: String(data.get("fixture_dir")) }); }
  return <section className="rounded-2xl border bg-card p-5 shadow-sm sm:p-6"><div className="mb-5 flex items-center gap-3"><span className="grid size-10 place-items-center rounded-xl bg-primary/10 text-primary"><MailPlus className="size-5" /></span><div><h2 className="font-semibold">Add IMAP account</h2><p className="text-sm text-muted-foreground">Credentials are verified before encrypted storage.</p></div></div><form id="imap-form" onSubmit={submit} autoComplete="off" className="grid gap-3 sm:grid-cols-2"><Field label="Name"><Input name="name" placeholder="Personal" required /></Field><Field label="IMAP host"><Input name="host" placeholder="imap.example.com" required /></Field><Field label="Username"><Input name="username" placeholder="you@example.com" required /></Field><Field label="Password"><Input name="password" type="password" required /></Field><Field label="Security"><select name="security" defaultValue="ssl" className="h-9 w-full rounded-md border bg-background px-3 text-sm"><option value="ssl">SSL/TLS</option><option value="starttls">STARTTLS</option></select></Field><Field label="Port"><Input name="port" type="number" min="1" max="65535" defaultValue="993" required /></Field><div className="sm:col-span-2"><Button type="submit" disabled={add.isPending}>{add.isPending ? <LoaderCircle className="animate-spin" /> : <MailPlus />}{add.isPending ? "Verifying…" : "Connect account"}</Button></div></form>{add.error && <div className="mt-4"><ErrorBanner message={add.error.message} /></div>}<details className="mt-5 border-t pt-4"><summary className="cursor-pointer text-xs text-muted-foreground">Add a local fixture account (development)</summary><form onSubmit={submitFixture} className="mt-3 flex flex-col gap-2 sm:flex-row"><Input name="name" placeholder="Demo" required /><Input name="fixture_dir" placeholder="/path/to/fixtures" required /><Button variant="outline" type="submit">Add fixture</Button></form></details></section>;
}
function Field({ label, children }: { label: string; children: React.ReactNode }) { return <label className="space-y-1.5 text-xs font-medium"><span>{label}</span>{children}</label>; }

function ContactsPanel({ accounts }: { accounts: StatusAccount[] }) {
  const navigate = useNavigate();
  const [account, setAccount] = useState<number | null>(accounts[0]?.id ?? null);
  const [filter, setFilter] = useState("");
  const deferredFilter = useDeferredValue(filter);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const client = useQueryClient();
  useEffect(() => { if (account == null && accounts[0]) setAccount(accounts[0].id); }, [account, accounts]);
  const contacts = useQuery({ queryKey: ["contacts", account, deferredFilter], queryFn: () => api<ContactsResponse>(`/api/contacts?account=${account}&q=${encodeURIComponent(deferredFilter)}`), enabled: account != null });
  const merges = useQuery({ queryKey: ["merges", account], queryFn: () => api<MergesResponse>(`/api/merges?account=${account}`), enabled: account != null });
  const merge = useMutation({ mutationFn: ({ undo, body }: { undo?: boolean; body: MergeBody }) => post<unknown>(undo ? "/api/unmerge" : "/api/merge", body), onSuccess: () => { setSelected(new Set()); client.invalidateQueries({ queryKey: ["contacts", account] }); client.invalidateQueries({ queryKey: ["merges", account] }); } });
  const selectedList = [...selected];
  return <div className="grid items-start gap-6 lg:grid-cols-[minmax(0,1fr)_20rem]"><section className="overflow-hidden rounded-2xl border bg-card shadow-sm"><div className="border-b p-4"><div className="flex flex-col gap-2 sm:flex-row"><select value={account ?? ""} onChange={(event) => { setAccount(Number(event.target.value)); setSelected(new Set()); }} className="h-9 rounded-md border bg-background px-3 text-sm">{accounts.map((item) => <option key={item.id} value={item.id}>{item.name}</option>)}</select><div className="relative min-w-0 flex-1"><Search className="absolute left-3 top-2.5 size-4 text-muted-foreground" /><Input value={filter} onChange={(event) => setFilter(event.target.value)} placeholder="Filter contacts" className="pl-9" /></div>{selected.size === 2 && <Button onClick={() => account && merge.mutate({ body: { account, a: selectedList[0]!, b: selectedList[1]! } })}><Merge />Merge</Button>}</div></div>{contacts.isLoading ? <div className="space-y-2 p-4">{[1,2,3,4].map((n) => <Skeleton key={n} className="h-14" />)}</div> : contacts.data?.contacts.length ? <div className="divide-y">{contacts.data.contacts.map((contact) => <label key={contact.email} className="flex cursor-pointer items-center gap-3 px-4 py-3 hover:bg-accent/40"><input type="checkbox" checked={selected.has(contact.email)} onChange={() => setSelected((current) => { const next = new Set(current); if (next.has(contact.email)) next.delete(contact.email); else next.add(contact.email); return next; })} className="accent-primary" /><span className="grid size-9 shrink-0 place-items-center rounded-full bg-secondary text-secondary-foreground"><ContactRound className="size-4" /></span><span className="min-w-0 flex-1"><span className="flex items-center gap-2"><strong className="truncate text-sm font-medium">{contact.display_name || contact.email}</strong>{contact.is_role && <Badge variant="outline">role</Badge>}</span><span className="block truncate text-xs text-muted-foreground">{contact.email}{contact.org ? ` · ${contact.org}` : ""}</span></span><span className="text-xs tabular-nums text-muted-foreground">{contact.msg_count}</span><Button variant="ghost" size="icon" onClick={(event) => { event.preventDefault(); void navigate({ to: "/", search: { q: `contact:${contact.email}`, account: account ? String(account) : undefined } }); }} aria-label={`Search mail for ${contact.email}`}><Search /></Button></label>)}</div> : <div className="p-10 text-center text-sm text-muted-foreground">No contacts found.</div>}{(contacts.error || merge.error) && <div className="p-4"><ErrorBanner message={(contacts.error || merge.error)?.message ?? "Contact action failed"} /></div>}</section><aside className="rounded-2xl border bg-card p-4 shadow-sm"><h2 className="mb-1 font-semibold">Merge history</h2><p className="mb-4 text-xs text-muted-foreground">Combined addresses search as one identity.</p>{merges.data?.ops.filter((op) => op.op === "merge").length ? <div className="space-y-2">{merges.data.ops.filter((op) => op.op === "merge").map((op) => <div key={`${op.a}-${op.b}-${op.at}`} className="rounded-xl border p-3 text-xs"><div className="truncate">{op.a}</div><div className="my-1 text-muted-foreground">↕ merged with</div><div className="truncate">{op.b}</div><Button variant="ghost" size="sm" className="mt-2" onClick={() => account && merge.mutate({ undo: true, body: { account, a: op.a, b: op.b } })}><Undo2 />Undo</Button></div>)}</div> : <p className="text-xs text-muted-foreground">No active merges.</p>}</aside></div>;
}

function AppearancePanel() {
  const theme = useUiStore((state) => state.theme);
  const density = useUiStore((state) => state.density);
  const setTheme = useUiStore((state) => state.setTheme);
  const setDensity = useUiStore((state) => state.setDensity);
  return <section className="max-w-2xl rounded-2xl border bg-card p-5 shadow-sm sm:p-6"><h2 className="font-semibold">Workspace</h2><p className="mb-6 text-sm text-muted-foreground">Preferences stay in this browser.</p><SettingRow label="Theme" description="Follow your device or choose explicitly."><div className="grid grid-cols-3 gap-2">{(["system", "light", "dark"] as const).map((value) => <button key={value} onClick={() => setTheme(value)} className={cn("rounded-xl border p-3 text-center text-xs capitalize hover:bg-accent", theme === value && "border-primary bg-primary/5 ring-1 ring-primary/20")}>{value === "dark" ? <Moon className="mx-auto mb-2 size-4" /> : <Sun className="mx-auto mb-2 size-4" />}{value}</button>)}</div></SettingRow><div className="my-6 border-t" /><SettingRow label="Result density" description="Controls vertical spacing in result lists."><div className="grid grid-cols-2 gap-2">{(["comfortable", "compact"] as const).map((value) => <button key={value} onClick={() => setDensity(value)} className={cn("rounded-xl border p-3 text-xs capitalize hover:bg-accent", density === value && "border-primary bg-primary/5 ring-1 ring-primary/20")}>{value}</button>)}</div></SettingRow></section>;
}
function ErrorBanner({ message }: { message: string }) { return <div className="flex items-start gap-2 rounded-lg border border-destructive/20 bg-destructive/8 p-3 text-xs text-destructive" role="alert"><AlertTriangle className="mt-0.5 size-4 shrink-0" />{message}</div>; }
