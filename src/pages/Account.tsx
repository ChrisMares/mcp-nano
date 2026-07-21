import React from "react";
import PageHead from "@/components/shared/PageHead";
import { card } from "@/styles/classes";

const Account: React.FC = () => {
  return (
    <div className={card}>
      <PageHead title="Account" description="Local profile" path="/account" />
      <h2 className="text-xl font-bold text-foreground mb-2">Local Mode</h2>
      <p className="text-sm text-muted-foreground">
        Authentication and billing are disabled in this simplified single-container build.
      </p>
    </div>
  );
};

export default Account;
