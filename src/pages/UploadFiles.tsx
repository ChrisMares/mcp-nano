import React, { useState, useMemo, useCallback, useEffect, useRef } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useLocalUpload } from "@/hooks/use-local-upload";
import { useJobTracking } from "@/hooks/use-job-tracking";
import { cancelWebsiteCrawl, crawlWebsite, embedWebsite, getMetadataValues } from "@/utils/apicalls";
import PageHead from "@/components/shared/PageHead";
import StepIndicator from "@/components/shared/StepIndicator";

import EmbeddingTypeStep from "@/components/uploadfiles/EmbeddingTypeStep";
import ConfigureCodeStep from "@/components/uploadfiles/ConfigureCodeStep";
import ConfigureDocsStep from "@/components/uploadfiles/ConfigureDocsStep";
import ConfigureWebsiteStep from "@/components/uploadfiles/ConfigureWebsiteStep";
import UploadStep from "@/components/uploadfiles/UploadStep";
import WebsiteProcessingStep from "@/components/uploadfiles/WebsiteProcessingStep";
import JobStatusPanel from "@/components/uploadfiles/JobStatusPanel";
import type { UploadJobEntry } from "@/types/embed";

interface CrawlProgressEvent {
  url: string;
  phase: string;
  found_count: number;
}

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
  const [websiteRenderJavascript, setWebsiteRenderJavascript] = useState<boolean>(false);
  const [websiteUrls, setWebsiteUrls] = useState<string[]>([]);
  const [websiteCrawlError, setWebsiteCrawlError] = useState<boolean>(false);
  const [websiteCrawling, setWebsiteCrawling] = useState<boolean>(false);
  const [crawlCurrentUrl, setCrawlCurrentUrl] = useState<string | null>(null);
  const [crawlFoundCount, setCrawlFoundCount] = useState(0);
  const [websiteEmbedJobId, setWebsiteEmbedJobId] = useState<string | null>(null);
  const [mixWarning, setMixWarning] = useState("");
  const [lastSubmittedCount, setLastSubmittedCount] = useState(0);
  const crawlSessionRef = useRef(0);
  const { activeJobs, completedJobs, trackUploadedJobs, trackQueuedJob } = useJobTracking();

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
    trackUploadedJobs(jobs);
  }, [trackUploadedJobs]);

  const handleWebsiteSubmit = useCallback(async () => {
    setWebsiteCrawlError(false);
    setWebsiteUrls([]);
    setWebsiteEmbedJobId(null);
    setCrawlCurrentUrl(null);
    setCrawlFoundCount(0);
    setWebsiteCrawling(true);
    const session = ++crawlSessionRef.current;
    try {
      const res = await crawlWebsite(
        websiteUrl,
        websiteDepth,
        websiteSameDomainOnly,
        websiteRenderJavascript,
      );
      if (crawlSessionRef.current === session) {
        setWebsiteUrls(res.urls ?? []);
        setStep((s) => (s === 2 ? 3 : s));
      }
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      if (crawlSessionRef.current === session && !message.includes("cancelled")) {
        setWebsiteCrawlError(true);
      }
    } finally {
      if (crawlSessionRef.current === session) setWebsiteCrawling(false);
    }
  }, [websiteUrl, websiteDepth, websiteSameDomainOnly, websiteRenderJavascript]);

  const handleCancelCrawl = useCallback(() => {
    void cancelWebsiteCrawl();
  }, []);

  const handleWebsiteBack = useCallback(() => {
    setStep(1);
  }, []);

  const handleStepClick = useCallback((nextStep: number) => {
    setStep(nextStep);
  }, []);

  const handleWebsiteEmbed = useCallback(async (urls: string[]) => {
    try {
      const res = await embedWebsite(
        urls,
        urlToGroupName(websiteUrl),
        websiteRenderJavascript,
      );
      const jobId = res.job_id;
      setWebsiteEmbedJobId(jobId);
      trackQueuedJob({
        job_id: jobId,
        status: "PENDING",
        progress_percentage: 0,
        file_name: urls[0],
        created_at: new Date().toISOString(),
      });
    } catch (err) {
      console.error("Failed to start website embedding:", err);
    }
  }, [websiteUrl, websiteRenderJavascript, trackQueuedJob]);

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

  const resetWebsiteState = () => {
    crawlSessionRef.current += 1;
    setWebsiteUrl("");
    setWebsiteDepth(1);
    setWebsiteSameDomainOnly(true);
    setWebsiteUrls([]);
    setWebsiteCrawlError(false);
    setWebsiteCrawling(false);
    setCrawlCurrentUrl(null);
    setCrawlFoundCount(0);
    setWebsiteEmbedJobId(null);
  };

  const handleCollectionSelect = (val: "codebase" | "general" | "website") => {
    setCollection(val);
    setCodeUploadMode("");
    if (val !== "codebase") { setRepoName(""); setRepoMode("existing"); }
    if (val !== "general" && val !== "website") { setGroupName(""); setGroupMode("existing"); }
    if (val !== "website") resetWebsiteState();
    setStep(2);
  };

  const handleStartAnother = () => {
    setCollection("");
    setGroupName("");
    setGroupMode("existing");
    setCodeUploadMode("");
    setRepoName("");
    setRepoMode("existing");
    resetWebsiteState();
    setStep(1);
  };

  const stepLabels = getStepLabels(collection);

  return (
    <div className="rounded-lg border border-border bg-card p-6">
      <PageHead
        title="Embed Files & Code Repos"
        description="Upload documents and code repositories for embedding and search."
      />
      <h1 className="text-2xl font-bold text-foreground mb-1">Upload &amp; Embed Files</h1>
      <p className="text-muted-foreground mb-6 text-sm">
        Use the steps below to select data type, configure scope, and upload files.
      </p>

      <StepIndicator current={step} total={3} labels={stepLabels} onStepClick={handleStepClick} />

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
            renderJavascript={websiteRenderJavascript}
            isCrawling={websiteCrawling}
            crawlCurrentUrl={crawlCurrentUrl}
            crawlFoundCount={crawlFoundCount}
            onUrlChange={setWebsiteUrl}
            onDepthChange={setWebsiteDepth}
            onSameDomainChange={setWebsiteSameDomainOnly}
            onRenderJavascriptChange={setWebsiteRenderJavascript}
            onCancelCrawl={handleCancelCrawl}
            onBack={handleWebsiteBack}
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
            embedJobId={websiteEmbedJobId}
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
