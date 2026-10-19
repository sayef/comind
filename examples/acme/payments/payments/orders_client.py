import httpx
from acme_common.http import DEFAULT_TIMEOUT

from payments.config import ORDERS_URL


class OrdersClient:
    def __init__(self):
        self.http = httpx.Client(base_url=ORDERS_URL, timeout=DEFAULT_TIMEOUT)

    def get(self, order_id: str) -> dict:
        resp = self.http.get(f"/v1/orders/{order_id}")
        resp.raise_for_status()
        return resp.json()

    def mark_paid(self, order_id: str) -> None:
        self.http.post(f"/v1/orders/{order_id}/paid").raise_for_status()
