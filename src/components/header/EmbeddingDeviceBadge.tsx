import { useBackendStatus } from "@/hooks/use-backend-status";

function isGpu(device: string | null | undefined): boolean {
  if (!device) return false;
  const d = device.toLowerCase();
  return d.includes("cuda") || d.includes("directml") || d.includes("gpu");
}

function isFallback(device: string | null | undefined): boolean {
  return !!device && device.toLowerCase().includes("fallback");
}

function shortLabel(device: string | null | undefined): string {
  if (!device) return "…";
  if (device.startsWith("CUDA")) return "GPU";
  if (device.startsWith("DirectML")) return "GPU";
  if (device.startsWith("CPU")) return "CPU";
  return device;
}

const EmbeddingDeviceBadge: React.FC = () => {
  const status = useBackendStatus();
  const device = status?.embedding_device ?? null;
  const ready = status?.embedders_ready ?? false;

  const label = ready ? shortLabel(device) : "…";
  const title = ready
    ? `Embedding device: ${device ?? "unknown"}`
    : "Embedders loading…";
  const gpu = ready && isGpu(device) && !isFallback(device);
  const fallback = ready && isFallback(device);

  const color = !ready
    ? "border-border bg-muted/50 text-muted-foreground"
    : gpu
      ? "border-emerald-500/40 bg-emerald-500/10 text-emerald-700 dark:text-emerald-400"
      : fallback
        ? "border-amber-500/40 bg-amber-500/10 text-amber-800 dark:text-amber-400"
        : "border-border bg-muted/60 text-muted-foreground";

  return (
    <span
      className={`inline-flex items-center gap-1.5 rounded-full border px-2.5 py-1 text-xs font-medium tabular-nums ${color}`}
      title={title}
      role="status"
      aria-label={title}
    >
      <span
        className={`h-1.5 w-1.5 rounded-full ${
          !ready
            ? "bg-muted-foreground/50"
            : gpu
              ? "bg-emerald-500"
              : fallback
                ? "bg-amber-500"
                : "bg-muted-foreground"
        }`}
      />
      <span className="hidden sm:inline">Embed</span>
      <span className="font-semibold">{label}</span>
    </span>
  );
};

export default EmbeddingDeviceBadge;
