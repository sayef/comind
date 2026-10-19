import uuid

from fastapi import APIRouter, HTTPException

from orders.db import Order, OrderLine, SessionLocal
from orders.events import order_cancelled, order_created
from orders.inventory import InventoryClient
from orders.pricing import order_total

router = APIRouter(prefix="/v1/orders")
inventory = InventoryClient()


@router.post("")
def create_order(body: dict):
    """Reserve stock for every line, store the order and announce it."""
    for line in body["lines"]:
        if inventory.available(line["sku"]) < line["quantity"]:
            raise HTTPException(409, f"out of stock: {line['sku']}")
    for line in body["lines"]:
        inventory.reserve(line["sku"], line["quantity"])
    total = order_total(body["lines"])
    order = Order(id=str(uuid.uuid4()), customer_id=body["customer_id"], total_cents=total.cents)
    with SessionLocal() as session:
        session.add(order)
        session.add_all(OrderLine(order_id=order.id, **line) for line in body["lines"])
        session.commit()
    order_created(order.id, total.cents, total.currency)
    return {"id": order.id, "total": str(total)}


@router.get("/{order_id}")
def get_order(order_id: str):
    with SessionLocal() as session:
        order = session.get(Order, order_id)
        if order is None:
            raise HTTPException(404)
        return {"id": order.id, "status": order.status, "total_cents": order.total_cents}


@router.post("/{order_id}/cancel")
def cancel_order(order_id: str):
    with SessionLocal() as session:
        order = session.get(Order, order_id)
        order.status = "cancelled"
        session.commit()
    order_cancelled(order_id)
    return {"id": order_id, "status": "cancelled"}


@router.post("/{order_id}/paid")
def mark_paid(order_id: str):
    with SessionLocal() as session:
        session.get(Order, order_id).status = "paid"
        session.commit()
    return {"id": order_id, "status": "paid"}
