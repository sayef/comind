import { cancelOrder, getOrder } from "./api";

export async function statusText(orderId: string): Promise<string> {
  const order = await getOrder(orderId);
  return order.status === "paid" ? "Paid, preparing shipment" : `Order ${order.status}`;
}

export async function cancel(orderId: string): Promise<boolean> {
  return (await cancelOrder(orderId)).status === "cancelled";
}
