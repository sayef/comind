import { ServiceClient, services } from "./lib/http";

export async function loadJob(id: string) {
  return fetch(`/api/jobs/${id}`);
}

export async function loadSkill(id: string, api: ServiceClient = services.graph) {
  return api.get(`v1/skills/${id}`);
}
