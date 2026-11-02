/** Hook for reading the shared backend readiness from {@link BackendStatusProvider}. */
import { useContext } from "react";
import { BackendStatusContext } from "@/contexts/BackendStatusContext";
import type { BackendStatus } from "@/types/api";

/** Current backend status, or `null` while the first status is loading. */
export function useBackendStatus(): BackendStatus | null {
  return useContext(BackendStatusContext);
}
