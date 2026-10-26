import { Outlet } from "react-router-dom";
import AppHeader from "./AppHeader";
import BackendStatusBanner from "./BackendStatusBanner";
import AppSidebar from "./AppSidebar";

const AppLayout: React.FC = () => {
  return (
    <div className="flex h-dvh overflow-hidden">
      <AppSidebar />
      <div className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">
        <AppHeader />
        <BackendStatusBanner />
        <main className="min-h-0 flex-1 overflow-x-hidden overflow-y-auto overscroll-contain">
          <div className="p-4 mx-auto max-w-(--breakpoint-2xl)">
            <Outlet />
          </div>
        </main>
      </div>
    </div>
  );
};

export default AppLayout;
