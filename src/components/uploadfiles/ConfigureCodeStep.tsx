import React from "react";
import { Archive, FileCode, ArrowLeft, ArrowRight } from "lucide-react";
import NamePicker from "./NamePicker";
import {
  wizardCard, wizardCardSelected, wizardCardUnselected,
  wizardNav, btnPrimary, btnSecondary,
} from "@/styles/classes";

interface Props {
  codeUploadMode: "zip" | "individual" | "";
  repoName: string;
  repoMode: "existing" | "new";
  repoOptions: string[];
  onSelectZip: () => void;
  onSelectIndividual: () => void;
  onRepoChange: (value: string, mode: "existing" | "new") => void;
  onBack: () => void;
  onNext: () => void;
}

const ConfigureCodeStep: React.FC<Props> = ({
  codeUploadMode, repoName, repoMode, repoOptions,
  onSelectZip, onSelectIndividual, onRepoChange, onBack, onNext,
}) => (
  <div>
    <h2 className="text-lg font-semibold text-foreground mb-1">How are you uploading code?</h2>
    <p className="text-sm text-muted-foreground mb-5">
      Choose how you'd like to upload your code files.
    </p>

    <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
      <button
        type="button"
        onClick={onSelectZip}
        className={`${wizardCard} ${wizardCardUnselected} text-left flex flex-col`}
      >
        <div className="flex items-center gap-3 mb-3">
          <div className="p-2 rounded-lg bg-primary/15">
            <Archive size={22} className="text-primary" />
          </div>
          <span className="font-semibold text-foreground">Zip Repository</span>
        </div>
        <div className="flex-1 flex items-center">
          <p className="text-sm text-muted-foreground leading-relaxed">
            Upload a <code className="text-xs bg-muted px-1 py-0.5 rounded">.zip</code> of your repo.
            The zip filename becomes the repository name. Download directly from github by pressing the "Code" button and selecting "Download ZIP",
            or zip up your local project folders.
          </p>
        </div>
      </button>

      <button
        type="button"
        onClick={onSelectIndividual}
        className={`${wizardCard} ${codeUploadMode === "individual" ? wizardCardSelected : wizardCardUnselected} text-left flex flex-col`}
      >
        <div className="flex items-center gap-3 mb-3">
          <div className="p-2 rounded-lg bg-primary/15">
            <FileCode size={22} className="text-primary" />
          </div>
          <span className="font-semibold text-foreground">Individual Files</span>
        </div>
        <div className="flex-1 flex items-center">
          <p className="text-sm text-muted-foreground leading-relaxed">
            Upload individual files and assign them to a repository. PNG and JPEG files will be scraped of text and embedded. 
          </p>
        </div>
      </button>
    </div>

    {codeUploadMode === "individual" && (
      <div className="mt-5 p-4 rounded-lg bg-muted/40 border border-border">
        <NamePicker
          label="Repository Name"
          required
          options={repoOptions}
          value={repoName}
          mode={repoMode}
          onChange={onRepoChange}
          emptyMessage="No repositories uploaded yet. Enter a name for your new repository."
          emptyPlaceholder="Enter repository name"
          newPlaceholder="Enter new repository name"
          createLabel="-- Create new repo --"
        />
      </div>
    )}

    <div className={wizardNav}>
      <button onClick={onBack} className={btnSecondary}>
        <ArrowLeft size={16} /> Back
      </button>
      {codeUploadMode === "individual" && (
        <button
          disabled={!repoName.trim()}
          onClick={onNext}
          className={btnPrimary}
        >
          Next <ArrowRight size={16} />
        </button>
      )}
    </div>
  </div>
);

export default ConfigureCodeStep;
