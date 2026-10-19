import { placeOrder, stockFor, type OrderLine } from "./api";

export class Cart {
  private lines = new Map<string, OrderLine>();

  add(sku: string, price_cents: number, quantity = 1): void {
    const line = this.lines.get(sku) ?? { sku, price_cents, quantity: 0 };
    line.quantity += quantity;
    this.lines.set(sku, line);
  }

  totalCents(): number {
    return [...this.lines.values()].reduce((sum, l) => sum + l.price_cents * l.quantity, 0);
  }

  /** Checks stock for every line first so the customer sees problems before paying. */
  async checkout(customerId: string): Promise<string> {
    for (const line of this.lines.values()) {
      if ((await stockFor(line.sku)) < line.quantity) throw new Error(`Only a few left of ${line.sku}`);
    }
    const order = await placeOrder(customerId, [...this.lines.values()]);
    this.lines.clear();
    return order.id;
  }
}
