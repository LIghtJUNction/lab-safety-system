FROM alpine:3.20

LABEL org.opencontainers.image.title="lab-safety-system"
LABEL org.opencontainers.image.description="Backend image scaffold for the Laboratory Safety Management Information System"
LABEL org.opencontainers.image.source="https://github.com/LIghtJUNction/lab-safety-system"

WORKDIR /app

RUN addgroup -S app && adduser -S app -G app

COPY README.md LICENSE ./

ENV APP_ENV=production
ENV APP_HOST=0.0.0.0
ENV APP_PORT=8080

EXPOSE 8080

USER app

CMD ["sh", "-c", "echo 'lab-safety-system backend image scaffold'; echo 'No backend runtime has been implemented yet.'; sleep infinity"]
