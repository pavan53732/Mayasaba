import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

/**
 * Joins class names and resolves Tailwind conflicts by last-wins.
 *
 * shadcn/ui primitives take a `className` that callers override with, so a caller's `px-6` has to beat the
 * component's own `px-4` rather than sit beside it in the class list where source order decides.
 */
export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}
