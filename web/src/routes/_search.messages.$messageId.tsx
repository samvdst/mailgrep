import { createFileRoute, Link, useNavigate } from "@tanstack/react-router";
import { queryOptions, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  AlertTriangle,
  ArrowLeft,
  CalendarClock,
  Download,
  ExternalLink,
  FileText,
  Image,
  Images,
  Mail,
  Paperclip,
  Search,
  ShieldCheck,
  Users,
} from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { MessageDetail } from "@/generated/MessageDetail";
import type { ThreadResponse } from "@/generated/ThreadResponse";
import type { ImagesAllowedResponse } from "@/generated/ImagesAllowedResponse";
import { api, post } from "@/lib/api";
import { cn, formatBytes, formatDate } from "@/lib/utils";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";

const messageQuery = (id: number) => queryOptions({
  queryKey: ["message", id],
  queryFn: ({ signal }) => api<MessageDetail>(`/api/message/${id}`, { signal }),
  staleTime: 60_000,
});
const threadQuery = (account: number, id: string) => queryOptions({
  queryKey: ["thread", account, id],
  queryFn: ({ signal }) => api<ThreadResponse>(`/api/thread/${account}/${encodeURIComponent(id)}`, { signal }),
  staleTime: 60_000,
});

export const Route = createFileRoute("/_search/messages/$messageId")({
  loader: async ({ context, params }) => {
    const id = Number(params.messageId);
    await context.queryClient.prefetchQuery(messageQuery(id));
    const detail = context.queryClient.getQueryData<MessageDetail>(["message", id]);
    if (detail?.thread_size && detail.thread_size > 1) {
      void context.queryClient.prefetchQuery(threadQuery(detail.account_id, detail.thread_id));
    }
  },
  pendingComponent: DetailSkeleton,
  component: MessagePane,
});

type BodyTab = "html" | "text" | "raw";

function MessagePane() {
  const { messageId } = Route.useParams();
  const search = Route.useSearch();
  const navigate = useNavigate({ from: Route.fullPath });
  const queryClient = useQueryClient();
  const paneRef = useRef<HTMLElement>(null);
  const id = Number(messageId);
  const detailQuery = useQuery(messageQuery(id));
  const detail = detailQuery.data;
  const [tab, setTab] = useState<BodyTab>("text");
  const [loadImagesOnce, setLoadImagesOnce] = useState(false);
  const raw = useQuery({ queryKey: ["message", id, "raw"], queryFn: () => fetch(`/api/message/${id}/raw`).then((response) => response.ok ? response.text() : Promise.reject(new Error(`${response.status} ${response.statusText}`))), enabled: tab === "raw", staleTime: Infinity });
  const thread = useQuery({ ...threadQuery(detail?.account_id ?? 0, detail?.thread_id ?? ""), enabled: Boolean(detail && detail.thread_size > 1) });
  const allowImages = useMutation({
    mutationFn: () => post<ImagesAllowedResponse>(`/api/message/${id}/allow_images`),
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: ["message", id] }),
  });

  useEffect(() => {
    paneRef.current?.focus();
    if (detail) setTab(detail.has_html ? "html" : "text");
    setLoadImagesOnce(false);
  }, [detail, id]);
  useEffect(() => {
    const close = (event: KeyboardEvent) => {
      if (event.key === "Escape") void navigate({ to: "/", search });
    };
    addEventListener("keydown", close);
    return () => removeEventListener("keydown", close);
  }, [navigate, search]);

  if (detailQuery.isLoading) return <DetailSkeleton />;
  if (!detail) return <DetailError message={detailQuery.error?.message ?? "Message not found"} search={search} />;

  const changeQuery = (q: string) => void navigate({ to: "/", search: { ...search, q } });

  return (
    <section ref={paneRef} tabIndex={-1} role="region" className="scrollbar-thin absolute inset-0 z-20 overflow-y-auto border-l bg-background shadow-2xl outline-none xl:static xl:z-auto xl:min-h-0 xl:overflow-y-auto xl:shadow-none" aria-label="Message detail">
      <div className="sticky top-0 z-10 flex items-center justify-between border-b bg-background/92 px-4 py-2 backdrop-blur-xl">
        <Button variant="ghost" size="sm" asChild><Link to="/" search={search}><ArrowLeft />Back</Link></Button>
        <div className="flex items-center gap-1">
          <Badge variant="outline" className="hidden sm:inline-flex">{detail.thread_size > 1 ? `${detail.thread_size} messages` : "single message"}</Badge>
          <Button variant="ghost" size="icon" asChild><a href={`/api/message/${id}/raw`} target="_blank" rel="noreferrer" aria-label="Open raw message"><ExternalLink /></a></Button>
        </div>
      </div>

      <article className="mx-auto max-w-5xl p-4 sm:p-6 lg:p-8">
        <header>
          <div className="mb-3 flex flex-wrap items-center gap-2">
            {detail.folders.map((location) => <Badge key={`${location.folder}-${location.uid}`} variant="secondary">{location.folder} · {location.uid}</Badge>)}
            {detail.dates.skew && <Badge variant="destructive"><AlertTriangle className="size-3" />stored date differs</Badge>}
          </div>
          <h1 className="text-balance text-xl font-semibold leading-tight tracking-[-0.025em] sm:text-2xl">{detail.subject || "(no subject)"}</h1>
          <dl className="mt-5 grid grid-cols-[3.5rem_minmax(0,1fr)] gap-x-3 gap-y-2 text-sm">
            <dt className="pt-1 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">From</dt><dd className="flex flex-wrap gap-1.5">{detail.from.map((address, index) => <AddressChip key={`${address.email}-${index}`} address={address} onClick={changeQuery} />)}</dd>
            <dt className="pt-1 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">To</dt><dd className="flex flex-wrap gap-1.5">{detail.to.map((address, index) => <AddressChip key={`${address.email}-${index}`} address={address} onClick={changeQuery} />)}</dd>
            {!!detail.cc.length && <><dt className="pt-1 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">Cc</dt><dd className="flex flex-wrap gap-1.5">{detail.cc.map((address, index) => <AddressChip key={`${address.email}-${index}`} address={address} onClick={changeQuery} />)}</dd></>}
          </dl>
          <details className="group mt-4 text-xs text-muted-foreground">
            <summary className="flex w-fit list-none items-center gap-2 rounded-md py-1 hover:text-foreground"><CalendarClock className="size-3.5" /><time className="font-mono">{formatDate(detail.dates.canonical, detail.dates.offset_mins)}</time><Badge>{detail.dates.source}</Badge><span className="transition-transform group-open:rotate-90">›</span></summary>
            <div className="mt-2 grid max-w-md grid-cols-2 gap-1 rounded-lg border bg-muted/40 p-3 font-mono text-[11px]">
              <span>Received</span><span>{formatDate(detail.dates.received_top, detail.dates.offset_mins)}</span>
              <span>Date header</span><span>{formatDate(detail.dates.date_header, detail.dates.offset_mins)}</span>
              <span>Stored</span><span>{formatDate(detail.dates.internaldate, detail.dates.offset_mins)}</span>
            </div>
          </details>
        </header>

        {!!detail.attachments.length && (
          <section className="mt-7 rounded-xl border bg-card p-3">
            <h2 className="mb-2 flex items-center gap-2 px-1 text-xs font-semibold uppercase tracking-wider text-muted-foreground"><Paperclip className="size-3.5" />Attachments</h2>
            <div className="grid gap-2 sm:grid-cols-2">
              {detail.attachments.map((attachment) => (
                <a key={attachment.path} href={`/api/message/${id}/part/${encodeURIComponent(attachment.path)}`} target="_blank" rel="noreferrer" className="flex min-w-0 items-center gap-3 rounded-lg border bg-background p-3 transition-colors hover:bg-accent">
                  <span className="grid size-9 shrink-0 place-items-center rounded-lg bg-secondary"><Download className="size-4" /></span>
                  <span className="min-w-0"><span className="block truncate text-sm font-medium">{attachment.filename || `Part ${attachment.path}`}</span><span className="text-[11px] text-muted-foreground">{attachment.mime} · {formatBytes(attachment.size)}</span></span>
                </a>
              ))}
            </div>
          </section>
        )}

        {detail.thread_size > 1 && (
          <section className="mt-7">
            <div className="mb-2 flex items-center justify-between"><h2 className="flex items-center gap-2 text-sm font-semibold"><Users className="size-4" />Conversation</h2><Button variant="ghost" size="sm" onClick={() => changeQuery(`thread:${detail.thread_id}`)}><Search />Search thread</Button></div>
            <div className="flex gap-2 overflow-x-auto pb-2 scrollbar-thin">
              {thread.isLoading && [1, 2, 3].map((n) => <Skeleton key={n} className="h-20 min-w-52" />)}
              {thread.data?.messages.map((message) => (
                <Link key={message.id} to="/messages/$messageId" params={{ messageId: String(message.id) }} search={search} preload="intent" className={cn("min-w-52 max-w-64 rounded-xl border bg-card p-3 text-xs transition-colors hover:bg-accent", message.id === id && "border-primary bg-primary/5 ring-1 ring-primary/20")}>
                  <div className="flex justify-between gap-2"><strong className="truncate font-medium">{addressLabel(message.from[0])}</strong><time className="shrink-0 font-mono text-[10px] text-muted-foreground">{formatDate(message.date)}</time></div>
                  <p className="mt-1.5 line-clamp-2 leading-5 text-muted-foreground">{message.snippet}</p>
                </Link>
              ))}
            </div>
          </section>
        )}

        <section className="mt-7 overflow-hidden rounded-xl border bg-card">
          <div className="flex items-center justify-between border-b px-2">
            <div role="tablist" aria-label="Message body" className="flex">
              {detail.has_html && <TabButton active={tab === "html"} onClick={() => setTab("html")} icon={<Image />}>HTML</TabButton>}
              <TabButton active={tab === "text"} onClick={() => setTab("text")} icon={<FileText />}>Text</TabButton>
              <TabButton active={tab === "raw"} onClick={() => setTab("raw")} icon={<Mail />}>Raw</TabButton>
            </div>
            <Badge variant="outline" className="hidden sm:inline-flex"><ShieldCheck className="size-3" />sanitised</Badge>
          </div>
          {tab === "html" && detail.has_html && !detail.images_allowed && !loadImagesOnce && (
            <div className="flex flex-wrap items-center gap-2 border-b bg-muted/50 px-4 py-2 text-xs text-muted-foreground">
              <Images className="size-4" /><span className="mr-auto">Remote images are blocked to protect your privacy.</span>
              <Button variant="outline" size="sm" onClick={() => setLoadImagesOnce(true)}>Load once</Button>
              <Button variant="secondary" size="sm" disabled={allowImages.isPending} onClick={() => allowImages.mutate()}>Always allow sender</Button>
            </div>
          )}
          {allowImages.error && <p className="border-b bg-destructive/8 px-4 py-2 text-xs text-destructive">{allowImages.error.message}</p>}
          <div className="min-h-[24rem]">
            {tab === "html" && <iframe key={`${id}-${loadImagesOnce}-${detail.images_allowed}`} src={`/api/message/${id}/html${loadImagesOnce ? "?images=1" : ""}`} sandbox="allow-same-origin" title="HTML message" className="min-h-[65vh] w-full bg-white" />}
            {tab === "text" && <pre className="min-h-[24rem] whitespace-pre-wrap break-words p-4 font-mono text-xs leading-6 sm:p-6">{detail.body_text || "(no text body)"}</pre>}
            {tab === "raw" && <pre className="min-h-[24rem] overflow-x-auto whitespace-pre-wrap break-words p-4 font-mono text-[11px] leading-5 sm:p-6">{raw.isLoading ? "Loading raw message…" : raw.error ? `Failed to load raw message: ${raw.error.message}` : raw.data}</pre>}
          </div>
        </section>
      </article>
    </section>
  );
}

function TabButton({ active, onClick, icon, children }: { active: boolean; onClick: () => void; icon: React.ReactNode; children: React.ReactNode }) {
  return <button role="tab" aria-selected={active} onClick={onClick} className={cn("flex h-11 items-center gap-1.5 border-b-2 border-transparent px-3 text-xs font-medium text-muted-foreground [&_svg]:size-3.5", active && "border-primary text-foreground")}>{icon}{children}</button>;
}
function AddressChip({ address, onClick }: { address: { email: string | null; name: string | null }; onClick: (q: string) => void }) {
  const label = addressLabel(address);
  return address.email ? <button onClick={() => onClick(`contact:${address.email}`)} className="rounded-md bg-secondary px-2 py-1 text-xs hover:bg-accent" title={address.email}>{label}</button> : <span className="rounded-md bg-secondary px-2 py-1 text-xs">{label}</span>;
}
function addressLabel(address: { email: string | null; name: string | null } | undefined) { return address?.name || address?.email || "Unknown"; }
function DetailSkeleton() { return <section className="absolute inset-0 z-20 border-l bg-background p-6 xl:static"><div className="mx-auto max-w-4xl space-y-5"><Skeleton className="h-5 w-20" /><Skeleton className="h-8 w-2/3" /><Skeleton className="h-20" /><Skeleton className="h-80" /></div></section>; }
function DetailError({ message, search }: { message: string; search: Record<string, unknown> }) { return <section className="absolute inset-0 z-20 grid place-items-center border-l bg-background p-6 xl:static"><div className="text-center"><AlertTriangle className="mx-auto mb-3 size-8 text-destructive" /><h2 className="font-semibold">Could not open message</h2><p className="mt-1 text-sm text-muted-foreground">{message}</p><Button className="mt-4" asChild><Link to="/" search={search}>Back to results</Link></Button></div></section>; }
