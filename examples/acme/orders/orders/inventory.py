import httpx
from acme_common.http import DEFAULT_TIMEOUT

from orders.config import INVENTORY_URL


class InventoryClient:
    """Stock checks and reservations against the inventory service."""

    def __init__(self):
        self.http = httpx.Client(base_url=INVENTORY_URL, timeout=DEFAULT_TIMEOUT)

    def available(self, sku: str) -> int:
        resp = self.http.get(f"/api/stock/{sku}")
        resp.raise_for_status()
        return resp.json()["available"]

    def reserve(self, sku: str, quantity: int) -> None:
        resp = self.http.post(f"/api/stock/{sku}/reservations", json={"quantity": quantity})
        resp.raise_for_status()
