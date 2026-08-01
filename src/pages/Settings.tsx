import PageHead from "@/components/shared/PageHead";
import { useBackendStatus } from "@/hooks/use-backend-status";
import { card } from "@/styles/classes";
import packageJson from "../../package.json";

const deviceClass = (device: string) =>
  device.includes("GPU")
    ? "bg-success/15 text-success"
    : "bg-muted text-muted-foreground";

const formatBytes = (bytes: number | null | undefined) => {
  if (bytes === null || bytes === undefined) return "Size unavailable";
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
};

const Settings: React.FC = () => {
  const status = useBackendStatus();
  const models = status?.model_statuses ?? [];
  const storageLocations = [
    { name: "Qdrant", path: status?.qdrant_storage_path },
    { name: "SQLite", path: status?.sqlite_path },
    { name: "Logs", path: status?.logs_path, size: status?.logs_size_bytes },
  ];

  return (
    <div className="space-y-6">
      <PageHead
        title="Settings"
        description="View the local models and devices used for search."
      />

      <div>
        <h1 className="text-2xl font-bold text-foreground">Settings</h1>
      </div>

      <section className={card} aria-labelledby="model-status-heading">
        {!status ? (
          <p className="mt-4 text-sm text-muted-foreground">
            Loading model status...
          </p>
        ) : models.length === 0 ? (
          <p className="mt-4 text-sm text-muted-foreground">
            Model status is unavailable because the embedding models have not
            loaded.
          </p>
        ) : (
          <ul className="divide-y divide-border">
            <li className="py-4 first:pt-0">
              <div className="flex flex-wrap items-center gap-2">
                <span className="text-sm font-medium text-foreground">
                  Application Version
                </span>
                <span className="ml-auto rounded-full bg-muted px-2 py-0.5 text-xs font-medium text-muted-foreground">
                  {packageJson.version}
                </span>
              </div>
            </li>
            {models.map((model) => (
              <li key={model.role} className="py-4 first:pt-0 last:pb-0">
                <div className="flex flex-wrap items-center gap-2">
                  <span className="text-sm font-medium text-foreground">
                    {model.role}
                  </span>
                  <span className="text-sm text-muted-foreground">
                    {model.model}
                  </span>
                  <span
                    className={`ml-auto rounded-full px-2 py-0.5 text-xs font-medium ${deviceClass(model.device)}`}
                  >
                    {model.device}
                  </span>
                </div>
                {model.cpu_reason && (
                  <p className="mt-2 text-sm text-muted-foreground">
                    {model.cpu_reason}
                  </p>
                )}
              </li>
            ))}
            {storageLocations.map((location) => (
              <li key={location.name} className="py-4 last:pb-0">
                <div className="flex flex-wrap items-center gap-2">
                  <span className="text-sm font-medium text-foreground">
                    {location.name}
                  </span>
                  <span className="text-sm text-muted-foreground">
                    Local storage
                  </span>
                  {location.size !== undefined && (
                    <span className="ml-auto rounded-full bg-muted px-2 py-0.5 text-xs font-medium text-muted-foreground">
                      {formatBytes(location.size)}
                    </span>
                  )}
                </div>
                <p className="mt-2 break-all font-mono text-sm text-muted-foreground">
                  {location.path ?? "Location unavailable"}
                </p>
              </li>
            ))}
          </ul>
        )}
      </section>
    </div>
  );
};

export default Settings;
