import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Inbox, LoaderCircle } from "lucide-react";
import { useState, type FormEvent } from "react";
import type { LoginBody } from "@/generated/LoginBody";
import type { OkResponse } from "@/generated/OkResponse";
import { post } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";

export function Login() {
  const client = useQueryClient();
  const [password, setPassword] = useState("");
  const login = useMutation({
    mutationFn: (body: LoginBody) => post<OkResponse>("/api/login", body),
    onSuccess: () => void client.invalidateQueries(),
  });
  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (password) login.mutate({ password });
  };

  return (
    <main className="grid min-h-dvh place-items-center bg-background p-6">
      <form onSubmit={submit} className="w-full max-w-xs">
        <div className="mb-6 flex items-center gap-2">
          <span className="grid size-8 place-items-center rounded-lg bg-primary text-primary-foreground"><Inbox className="size-4" /></span>
          <strong className="font-mono text-sm">mailgrep</strong>
        </div>
        <label htmlFor="password" className="mb-1.5 block text-sm font-medium">Password</label>
        <Input id="password" type="password" autoComplete="current-password" autoFocus value={password} onChange={(event) => setPassword(event.target.value)} />
        {login.error && <p className="mt-2 text-sm text-destructive">{login.error.message}</p>}
        <Button type="submit" className="mt-4 w-full" disabled={!password || login.isPending}>{login.isPending && <LoaderCircle className="animate-spin" />}Sign in</Button>
      </form>
    </main>
  );
}
