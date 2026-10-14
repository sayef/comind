from fastapi import FastAPI

from skills_svc import api

app = FastAPI()
app.include_router(api.router, prefix="/v1")
