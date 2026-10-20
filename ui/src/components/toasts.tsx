import { type MutationMeta } from "@tanstack/react-query";
import { Toaster, toast } from "sonner";
import { useTheme } from "@/components/theme";
import { sentence } from "@/lib/utils";

declare module "@tanstack/react-query" {
  interface Register {
    mutationMeta: {
      /** The form shows this action's error itself; no toast for it. */
      inline?: boolean;
      /** No toast on success (the change is visible right away). */
      quiet?: boolean;
    };
  }
}

/** Toasts in the corner for every action that finishes. */
export function Toasts() {
  const { theme } = useTheme();
  return <Toaster theme={theme} position="bottom-right" richColors closeButton />;
}

/** A finished action: its server message as a success toast, or its error (unless shown inline). */
export function announce(data: unknown, error: Error | null, meta?: MutationMeta) {
  if (error) {
    if (error.name === "AbortError" || meta?.inline) return;
    toast.error(sentence(error.message));
    return;
  }
  const message = (data as { message?: unknown } | null)?.message;
  if (!meta?.quiet && typeof message === "string" && message) toast.success(sentence(message));
}
