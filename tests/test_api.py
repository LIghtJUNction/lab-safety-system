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
    ready = client.get("/api/v1/ready")
    assert ready.status_code == 200


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
            "password": "ChangeMe123!",
        },
    )
    assert user.status_code == 200
    login = client.post("/api/v1/auth/password-login", json={"username": "admin", "password": "ChangeMe123!"})
    assert login.status_code == 200
    assert login.json()["token_type"] == "bearer"

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

    analytics = client.get("/api/v1/analytics/incidents").json()
    assert analytics["by_category"][0]["name"] == "危化品"


def test_equipment_booking_conflict_and_repair_update() -> None:
    client = build_client()
    user = client.post(
        "/api/v1/users",
        json={
            "username": "lab-user",
            "display_name": "实验员",
            "email": "user@example.com",
            "role": "researcher",
        },
    ).json()
    equipment = client.post(
        "/api/v1/equipment",
        json={"asset_code": "EQ-001", "name": "气相色谱仪", "lab_name": "分析测试中心"},
    ).json()

    booking_payload = {
        "equipment_id": equipment["id"],
        "user_id": user["id"],
        "starts_at": "2026-07-08T09:00:00Z",
        "ends_at": "2026-07-08T11:00:00Z",
        "purpose": "样品分析",
    }
    assert client.post("/api/v1/equipment-bookings", json=booking_payload).status_code == 200
    assert client.post("/api/v1/equipment-bookings", json=booking_payload).status_code == 409

    ticket = client.post(
        "/api/v1/repair-tickets",
        json={"equipment_id": equipment["id"], "reported_by": user["id"], "description": "风扇异响"},
    ).json()
    updated = client.patch(f"/api/v1/repair-tickets/{ticket['id']}", json={"status": "resolved"})
    assert updated.status_code == 200
    assert updated.json()["status"] == "resolved"


def test_upload_regulation_file(tmp_path, monkeypatch) -> None:
    monkeypatch.setattr("app.config.settings.upload_dir", str(tmp_path))
    monkeypatch.setattr("app.api.settings.upload_dir", str(tmp_path))
    client = build_client()
    response = client.post(
        "/api/v1/regulations/upload",
        files={"file": ("rule.txt", b"safe lab rule", "text/plain")},
    )
    assert response.status_code == 200
    assert response.json()["size"] == len(b"safe lab rule")
