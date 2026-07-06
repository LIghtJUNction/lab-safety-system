from collections.abc import Generator

from fastapi.testclient import TestClient
from sqlalchemy import create_engine
from sqlalchemy.orm import Session, sessionmaker
from sqlalchemy.pool import StaticPool

from app.database import Base, get_db
from app.main import create_app


def build_client() -> TestClient:
    engine = create_engine("sqlite://", connect_args={"check_same_thread": False}, poolclass=StaticPool)
    TestingSessionLocal = sessionmaker(bind=engine, autoflush=False, autocommit=False)
    Base.metadata.create_all(bind=engine)
    app = create_app()

    def override_get_db() -> Generator[Session, None, None]:
        db = TestingSessionLocal()
        try:
            yield db
        finally:
            db.close()

    app.dependency_overrides[get_db] = override_get_db
    return TestClient(app)


def test_dashboard_stats_empty() -> None:
    client = build_client()
    response = client.get("/api/v1/analytics/dashboard")
    assert response.status_code == 200
    assert response.json()["regulation_count"] == 0


def test_create_and_query_core_records() -> None:
    client = build_client()
    user = client.post(
        "/api/v1/users",
        json={
            "username": "admin",
            "display_name": "安全管理员",
            "email": "admin@example.com",
            "role": "admin",
            "department": "安全办公室",
        },
    )
    assert user.status_code == 200

    regulation = client.post(
        "/api/v1/regulations",
        json={
            "title": "实验室安全管理条例",
            "regulation_type": "regulation",
            "issuing_authority": "安全办公室",
            "summary": "规范实验室准入、风险评估和应急处置。",
        },
    )
    assert regulation.status_code == 200

    incident = client.post(
        "/api/v1/incidents",
        json={
            "title": "危化品泄漏案例",
            "lab_name": "化学实验室 A",
            "occurred_on": "2026-01-10",
            "severity": "high",
            "category": "危化品",
            "root_cause": "试剂瓶标识不清且存放不当。",
            "corrective_actions": "完善标签、分区存储并补充培训。",
        },
    )
    assert incident.status_code == 200

    stats = client.get("/api/v1/analytics/dashboard").json()
    assert stats["regulation_count"] == 1
    assert stats["incident_count"] == 1
