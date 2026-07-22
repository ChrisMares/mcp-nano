import React, { useState, useMemo, useCallback, useEffect, useRef } from "react";
import { useLocalUpload } from "@/hooks/use-local-upload";
import { useAuth } from "@/hooks/useAuth";
import { useJobEvents, type JobProgressEvent } from "@/hooks/use-job-events";
import { getActiveJobs, crawlWebsite, embedWebsite, getMetadataValues } from "@/utils/apicalls";
import { CheckCircle } from "lucide-react";
import PageHead from "@/components/shared/PageHead";
import {
  wizardStepDot, wizardStepDotActive, wizardStepDotCompleted, wizardStepDotPending,
  wizardStepLabel, wizardConnector,
} from "@/styles/classes";

import EmbeddingTypeStep from "@/components/uploadfiles/EmbeddingTypeStep";
import ConfigureCodeStep from "@/components/uploadfiles/ConfigureCodeStep";
import ConfigureDocsStep from "@/components/uploadfiles/ConfigureDocsStep";
import ConfigureWebsiteStep from "@/components/uploadfiles/ConfigureWebsiteStep";
import UploadStep from "@/components/uploadfiles/UploadStep";
import WebsiteProcessingStep from "@/components/uploadfiles/WebsiteProcessingStep";
import JobStatusPanel from "@/components/uploadfiles/JobStatusPanel";
import type { EmbedJob } from "@/types/embed";

const isZipFile = (f: { name: string }) => f.name.toLowerCase().endsWith(".zip");

const urlToGroupName = (raw: string): string => {
  try {
    const u = new URL(raw);
    const path = u.pathname.replace(/\/+$/, "");
    return u.hostname + path;
  } catch {
    return raw || "website";
  }
};

type CollectionType = "codebase" | "general" | "website" | "";

const getStepLabels = (collection: CollectionType) =>
  collection === "website"
    ? ["Embedding Type", "Configure", "Processing"]
    : ["Embedding Type", "Configure", "Upload Files"];

const StepIndicator: React.FC<{ current: number; total: number; labels: string[]; onStepClick: (step: number) => void }> = ({ current, total, labels, onStepClick }) => (
  <div className="flex items-center justify-center mb-8">
    {Array.from({ length: total }, (_, i) => {
      const stepNum = i + 1;
      const isCompleted = stepNum < current;
      const isActive = stepNum === current;
      const canClick = isCompleted;
      return (
        <React.Fragment key={i}>
          <div className="flex flex-col items-center min-w-[70px]">
            <button
              type="button"
              disabled={!canClick}
              onClick={() => canClick && onStepClick(stepNum)}
              className={`${wizardStepDot} ${isCompleted ? wizardStepDotCompleted : isActive ? wizardStepDotActive : wizardStepDotPending} ${canClick ? "cursor-pointer hover:scale-110 transition-transform" : "cursor-default"}`}
            >
              {isCompleted ? <CheckCircle size={16} /> : stepNum}
            </button>
            <span className={`${wizardStepLabel} ${isActive ? "text-foreground" : "text-muted-foreground"}`}>
              {labels[i]}
            </span>
          </div>
          {i < total - 1 && (
            <div className={`${wizardConnector} ${stepNum < current ? "bg-success" : "bg-border"} self-start mt-4`} />
          )}
        </React.Fragment>
      );
    })}
  </div>
);

const UploadFiles: React.FC = () => {
  const { user } = useAuth();
  const [step, setStep] = useState(1);
  const [collection, setCollection] = useState<CollectionType>("");
  const [groupName, setGroupName] = useState<string>("");
  const [groupMode, setGroupMode] = useState<"existing" | "new">("existing");
  const [groupOptions, setGroupOptions] = useState<string[]>([]);
  const [codeUploadMode, setCodeUploadMode] = useState<"zip" | "individual" | "">("");
  const [repoName, setRepoName] = useState<string>("");
  const [repoMode, setRepoMode] = useState<"existing" | "new">("existing");
  const [repoOptions, setRepoOptions] = useState<string[]>([]);
  const [websiteUrl, setWebsiteUrl] = useState<string>("");
  const [websiteDepth, setWebsiteDepth] = useState<number>(1);
  const [websiteSameDomainOnly, setWebsiteSameDomainOnly] = useState<boolean>(true);
  const [websiteUrls, setWebsiteUrls] = useState<string[]>([]);
  const [websiteCrawlError, setWebsiteCrawlError] = useState<boolean>(false);
  const [websiteCrawling, setWebsiteCrawling] = useState<boolean>(false);
  const [websiteEmbedJobId, setWebsiteEmbedJobId] = useState<string | null>(null);
  const [mixWarning, setMixWarning] = useState("");
  const [activeJobs, setActiveJobs] = useState<EmbedJob[]>([]);
  const [completedJobs, setCompletedJobs] = useState<EmbedJob[]>([]);
  const [processing] = useState(false);
  const [lastSubmittedCount, setLastSubmittedCount] = useState(0);
  const trackedJobsRef = useRef<Map<string, EmbedJob>>(new Map());

  // -- Initial fetch: grab any active jobs the worker is already processing
  //    (e.g. jobs queued before the user opened the UploadFiles view or
  //    still-running jobs from a previous session that were reset to
  //    PENDING on shutdown). The live progress events from the worker then
  //    keep this list in sync without further polling.
  useEffect(() => {
    let cancelled = false;
    const fetchInitial = async () => {
      try {
        const res = await getActiveJobs();
        if (cancelled) return;
        const jobs = res.jobs ?? [];
        setActiveJobs(jobs);
        trackedJobsRef.current = new Map(jobs.map((j) => [j.job_id, j]));
      } catch {
        // The SQLite/Qdrant sidecars may not be ready yet on first mount; a
        // future event from the worker will refresh `activeJobs` as soon as
        // the corresponding job_status row exists.
      }
    };
    void fetchInitial();
    return () => {
      cancelled = true;
    };
  }, []);

  // -- Live push: subscribe once to the three job lifecycle Tauri events.
  //    The Rust worker emits `job_progress` on every progress update,
  //    `job_finished` at status=FINISHED, and `job_failed` at status=FAILED.
  //    This replaces the previous `setInterval(getActiveJobs, 5000)` polling
  //    loop — the user sees progress bar ticks within milliseconds of the
  //    worker emitting them, with zero network round-trips between updates.
  useJobEvents({
    onProgress: (e: JobProgressEvent) => {
      setActiveJobs((prev) => {
        const existing = prev.find((j) => j.job_id === e.job_id);
        if (!existing) {
          // New job we haven't seen before (queued by another view); add it
          // to the tracked set so subsequent progress events update it
          // in place.
          const fresh: EmbedJob = {
            job_id: e.job_id,
            status: "RUNNING",
            progress_percentage: e.percentage,
            file_name: null,
            created_at: new Date().toISOString(),
          };
          trackedJobsRef.current.set(e.job_id, fresh);
          return [...prev, fresh];
        }
        if (existing.progress_percentage === e.percentage) return prev;
        const updated: EmbedJob = { ...existing, progress_percentage: e.percentage, status: "RUNNING" };
        trackedJobsRef.current.set(e.job_id, updated);
        return prev.map((j) => (j.job_id === e.job_id ? updated : j));
      });
    },
    onFinished: (e: JobProgressEvent) => {
      const prev = trackedJobsRef.current.get(e.job_id);
      trackedJobsRef.current.delete(e.job_id);
      setActiveJobs((prevJobs) => prevJobs.filter((j) => j.job_id !== e.job_id));
      setCompletedJobs((prevDone) => {
        if (prevDone.find((j) => j.job_id === e.job_id)) return prevDone;
        return [
          ...prevDone,
          {
            job_id: e.job_id,
            status: "COMPLETED",
            progress_percentage: 100,
            file_name: prev?.file_name ?? null,
            created_at: prev?.created_at ?? new Date().toISOString(),
          },
        ];
      });
    },
    onFailed: (e: JobProgressEvent) => {
      const prev = trackedJobsRef.current.get(e.job_id);
      trackedJobsRef.current.delete(e.job_id);
      setActiveJobs((prevJobs) => prevJobs.filter((j) => j.job_id !== e.job_id));
      setCompletedJobs((prevDone) => {
        if (prevDone.find((j) => j.job_id === e.job_id)) return prevDone;
        return [
          ...prevDone,
          {
            job_id: e.job_id,
            status: "FAILED",
            progress_percentage: 100,
            file_name: prev?.file_name ?? null,
            created_at: prev?.created_at ?? new Date().toISOString(),
          },
        ];
      });
    },
  });

  const handleUploadSuccess = useCallback((submittedCount: number) => {
    setLastSubmittedCount(submittedCount);
    // The worker will emit `job_progress` events for the queued jobs as
    // soon as it claims them; no polling kick needed.
  }, []);

  const handleWebsiteSubmit = useCallback(async () => {
    setWebsiteCrawlError(false);
    setWebsiteUrls([]);
    setWebsiteEmbedJobId(null);
    setWebsiteCrawling(true);
    try {
      const res = await crawlWebsite(websiteUrl, websiteDepth, websiteSameDomainOnly);
      setWebsiteUrls(res.urls ?? []);
      setStep(3);
    } catch {
      setWebsiteCrawlError(true);
    } finally {
      setWebsiteCrawling(false);
    }
  }, [websiteUrl, websiteDepth, websiteSameDomainOnly]);

  const handleWebsiteEmbed = useCallback(async (urls: string[]) => {
    try {
      const res = await embedWebsite(urls, urlToGroupName(websiteUrl));
      const jobId = res.job_id;
      setWebsiteEmbedJobId(jobId);
      const newJob: EmbedJob = {
        job_id: jobId,
        status: "PENDING",
        progress_percentage: 0,
        file_name: urls[0],
        created_at: new Date().toISOString(),
      };
      trackedJobsRef.current.set(jobId, newJob);
      setActiveJobs(prev => {
        const existingIds = new Set(prev.map(j => j.job_id));
        return [...prev, ...(existingIds.has(jobId) ? [] : [newJob])];
      });
    } catch (err) {
      console.error("Failed to start website embedding:", err);
    }
  }, [websiteUrl]);

  // Fetch repo names when codebase is selected
  useEffect(() => {
    if (collection !== "codebase" || !user) return;
    getMetadataValues("codebase", "repo_name")
      .then((res) => setRepoOptions(res.values ?? []))
      .catch(() => setRepoOptions([]));
  }, [collection, user]);

  // Fetch group names when general is selected
  useEffect(() => {
    if (collection !== "general" || !user) return;
    getMetadataValues("general", "group")
      .then((res) => {
        const vals: string[] = res.values ?? [];
        if (!vals.includes("default")) vals.unshift("default");
        setGroupOptions(vals);
        if (!vals.includes(groupName) && groupMode === "existing") setGroupName("");
      })
      .catch(() => setGroupOptions(["default"]));
  }, [collection, user, groupName, groupMode]);

  const uploadProps = useLocalUpload({
    collection: collection === "" ? "general" : collection === "website" ? "general" : collection,
    codeUploadMode,
    repoName,
    groupName,
    maxFiles: 10,
    maxFileSize: 1000 * 1000 * 200,
    onSuccess: handleUploadSuccess,
  });

  const { files, setFiles } = uploadProps;

  // Prevent mixing zip and non-zip
  useEffect(() => {
    if (files.length === 0) { setMixWarning(""); return; }
    const hasZip = files.some(isZipFile);
    const hasNonZip = files.some((f) => !isZipFile(f));
    if (hasZip && hasNonZip) {
      const firstIsZip = isZipFile(files[0]);
      setFiles(files.filter((f) => isZipFile(f) === firstIsZip));
      setMixWarning("Cannot mix .zip and non-.zip files in the same upload. Conflicting files were removed.");
    } else {
      setMixWarning("");
    }
  }, [files, setFiles]);

  const { disableUpload, disableReason } = useMemo(() => {
    if (!collection) return { disableUpload: true, disableReason: "Select a collection before uploading." };
    if (collection === "codebase" && codeUploadMode === "individual" && !repoName.trim())
      return { disableUpload: true, disableReason: "Enter a repo name before uploading." };
    return { disableUpload: false, disableReason: "" };
  }, [collection, codeUploadMode, repoName]);

  if (!user) return null;

  const handleCollectionSelect = (val: "codebase" | "general" | "website") => {
    setCollection(val);
    setCodeUploadMode("");
    if (val !== "codebase") { setRepoName(""); setRepoMode("existing"); }
    if (val !== "general" && val !== "website") { setGroupName(""); setGroupMode("existing"); }
    if (val !== "website") { setWebsiteUrl(""); setWebsiteDepth(1); setWebsiteSameDomainOnly(true); setWebsiteUrls([]); setWebsiteCrawlError(false); setWebsiteCrawling(false); setWebsiteEmbedJobId(null); }
    setStep(2);
  };

  const handleStartAnother = () => {
    setCollection("");
    setGroupName("");
    setGroupMode("existing");
    setCodeUploadMode("");
    setRepoName("");
    setRepoMode("existing");
    setWebsiteUrl("");
    setWebsiteDepth(1);
    setWebsiteSameDomainOnly(true);
    setWebsiteUrls([]);
    setWebsiteCrawlError(false);
    setWebsiteCrawling(false);
    setWebsiteEmbedJobId(null);
    setStep(1);
  };

  const stepLabels = getStepLabels(collection);

  return (
    <div className="rounded-lg border border-border bg-card p-6">
      <PageHead
        title="Embed Files & Code Repos"
        description="Upload documents and code repositories for embedding and search."
        path="/embed/upload"
      />
      <h1 className="text-2xl font-bold text-foreground mb-1">Upload &amp; Embed Files</h1>
      <p className="text-muted-foreground mb-6 text-sm">
        Use the steps below to select data type, configure scope, and upload files.
      </p>

      <StepIndicator current={step} total={3} labels={stepLabels} onStepClick={(s) => setStep(s)} />

      <div className="max-w-3xl mx-auto">
        {step === 1 && (
          <EmbeddingTypeStep onSelect={handleCollectionSelect} />
        )}

        {step === 2 && collection === "codebase" && (
          <ConfigureCodeStep
            codeUploadMode={codeUploadMode}
            repoName={repoName}
            repoMode={repoMode}
            repoOptions={repoOptions}
            onSelectZip={() => { setCodeUploadMode("zip"); setRepoName(""); setRepoMode("existing"); setStep(3); }}
            onSelectIndividual={() => setCodeUploadMode("individual")}
            onRepoChange={(value, mode) => { setRepoName(value); setRepoMode(mode); }}
            onBack={() => { setStep(1); setCodeUploadMode(""); }}
            onNext={() => setStep(3)}
          />
        )}

        {step === 2 && collection === "general" && (
          <ConfigureDocsStep
            groupName={groupName}
            groupMode={groupMode}
            groupOptions={groupOptions}
            onGroupChange={(value, mode) => { setGroupName(value); setGroupMode(mode); }}
            onBack={() => setStep(1)}
            onNext={() => setStep(3)}
          />
        )}

        {step === 2 && collection === "website" && (
          <ConfigureWebsiteStep
            websiteUrl={websiteUrl}
            depth={websiteDepth}
            sameDomainOnly={websiteSameDomainOnly}
            isCrawling={websiteCrawling}
            onUrlChange={setWebsiteUrl}
            onDepthChange={setWebsiteDepth}
            onSameDomainChange={setWebsiteSameDomainOnly}
            onBack={() => setStep(1)}
            onNext={handleWebsiteSubmit}
          />
        )}

        {step === 3 && collection !== "website" && (
          <UploadStep
            collection={collection}
            codeUploadMode={codeUploadMode}
            repoName={repoName}
            groupName={groupName}
            uploadProps={uploadProps}
            disableUpload={disableUpload}
            disableReason={disableReason}
            mixWarning={mixWarning}
            lastSubmittedCount={lastSubmittedCount}
            onDismissSubmitted={() => setLastSubmittedCount(0)}
            onBack={() => setStep(2)}
          />
        )}

        {step === 3 && collection === "website" && (
          <WebsiteProcessingStep
            websiteUrl={websiteUrl}
            crawledUrls={websiteUrls}
            crawlError={websiteCrawlError}
            groupName={urlToGroupName(websiteUrl)}
            embedJobId={websiteEmbedJobId}
            activeJobs={activeJobs}
            completedJobs={completedJobs}
            onEmbed={handleWebsiteEmbed}
            onStartAnother={handleStartAnother}
          />
        )}
      </div>

      <JobStatusPanel
        processing={processing}
        activeJobs={activeJobs}
        completedJobs={completedJobs}
      />
    </div>
  );
};

export default UploadFiles;
