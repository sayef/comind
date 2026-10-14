from fastapi import APIRouter

from skills_svc.db import SessionLocal, Skill
from skills_svc.events import publish_skill_updated

router = APIRouter(prefix="/skills")


@router.get("/{skill_id}")
def get_skill(skill_id: str):
    with SessionLocal() as session:
        return session.get(Skill, skill_id)


@router.post("/{skill_id}")
def update_skill(skill_id: str, body: dict):
    with SessionLocal() as session:
        session.merge(Skill(id=skill_id, **body))
        session.commit()
    publish_skill_updated(skill_id)
