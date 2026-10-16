from sqlalchemy import Column, String
from sqlalchemy.orm import declarative_base, sessionmaker

Base = declarative_base()
SessionLocal = sessionmaker()


class Skill(Base):
    __tablename__ = "skills"
    id = Column(String, primary_key=True)
    name = Column(String)
