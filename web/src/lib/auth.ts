import { queryOptions } from "@tanstack/react-query";
import type { AuthStatus } from "@/generated/AuthStatus";
import { api } from "@/lib/api";

export const authQuery = queryOptions({ queryKey: ["auth"], queryFn: ({ signal }) => api<AuthStatus>("/api/auth", { signal }), staleTime: Infinity });

export const signedOut: AuthStatus = { required: true, authenticated: false };
