from dataclasses import dataclass
from decimal import ROUND_HALF_EVEN, Decimal


@dataclass(frozen=True)
class Money:
    """An amount in minor units with its ISO 4217 currency."""

    cents: int
    currency: str = "EUR"

    @classmethod
    def from_decimal(cls, amount: Decimal, currency: str = "EUR") -> "Money":
        cents = (amount * 100).quantize(Decimal("1"), rounding=ROUND_HALF_EVEN)
        return cls(int(cents), currency)

    def add(self, other: "Money") -> "Money":
        if other.currency != self.currency:
            raise ValueError("currency mismatch")
        return Money(self.cents + other.cents, self.currency)

    def times(self, quantity: int) -> "Money":
        return Money(self.cents * quantity, self.currency)

    def __str__(self) -> str:
        return f"{self.cents / 100:.2f} {self.currency}"
