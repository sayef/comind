import httpx

from profile_svc.config import settings


class SkillsClient:
    def __init__(self):
        self.http = httpx.Client(base_url=settings.skills_api_url)

    def fetch(self, skill_id: str):
        return self.http.get(f"/v1/skills/{skill_id}")

    def rename(self, skill_id: str, name: str):
        return self.http.post(f"/v1/skills/{skill_id}", json={"name": name})
