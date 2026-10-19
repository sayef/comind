resource "aws_sns_topic" "order_events" {
  name = "order-events"
}

resource "aws_sqs_queue" "order_payments" {
  name                       = "order-payments"
  visibility_timeout_seconds = 60
}

resource "aws_sns_topic_subscription" "payments" {
  topic_arn = aws_sns_topic.order_events.arn
  protocol  = "sqs"
  endpoint  = aws_sqs_queue.order_payments.arn
}

resource "aws_s3_bucket" "receipts" {
  bucket = "acme-payment-receipts"
}
