import React, { useState, useMemo, useCallback, useEffect, useRef } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useLocalUpload } from "@/hooks/use-local-upload";
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
import type { EmbedJob, UploadJobEntry } from "@/types/embed";

interface CrawlProgressEvent {
  url: string;
  phase: string;
  found_count: number;
}

const mergeJobFromEvent = (prev: EmbedJob | undefined, e: JobProgressEvent, status: string): EmbedJob => ({
  job_id: e.job_id,
  status,
  progress_percentage: e.percentage ?? prev?.progress_percentage ?? 0,
  file_name: e.file_name ?? prev?.file_name ?? null,
  created_at: prev?.created_at ?? new Date().toISOString(),
  message: e.message ?? prev?.message ?? null,
});

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
  const [crawlCurrentUrl, setCrawlCurrentUrl] = useState<string | null>(null);
  const [crawlFoundCount, setCrawlFoundCount] = useState(0);
  const [websiteEmbedJobId, setWebsiteEmbedJobId] = useState<string | null>(null);
  const [mixWarning, setMixWarning] = useState("");
  const [activeJobs, setActiveJobs] = useState<EmbedJob[]>([]);
  const [completedJobs, setCompletedJobs] = useState<EmbedJob[]>([]);
  const [lastSubmittedCount, setLastSubmittedCount] = useState(0);
  const trackedJobsRef = useRef<Map<string, EmbedJob>>(new Map());

  useEffect(() => {
    let cancelled = false;
    const fetchInitial = async () => {
      try {
        const res = await getActiveJobs();
        if (cancelled) return;
        const jobs = (res.jobs ?? []).map((j) => ({
          job_id: j.job_id,
          status: j.status,
          progress_percentage: j.progress_percentage,
          file_name: j.file_name,
          created_at: j.created_at,
          message: null,
        }));
        setActiveJobs(jobs);
        trackedJobsRef.current = new Map(jobs.map((j) => [j.job_id, j]));
      } catch {
        // sidecars may not be ready yet
      }
    };
    void fetchInitial();
    return () => {
      cancelled = true;
    };
  }, []);

  const upsertActive = useCallback((job: EmbedJob) => {
    trackedJobsRef.current.set(job.job_id, job);
    setActiveJobs((prev) => {
      const idx = prev.findIndex((j) => j.job_id === job.job_id);
      if (idx < 0) return [...prev, job];
      const next = [...prev];
      next[idx] = job;
      return next;
    });
  }, []);

  useJobEvents({
    onQueued: (e: JobProgressEvent) => {
      const prev = trackedJobsRef.current.get(e.job_id);
      upsertActive(mergeJobFromEvent(prev, e, e.status ?? "PENDING"));
    },
    onProgress: (e: JobProgressEvent) => {
      const prev = trackedJobsRef.current.get(e.job_id);
      const next = mergeJobFromEvent(prev, e, e.status ?? "RUNNING");
      if (
        prev &&
        prev.progress_percentage === next.progress_percentage &&
        prev.status === next.status &&
        prev.message === next.message &&
        prev.file_name === next.file_name
      ) {
        return;
      }
      upsertActive(next);
    },
    onFinished: (e: JobProgressEvent) => {
      const prev = trackedJobsRef.current.get(e.job_id);
      trackedJobsRef.current.delete(e.job_id);
      setActiveJobs((prevJobs) => prevJobs.filter((j) => j.job_id !== e.job_id));
      const done = mergeJobFromEvent(prev, e, "COMPLETED");
      done.progress_percentage = 100;
      setCompletedJobs((prevDone) => {
        if (prevDone.find((j) => j.job_id === e.job_id)) return prevDone;
        return [...prevDone, done];
      });
    },
    onFailed: (e: JobProgressEvent) => {
      const prev = trackedJobsRef.current.get(e.job_id);
      trackedJobsRef.current.delete(e.job_id);
      setActiveJobs((prevJobs) => prevJobs.filter((j) => j.job_id !== e.job_id));
      const failed = mergeJobFromEvent(prev, e, "FAILED");
      failed.progress_percentage = 100;
      setCompletedJobs((prevDone) => {
        if (prevDone.find((j) => j.job_id === e.job_id)) return prevDone;
        return [...prevDone, failed];
      });
    },
  });

  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    let cancelled = false;
    void listen<CrawlProgressEvent>("crawl_progress", (ev) => {
      if (cancelled) return;
      const { url, phase, found_count } = ev.payload;
      setCrawlFoundCount(found_count);
      if (phase === "done") {
        setCrawlCurrentUrl(null);
        return;
      }
      if (url) setCrawlCurrentUrl(url);
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  const handleUploadSuccess = useCallback((submittedCount: number, jobs: UploadJobEntry[]) => {
    setLastSubmittedCount(submittedCount);
    for (const entry of jobs) {
      const job: EmbedJob = {
        job_id: entry.job_id,
        status: entry.status || "PENDING",
        progress_percentage: 0,
        file_name: entry.filename,
        created_at: new Date().toISOString(),
        message: "Queued",
      };
      trackedJobsRef.current.set(job.job_id, job);
    }
    setActiveJobs((prev) => {
      const byId = new Map(prev.map((j) => [j.job_id, j]));
      for (const entry of jobs) {
        const existing = byId.get(entry.job_id);
        byId.set(entry.job_id, {
          job_id: entry.job_id,
          status: existing?.status === "RUNNING" ? existing.status : entry.status || "PENDING",
          progress_percentage: existing?.progress_percentage ?? 0,
          file_name: entry.filename || existing?.file_name || null,
          created_at: existing?.created_at ?? new Date().toISOString(),
          message: existing?.message ?? "Queued",
        });
      }
      return Array.from(byId.values());
    });
  }, []);

  const handleWebsiteSubmit = useCallback(async () => {
    setWebsiteCrawlError(false);
    setWebsiteUrls([]);
    setWebsiteEmbedJobId(null);
    setCrawlCurrentUrl(null);
    setCrawlFoundCount(0);
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
    if (collection !== "codebase") return;
    getMetadataValues("codebase", "repo_name")
      .then((res) => setRepoOptions(res.values ?? []))
      .catch(() => setRepoOptions([]));
  }, [collection]);

  // Fetch group names when general is selected
  useEffect(() => {
    if (collection !== "general") return;
    getMetadataValues("general", "group")
      .then((res) => {
        const vals: string[] = res.values ?? [];
        if (!vals.includes("default")) vals.unshift("default");
        setGroupOptions(vals);
        if (!vals.includes(groupName) && groupMode === "existing") setGroupName("");
      })
      .catch(() => setGroupOptions(["default"]));
  }, [collection, groupName, groupMode]);

  const uploadProps = useLocalUpload({
    collection: collection === "" ? "general" : collection === "website" ? "general" : collection,
    codeUploadMode,
    repoName,
    groupName,
    maxFiles: 10,
    maxFileSize: Number.POSITIVE_INFINITY,
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

  const handleCollectionSelect = (val: "codebase" | "general" | "website") => {
    setCollection(val);
    setCodeUploadMode("");
    if (val !== "codebase") { setRepoName(""); setRepoMode("existing"); }
    if (val !== "general" && val !== "website") { setGroupName(""); setGroupMode("existing"); }
    if (val !== "website") {
      setWebsiteUrl("");
      setWebsiteDepth(1);
      setWebsiteSameDomainOnly(true);
      setWebsiteUrls([]);
      setWebsiteCrawlError(false);
      setWebsiteCrawling(false);
      setCrawlCurrentUrl(null);
      setCrawlFoundCount(0);
      setWebsiteEmbedJobId(null);
    }
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
    setCrawlCurrentUrl(null);
    setCrawlFoundCount(0);
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
            crawlCurrentUrl={crawlCurrentUrl}
            crawlFoundCount={crawlFoundCount}
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
        processing={uploadProps.loading}
        activeJobs={activeJobs}
        completedJobs={completedJobs}
      />
    </div>
  );
};

export default UploadFiles;
