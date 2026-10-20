import { useEffect, useRef } from "react";

/** An abort signal per run of a request: `next()` for a new run (stopping the last), `abort()` to cancel. */
export function useAbort() {
  const current = useRef<AbortController | null>(null);
  useEffect(() => () => current.current?.abort(), []);
  return {
    next: () => {
      current.current?.abort();
      current.current = new AbortController();
      return current.current.signal;
    },
    abort: () => current.current?.abort(),
  };
}
