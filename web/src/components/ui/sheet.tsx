import { Dialog } from "radix-ui";
import { X } from "lucide-react";
import type { ComponentProps } from "react";
import { cn } from "@/lib/utils";

export const Sheet = Dialog.Root;
export const SheetTrigger = Dialog.Trigger;
export const SheetClose = Dialog.Close;

export function SheetContent({ className, children, side = "right", ...props }: ComponentProps<typeof Dialog.Content> & { side?: "left" | "right" }) {
  return (
    <Dialog.Portal>
      <Dialog.Overlay className="fixed inset-0 z-50 bg-black/35 backdrop-blur-[2px] data-[state=closed]:animate-out data-[state=open]:animate-in" />
      <Dialog.Content
        className={cn("fixed inset-y-0 z-50 w-[min(92vw,26rem)] overflow-y-auto border bg-background p-5 shadow-2xl outline-none transition-transform duration-200", side === "right" ? "right-0 border-l data-[state=closed]:translate-x-full" : "left-0 border-r data-[state=closed]:-translate-x-full", className)}
        {...props}
      >
        {children}
        <Dialog.Close className="absolute right-4 top-4 rounded-md p-1.5 text-muted-foreground hover:bg-accent hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring" aria-label="Close">
          <X className="size-4" />
        </Dialog.Close>
      </Dialog.Content>
    </Dialog.Portal>
  );
}

export function SheetHeader({ className, ...props }: ComponentProps<"div">) {
  return <div className={cn("mb-5 space-y-1.5 pr-8", className)} {...props} />;
}
export function SheetTitle(props: ComponentProps<typeof Dialog.Title>) {
  return <Dialog.Title className="text-lg font-semibold tracking-tight" {...props} />;
}
export function SheetDescription(props: ComponentProps<typeof Dialog.Description>) {
  return <Dialog.Description className="text-sm text-muted-foreground" {...props} />;
}
