import { clsx, type ClassValue } from "clsx"
import { twMerge } from "tailwind-merge"

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}

export function errorMessage(err: unknown, fallback?: string): string {
  if (err instanceof Error) return err.message
  if (typeof err === "string") return err
  return fallback ?? String(err)
}
