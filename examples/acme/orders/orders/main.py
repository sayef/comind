from fastapi import FastAPI

from orders.api import router

app = FastAPI(title="orders")
app.include_router(router)
