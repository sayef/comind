import { type ClassValue, clsx } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

/** Stable hue per repository, so a repo keeps its colour on every graph. */
export function repoHue(repo: string): number {
  let h = 0;
  for (const c of repo) h = (h * 31 + c.charCodeAt(0)) % 360;
  return h;
}

export const CONTRACTS = "_contracts";
