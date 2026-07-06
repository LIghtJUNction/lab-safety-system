FROM python:3.12-slim

LABEL org.opencontainers.image.title="lab-safety-system"
LABEL org.opencontainers.image.description="Backend image scaffold for the Laboratory Safety Management Information System"
LABEL org.opencontainers.image.source="https://github.com/LIghtJUNction/lab-safety-system"

WORKDIR /app

RUN groupadd --system app && useradd --system --gid app --create-home app

COPY pyproject.toml README.md LICENSE ./
RUN pip install --no-cache-dir .

COPY app ./app

ENV APP_ENV=production
ENV APP_HOST=0.0.0.0
ENV APP_PORT=8080

EXPOSE 8080

USER app

CMD ["uvicorn", "app.main:app", "--host", "0.0.0.0", "--port", "8080"]
