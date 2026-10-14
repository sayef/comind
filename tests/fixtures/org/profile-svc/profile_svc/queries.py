from pypika import Query, Table


class QueryBase:
    def __init__(self):
        self.t_skill = Table("skills", schema="core")


class SkillQueries(QueryBase):
    def list_skills(self):
        return Query.from_(self.t_skill).select("*")

    def rename(self, skill_id, name):
        return Query.update(self.t_skill).set("name", name).where(self.t_skill.id == skill_id)
