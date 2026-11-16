// Fixture data shared by the loader and the API route. Swap for a database or fetch().
export interface Project {
  id: string;
  name: string;
  owner: string;
  status: "Live" | "Building" | "Paused";
  visitors: number;
}

export const projects: Project[] = [
  { id: "atlas", name: "Atlas", owner: "Ada", status: "Live", visitors: 18240 },
  { id: "beacon", name: "Beacon", owner: "Grace", status: "Building", visitors: 0 },
  { id: "comet", name: "Comet", owner: "Linus", status: "Live", visitors: 5312 },
  { id: "drift", name: "Drift", owner: "Ada", status: "Paused", visitors: 947 },
];
