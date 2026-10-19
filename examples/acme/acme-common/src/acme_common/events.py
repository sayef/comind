import json
from dataclasses import asdict, dataclass, field
from datetime import datetime, timezone

import boto3

sns = boto3.client("sns")


@dataclass
class Event:
    type: str
    payload: dict
    occurred_at: str = field(default_factory=lambda: datetime.now(timezone.utc).isoformat())


def publish(topic_arn: str, event: Event) -> str:
    """Publish an event to an SNS topic; returns the message id."""
    resp = sns.publish(TopicArn=topic_arn, Message=json.dumps(asdict(event)))
    return resp["MessageId"]
