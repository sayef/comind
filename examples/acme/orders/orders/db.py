from sqlalchemy import Column, ForeignKey, Integer, String
from sqlalchemy.orm import declarative_base, relationship, sessionmaker

Base = declarative_base()
SessionLocal = sessionmaker()


class Order(Base):
    __tablename__ = "orders"
    id = Column(String, primary_key=True)
    customer_id = Column(String, nullable=False)
    status = Column(String, default="pending")
    total_cents = Column(Integer, nullable=False)
    currency = Column(String, default="EUR")
    lines = relationship("OrderLine", back_populates="order")


class OrderLine(Base):
    __tablename__ = "order_lines"
    id = Column(Integer, primary_key=True)
    order_id = Column(String, ForeignKey("orders.id"))
    sku = Column(String, nullable=False)
    quantity = Column(Integer, nullable=False)
    price_cents = Column(Integer, nullable=False)
    order = relationship("Order", back_populates="lines")
