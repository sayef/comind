from pydantic_settings import BaseSettings


class Settings(BaseSettings):
    skills_api_url: str = "http://skills-svc"
    exports_bucket: str = "acme-profile-exports"


settings = Settings()
