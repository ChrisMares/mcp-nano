import React from "react";
import { Link } from "react-router-dom";
import DataSelectList from "./DataSelectList";
import { wizardNav, btnPrimary, btnSecondary } from "@/styles/classes";

interface DataSelectStepProps {
  repoOptions: string[];
  groupOptions: string[];
  websiteOptions: string[];
  selectedRepos: Set<string>;
  selectedGroups: Set<string>;
  selectedWebsites: Set<string>;
  onToggleRepo: (repo: string) => void;
  onToggleGroup: (group: string) => void;
  onToggleWebsite: (website: string) => void;
  onSetRepos: (repos: Set<string>) => void;
  onSetGroups: (groups: Set<string>) => void;
  onSetWebsites: (websites: Set<string>) => void;
  onBack?: () => void;
  onNext: () => void;
  editMode?: boolean;
}

const DataSelectStep: React.FC<DataSelectStepProps> = ({
  repoOptions,
  groupOptions,
  websiteOptions,
  selectedRepos,
  selectedGroups,
  selectedWebsites,
  onToggleRepo,
  onToggleGroup,
  onToggleWebsite,
  onSetRepos,
  onSetGroups,
  onSetWebsites,
  onBack,
  onNext,
  editMode = false,
}) => {
  const hasNoData = repoOptions.length === 0 && groupOptions.length === 0 && websiteOptions.length === 0;
  const hasSelection = selectedRepos.size > 0 || selectedGroups.size > 0 || selectedWebsites.size > 0;

  return (
    <div>
      {!editMode && (
        <>
          <h2 className="text-lg font-semibold text-foreground mb-1">Select Data for Your Tool</h2>
          <p className="text-sm text-muted-foreground mb-5">
            Choose which embedded data this tool will be able to search. You can select code repositories, document groups, websites, or a combination.
          </p>
        </>
      )}

      {hasNoData ? (
        <div className="rounded-md border border-border bg-muted/30 p-6 text-center">
          <p className="text-sm text-muted-foreground mb-3">
            No embedded data found. You need to upload and embed files before creating a tool.
          </p>
          <Link
            to="/embed/upload"
            className="text-sm font-medium text-brand-cyan hover:underline"
          >
            Go to Upload Files &rarr;
          </Link>
        </div>
      ) : (
        <div className="space-y-6">
          <DataSelectList
            title="Add Code Repos"
            options={repoOptions}
            selected={selectedRepos}
            onToggle={onToggleRepo}
            onSetSelected={onSetRepos}
          />
          <DataSelectList
            title="Add Document Groups"
            options={groupOptions}
            selected={selectedGroups}
            onToggle={onToggleGroup}
            onSetSelected={onSetGroups}
          />
          <DataSelectList
            title="Add Websites"
            options={websiteOptions}
            selected={selectedWebsites}
            onToggle={onToggleWebsite}
            onSetSelected={onSetWebsites}
          />
        </div>
      )}

      {!editMode && (
        <div className={wizardNav}>
          {onBack ? (
            <button type="button" onClick={onBack} className={btnSecondary}>Back</button>
          ) : (
            <div />
          )}
          <button type="button" disabled={!hasSelection} onClick={onNext} className={btnPrimary}>Next</button>
        </div>
      )}
    </div>
  );
};

export default DataSelectStep;
