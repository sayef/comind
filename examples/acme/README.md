# Acme: a small example company

Five tiny repositories that talk to each other the way real services do, used for comindly's demo and
screenshots. The code is illustrative and does not run as a product.

| Repository | Language | What it does |
|---|---|---|
| `storefront` | TypeScript | Cart and checkout; calls `orders` and `inventory` over HTTP |
| `orders` | Python (FastAPI) | Takes orders, reserves stock in `inventory`, publishes `order-events` |
| `payments` | Python | Consumes `order-payments` (subscribed to `order-events`), charges, calls back `orders` |
| `inventory` | Go | Stock levels and reservations |
| `acme-common` | Python package | Money, events and HTTP helpers; `payments` still pins an older version |

Index them with `comindly link examples/acme/*`, or run `scripts/screenshots.sh` to build the demo index
and the product screenshots.
