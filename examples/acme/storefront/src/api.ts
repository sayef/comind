export interface OrderLine {
  sku: string;
  quantity: number;
  price_cents: number;
}

export interface Order {
  id: string;
  status: string;
  total_cents: number;
}

const ORDERS = "/v1/orders";

async function json<T>(res: Response): Promise<T> {
  if (!res.ok) throw new Error(`${res.status} ${res.statusText}`);
  return res.json() as Promise<T>;
}

export async function placeOrder(customerId: string, lines: OrderLine[]): Promise<{ id: string; total: string }> {
  const res = await fetch(ORDERS, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ customer_id: customerId, lines }),
  });
  return json(res);
}

export async function getOrder(id: string): Promise<Order> {
  return json(await fetch(`/v1/orders/${id}`));
}

export async function cancelOrder(id: string): Promise<Order> {
  return json(await fetch(`/v1/orders/${id}/cancel`, { method: "POST" }));
}

export async function stockFor(sku: string): Promise<number> {
  const body = await json<{ available: number }>(await fetch(`/api/stock/${sku}`));
  return body.available;
}
