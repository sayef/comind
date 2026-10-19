from sqlalchemy import Column, Integer, String
from sqlalchemy.orm import declarative_base, sessionmaker

Base = declarative_base()
SessionLocal = sessionmaker()


class Payment(Base):
    __tablename__ = "payments"
    id = Column(String, primary_key=True)
    order_id = Column(String, nullable=False, unique=True)
    amount_cents = Column(Integer, nullable=False)
    currency = Column(String, nullable=False)
    provider_ref = Column(String)
