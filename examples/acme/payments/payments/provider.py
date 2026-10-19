import uuid

from acme_common import Money


class PaymentProvider:
    """The card processor. Charges are idempotent per order id."""

    def charge(self, order_id: str, amount: Money) -> str:
        if amount.cents <= 0:
            raise ValueError("nothing to charge")
        return f"ch_{uuid.uuid5(uuid.NAMESPACE_URL, order_id).hex[:16]}"
