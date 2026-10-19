import json

import boto3
from acme_common import Money

from payments.db import Payment, SessionLocal
from payments.orders_client import OrdersClient
from payments.provider import PaymentProvider

sqs = boto3.client("sqs")
s3 = boto3.client("s3")
QUEUE_URL = "https://sqs.eu-central-1.amazonaws.com/123456789012/order-payments"
RECEIPTS_BUCKET = "acme-payment-receipts"
orders = OrdersClient()
provider = PaymentProvider()


def handle_order_created(payload: dict) -> None:
    """Charge the order once, store the payment and its receipt, then tell orders."""
    order = orders.get(payload["order_id"])
    amount = Money(order["total_cents"], payload["currency"])
    ref = provider.charge(order["id"], amount)
    with SessionLocal() as session:
        session.add(Payment(id=ref, order_id=order["id"], amount_cents=amount.cents, currency=amount.currency, provider_ref=ref))
        session.commit()
    s3.put_object(Bucket=RECEIPTS_BUCKET, Key=f"{order['id']}.json", Body=json.dumps({"ref": ref, "amount": str(amount)}))
    orders.mark_paid(order["id"])


def poll() -> None:
    resp = sqs.receive_message(QueueUrl=QUEUE_URL, MaxNumberOfMessages=10, WaitTimeSeconds=20)
    for msg in resp.get("Messages", []):
        event = json.loads(json.loads(msg["Body"])["Message"])
        if event["type"] == "order.created":
            handle_order_created(event["payload"])
        sqs.delete_message(QueueUrl=QUEUE_URL, ReceiptHandle=msg["ReceiptHandle"])
