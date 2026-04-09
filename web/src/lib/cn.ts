import { clsx, type ClassValue } from "clsx";

/** Merge Tailwind class names with clsx. Handles conditionals safely. */
export function cn(...inputs: ClassValue[]) {
  return clsx(inputs);
}
