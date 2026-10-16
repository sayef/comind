import os

import boto3

sns = boto3.client("sns")
SKILL_TOPIC = os.environ["SKILL_EVENTS_TOPIC_ARN"]


def publish_skill_updated(skill_id: str):
    sns.publish(TopicArn=SKILL_TOPIC, Message=skill_id)
