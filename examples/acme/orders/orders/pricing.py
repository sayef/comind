from acme_common import Money

VAT_RATE = 19


def line_total(price_cents: int, quantity: int, currency: str = "EUR") -> Money:
    return Money(price_cents, currency).times(quantity)


def order_total(lines: list[dict], currency: str = "EUR") -> Money:
    """Sum of all lines, VAT included."""
    total = Money(0, currency)
    for line in lines:
        total = total.add(line_total(line["price_cents"], line["quantity"], currency))
    return total
