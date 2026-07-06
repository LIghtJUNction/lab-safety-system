from typing import TypeVar

from pathlib import Path
from uuid import uuid4

from fastapi import APIRouter, Depends, File, HTTPException, Query, UploadFile
from sqlalchemy import and_, func, or_, select, text
from sqlalchemy.orm import Session

from app.database import get_db
from app.models import (
    Equipment,
    EquipmentBooking,
    ExamResult,
    ExamResultStatus,
    IncidentCase,
    Regulation,
    RepairStatus,
    RepairTicket,
    Training,
    User,
)
from app.schemas import (
    AuthMethods,
    AuthToken,
    CountBucket,
    DashboardStats,
    EquipmentBookingCreate,
    EquipmentBookingRead,
    EquipmentCreate,
    EquipmentRead,
    ExamResultCreate,
    ExamResultRead,
    IncidentCaseCreate,
    IncidentCaseRead,
    IncidentAnalytics,
    PasswordLogin,
    RegulationCreate,
    RegulationRead,
    RepairTicketCreate,
    RepairTicketRead,
    RepairTicketUpdate,
    TrainingCreate,
    TrainingRead,
    TrainingResultSummary,
    UserCreate,
    UserRead,
    UploadedFile,
)
from app.config import settings
from app.security import create_access_token, hash_password, verify_password

router = APIRouter(prefix="/api/v1")
ModelT = TypeVar("ModelT")


def add_record(db: Session, record: ModelT) -> ModelT:
    db.add(record)
    db.commit()
    db.refresh(record)
    return record


def page_bounds(limit: int, offset: int) -> tuple[int, int]:
    return min(max(limit, 1), 100), max(offset, 0)


@router.get("/health")
def health() -> dict[str, str]:
    return {"status": "ok"}


@router.get("/ready")
def ready(db: Session = Depends(get_db)) -> dict[str, str]:
    db.execute(text("select 1"))
    return {"status": "ready"}


@router.get("/auth/methods", response_model=AuthMethods)
def auth_methods() -> AuthMethods:
    return AuthMethods(sso=settings.sso_enabled, oauth=settings.oauth_enabled)


@router.post("/auth/password-login", response_model=AuthToken)
def password_login(payload: PasswordLogin, db: Session = Depends(get_db)) -> AuthToken:
    user = db.scalar(select(User).where(User.username == payload.username))
    if not user or not user.is_active or not verify_password(payload.password, user.password_hash):
        raise HTTPException(status_code=401, detail="Invalid username or password")
    return AuthToken(access_token=create_access_token(user.username), expires_in=settings.token_ttl_seconds)


@router.post("/users", response_model=UserRead)
def create_user(payload: UserCreate, db: Session = Depends(get_db)) -> User:
    data = payload.model_dump(exclude={"password"})
    if payload.password:
        data["password_hash"] = hash_password(payload.password)
    return add_record(db, User(**data))


@router.get("/users", response_model=list[UserRead])
def list_users(
    q: str | None = Query(default=None),
    role: str | None = Query(default=None),
    limit: int = Query(default=50),
    offset: int = Query(default=0),
    db: Session = Depends(get_db),
) -> list[User]:
    limit, offset = page_bounds(limit, offset)
    stmt = select(User).order_by(User.created_at.desc()).limit(limit).offset(offset)
    if q:
        stmt = stmt.where(or_(User.username.ilike(f"%{q}%"), User.display_name.ilike(f"%{q}%"), User.email.ilike(f"%{q}%")))
    if role:
        stmt = stmt.where(User.role == role)
    return list(db.scalars(stmt))


@router.post("/regulations", response_model=RegulationRead)
def create_regulation(payload: RegulationCreate, db: Session = Depends(get_db)) -> Regulation:
    return add_record(db, Regulation(**payload.model_dump()))


@router.get("/regulations", response_model=list[RegulationRead])
def list_regulations(
    q: str | None = Query(default=None),
    regulation_type: str | None = Query(default=None),
    limit: int = Query(default=50),
    offset: int = Query(default=0),
    db: Session = Depends(get_db),
) -> list[Regulation]:
    limit, offset = page_bounds(limit, offset)
    stmt = select(Regulation).order_by(Regulation.created_at.desc()).limit(limit).offset(offset)
    if q:
        stmt = stmt.where(Regulation.title.ilike(f"%{q}%"))
    if regulation_type:
        stmt = stmt.where(Regulation.regulation_type == regulation_type)
    return list(db.scalars(stmt))


@router.post("/regulations/upload", response_model=UploadedFile)
async def upload_regulation_file(file: UploadFile = File(...)) -> UploadedFile:
    return await save_upload(file, "regulations")


@router.post("/incidents", response_model=IncidentCaseRead)
def create_incident(payload: IncidentCaseCreate, db: Session = Depends(get_db)) -> IncidentCase:
    return add_record(db, IncidentCase(**payload.model_dump()))


@router.get("/incidents", response_model=list[IncidentCaseRead])
def list_incidents(
    q: str | None = Query(default=None),
    severity: str | None = Query(default=None),
    category: str | None = Query(default=None),
    limit: int = Query(default=50),
    offset: int = Query(default=0),
    db: Session = Depends(get_db),
) -> list[IncidentCase]:
    limit, offset = page_bounds(limit, offset)
    stmt = select(IncidentCase).order_by(IncidentCase.occurred_on.desc()).limit(limit).offset(offset)
    if q:
        stmt = stmt.where(IncidentCase.title.ilike(f"%{q}%"))
    if severity:
        stmt = stmt.where(IncidentCase.severity == severity)
    if category:
        stmt = stmt.where(IncidentCase.category == category)
    return list(db.scalars(stmt))


@router.post("/incidents/upload", response_model=UploadedFile)
async def upload_incident_file(file: UploadFile = File(...)) -> UploadedFile:
    return await save_upload(file, "incidents")


@router.post("/trainings", response_model=TrainingRead)
def create_training(payload: TrainingCreate, db: Session = Depends(get_db)) -> Training:
    return add_record(db, Training(**payload.model_dump()))


@router.get("/trainings", response_model=list[TrainingRead])
def list_trainings(
    status: str | None = Query(default=None),
    limit: int = Query(default=50),
    offset: int = Query(default=0),
    db: Session = Depends(get_db),
) -> list[Training]:
    limit, offset = page_bounds(limit, offset)
    stmt = select(Training).order_by(Training.created_at.desc()).limit(limit).offset(offset)
    if status:
        stmt = stmt.where(Training.status == status)
    return list(db.scalars(stmt))


@router.post("/exam-results", response_model=ExamResultRead)
def create_exam_result(payload: ExamResultCreate, db: Session = Depends(get_db)) -> ExamResult:
    return add_record(db, ExamResult(**payload.model_dump()))


@router.get("/exam-results", response_model=list[ExamResultRead])
def list_exam_results(db: Session = Depends(get_db)) -> list[ExamResult]:
    return list(db.scalars(select(ExamResult).order_by(ExamResult.created_at.desc())))


@router.get("/trainings/results-summary", response_model=list[TrainingResultSummary])
def training_results_summary(db: Session = Depends(get_db)) -> list[TrainingResultSummary]:
    trainings = db.scalars(select(Training).order_by(Training.created_at.desc())).all()
    summaries: list[TrainingResultSummary] = []
    for training in trainings:
        counts = dict(
            db.execute(
                select(ExamResult.status, func.count())
                .where(ExamResult.training_id == training.id)
                .group_by(ExamResult.status)
            ).all()
        )
        passed = counts.get(ExamResultStatus.passed, 0)
        failed = counts.get(ExamResultStatus.failed, 0)
        pending = counts.get(ExamResultStatus.pending, 0)
        total = passed + failed + pending
        summaries.append(
            TrainingResultSummary(
                training_id=training.id,
                title=training.title,
                passed=passed,
                failed=failed,
                pending=pending,
                pass_rate=round(passed / total, 4) if total else 0.0,
            )
        )
    return summaries


@router.post("/equipment", response_model=EquipmentRead)
def create_equipment(payload: EquipmentCreate, db: Session = Depends(get_db)) -> Equipment:
    return add_record(db, Equipment(**payload.model_dump()))


@router.get("/equipment", response_model=list[EquipmentRead])
def list_equipment(
    q: str | None = Query(default=None),
    status: str | None = Query(default=None),
    lab_name: str | None = Query(default=None),
    limit: int = Query(default=50),
    offset: int = Query(default=0),
    db: Session = Depends(get_db),
) -> list[Equipment]:
    limit, offset = page_bounds(limit, offset)
    stmt = select(Equipment).order_by(Equipment.created_at.desc()).limit(limit).offset(offset)
    if q:
        stmt = stmt.where(or_(Equipment.name.ilike(f"%{q}%"), Equipment.asset_code.ilike(f"%{q}%")))
    if status:
        stmt = stmt.where(Equipment.status == status)
    if lab_name:
        stmt = stmt.where(Equipment.lab_name.ilike(f"%{lab_name}%"))
    return list(db.scalars(stmt))


@router.post("/equipment-bookings", response_model=EquipmentBookingRead)
def create_equipment_booking(payload: EquipmentBookingCreate, db: Session = Depends(get_db)) -> EquipmentBooking:
    if payload.ends_at <= payload.starts_at:
        raise HTTPException(status_code=400, detail="Booking end time must be later than start time")
    conflict = db.scalar(
        select(EquipmentBooking).where(
            and_(
                EquipmentBooking.equipment_id == payload.equipment_id,
                EquipmentBooking.starts_at < payload.ends_at,
                EquipmentBooking.ends_at > payload.starts_at,
            )
        )
    )
    if conflict:
        raise HTTPException(status_code=409, detail="Equipment is already booked for the selected time range")
    return add_record(db, EquipmentBooking(**payload.model_dump()))


@router.get("/equipment-bookings", response_model=list[EquipmentBookingRead])
def list_equipment_bookings(db: Session = Depends(get_db)) -> list[EquipmentBooking]:
    return list(db.scalars(select(EquipmentBooking).order_by(EquipmentBooking.starts_at.desc())))


@router.post("/repair-tickets", response_model=RepairTicketRead)
def create_repair_ticket(payload: RepairTicketCreate, db: Session = Depends(get_db)) -> RepairTicket:
    return add_record(db, RepairTicket(**payload.model_dump()))


@router.get("/repair-tickets", response_model=list[RepairTicketRead])
def list_repair_tickets(db: Session = Depends(get_db)) -> list[RepairTicket]:
    return list(db.scalars(select(RepairTicket).order_by(RepairTicket.created_at.desc())))


@router.patch("/repair-tickets/{ticket_id}", response_model=RepairTicketRead)
def update_repair_ticket(ticket_id: int, payload: RepairTicketUpdate, db: Session = Depends(get_db)) -> RepairTicket:
    ticket = db.get(RepairTicket, ticket_id)
    if not ticket:
        raise HTTPException(status_code=404, detail="Repair ticket not found")
    ticket.status = payload.status
    db.commit()
    db.refresh(ticket)
    return ticket


@router.get("/analytics/incidents", response_model=IncidentAnalytics)
def incident_analytics(db: Session = Depends(get_db)) -> IncidentAnalytics:
    by_category = [
        CountBucket(name=name, count=count)
        for name, count in db.execute(
            select(IncidentCase.category, func.count()).group_by(IncidentCase.category).order_by(func.count().desc())
        )
    ]
    by_severity = [
        CountBucket(name=str(name), count=count)
        for name, count in db.execute(
            select(IncidentCase.severity, func.count()).group_by(IncidentCase.severity).order_by(func.count().desc())
        )
    ]
    return IncidentAnalytics(by_category=by_category, by_severity=by_severity)


@router.get("/analytics/dashboard", response_model=DashboardStats)
def dashboard_stats(db: Session = Depends(get_db)) -> DashboardStats:
    total_results = db.scalar(select(func.count()).select_from(ExamResult)) or 0
    passed_results = db.scalar(select(func.count()).select_from(ExamResult).where(ExamResult.status == ExamResultStatus.passed)) or 0
    return DashboardStats(
        regulation_count=db.scalar(select(func.count()).select_from(Regulation)) or 0,
        incident_count=db.scalar(select(func.count()).select_from(IncidentCase)) or 0,
        training_count=db.scalar(select(func.count()).select_from(Training)) or 0,
        equipment_count=db.scalar(select(func.count()).select_from(Equipment)) or 0,
        open_repair_count=db.scalar(select(func.count()).select_from(RepairTicket).where(RepairTicket.status == RepairStatus.open)) or 0,
        exam_pass_rate=round(passed_results / total_results, 4) if total_results else 0.0,
    )


async def save_upload(file: UploadFile, category: str) -> UploadedFile:
    content = await file.read()
    safe_name = Path(file.filename or "upload.bin").name
    stored_name = f"{uuid4().hex}-{safe_name}"
    target_dir = Path(settings.upload_dir) / category
    target_dir.mkdir(parents=True, exist_ok=True)
    target = target_dir / stored_name
    target.write_bytes(content)
    return UploadedFile(
        filename=safe_name,
        content_type=file.content_type,
        size=len(content),
        url=f"/uploads/{category}/{stored_name}",
    )
