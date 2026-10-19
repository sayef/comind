import httpx

DEFAULT_TIMEOUT = httpx.Timeout(5.0, connect=2.0)


def service_client(base_url: str) -> httpx.Client:
    """An HTTP client for another Acme service, with retries and sane timeouts."""
    transport = httpx.HTTPTransport(retries=2)
    return httpx.Client(base_url=base_url, timeout=DEFAULT_TIMEOUT, transport=transport)
