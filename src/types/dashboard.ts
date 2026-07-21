export interface DashboardStats {
  repos: number;
  documents: number;
  servers: number;
  tools: number;
}

export const emptyDashboardStats: DashboardStats = {
  repos: 0,
  documents: 0,
  servers: 0,
  tools: 0,
};
