from pydantic_settings import BaseSettings, SettingsConfigDict


class Settings(BaseSettings):
    app_env: str = "production"
    app_host: str = "0.0.0.0"
    app_port: int = 8080
    database_url: str = "postgresql+psycopg://lab_safety:change-me@postgres:5432/lab_safety"
    cors_origins: str = "*"
    secret_key: str = "change-me-in-production"
    token_ttl_seconds: int = 3600
    upload_dir: str = "uploads"
    sso_enabled: bool = False
    oauth_enabled: bool = False

    model_config = SettingsConfigDict(env_file=".env", extra="ignore")


settings = Settings()
