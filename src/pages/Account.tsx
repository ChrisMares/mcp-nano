import React from "react";
import PageHead from "@/components/shared/PageHead";
import { card } from "@/styles/classes";

const Account: React.FC = () => {
  return (
    <div className={card}>
      <PageHead title="Account" description="Local profile" path="/account" />
      <h2 className="text-xl font-bold text-foreground mb-2">Local Mode</h2>
      <p className="text-sm text-muted-foreground">
        This app runs entirely on your machine. There is no account or sign-in.
      </p>
    </div>
  );
};

export default Account;
