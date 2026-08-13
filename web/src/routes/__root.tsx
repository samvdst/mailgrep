import { createRootRouteWithContext, Outlet } from "@tanstack/react-router";
import type { QueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import { SearchX } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useUiStore } from "@/store/ui";

export type RouterContext = { queryClient: QueryClient };

export const Route = createRootRouteWithContext<RouterContext>()({
  component: Root,
  notFoundComponent: () => (
    <main className="grid min-h-dvh place-items-center p-6 text-center">
      <div>
        <SearchX className="mx-auto mb-4 size-9 text-muted-foreground" />
        <h1 className="text-xl font-semibold">Nothing here</h1>
        <p className="mt-1 text-sm text-muted-foreground">This page is not in the archive.</p>
        <Button className="mt-5" onClick={() => location.assign("/")}>Back to search</Button>
      </div>
    </main>
  ),
});

function Root() {
  const theme = useUiStore((state) => state.theme);

  useEffect(() => {
    const media = matchMedia("(prefers-color-scheme: dark)");
    const apply = () => document.documentElement.classList.toggle("dark", theme === "dark" || (theme === "system" && media.matches));
    apply();
    media.addEventListener("change", apply);
    return () => media.removeEventListener("change", apply);
  }, [theme]);

  return <Outlet />;
}
