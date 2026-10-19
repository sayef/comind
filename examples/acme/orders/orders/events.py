import json
from dataclasses import asdict

import boto3
from acme_common import Event

sns = boto3.client("sns")
ORDER_EVENTS_TOPIC = "arn:aws:sns:eu-central-1:123456789012:order-events"


def _publish(event: Event) -> str:
    return sns.publish(TopicArn=ORDER_EVENTS_TOPIC, Message=json.dumps(asdict(event)))["MessageId"]


def order_created(order_id: str, total_cents: int, currency: str) -> str:
    return _publish(Event("order.created", {"order_id": order_id, "total_cents": total_cents, "currency": currency}))


def order_cancelled(order_id: str) -> str:
    return _publish(Event("order.cancelled", {"order_id": order_id}))
