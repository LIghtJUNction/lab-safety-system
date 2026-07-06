from typing import TypeVar

from fastapi import APIRouter, Depends, Query
from sqlalchemy import func, select
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
    DashboardStats,
    EquipmentBookingCreate,
    EquipmentBookingRead,
    EquipmentCreate,
    EquipmentRead,
    ExamResultCreate,
    ExamResultRead,
    IncidentCaseCreate,
    IncidentCaseRead,
    RegulationCreate,
    RegulationRead,
    RepairTicketCreate,
    RepairTicketRead,
    TrainingCreate,
    TrainingRead,
    UserCreate,
    UserRead,
)

router = APIRouter(prefix="/api/v1")
ModelT = TypeVar("ModelT")


def add_record(db: Session, record: ModelT) -> ModelT:
    db.add(record)
    db.commit()
    db.refresh(record)
    return record


@router.get("/health")
def health() -> dict[str, str]:
    return {"status": "ok"}


@router.post("/users", response_model=UserRead)
def create_user(payload: UserCreate, db: Session = Depends(get_db)) -> User:
    return add_record(db, User(**payload.model_dump()))


@router.get("/users", response_model=list[UserRead])
def list_users(db: Session = Depends(get_db)) -> list[User]:
    return list(db.scalars(select(User).order_by(User.created_at.desc())))


@router.post("/regulations", response_model=RegulationRead)
def create_regulation(payload: RegulationCreate, db: Session = Depends(get_db)) -> Regulation:
    return add_record(db, Regulation(**payload.model_dump()))


@router.get("/regulations", response_model=list[RegulationRead])
def list_regulations(q: str | None = Query(default=None), db: Session = Depends(get_db)) -> list[Regulation]:
    stmt = select(Regulation).order_by(Regulation.created_at.desc())
    if q:
        stmt = stmt.where(Regulation.title.ilike(f"%{q}%"))
    return list(db.scalars(stmt))


@router.post("/incidents", response_model=IncidentCaseRead)
def create_incident(payload: IncidentCaseCreate, db: Session = Depends(get_db)) -> IncidentCase:
    return add_record(db, IncidentCase(**payload.model_dump()))


@router.get("/incidents", response_model=list[IncidentCaseRead])
def list_incidents(q: str | None = Query(default=None), db: Session = Depends(get_db)) -> list[IncidentCase]:
    stmt = select(IncidentCase).order_by(IncidentCase.occurred_on.desc())
    if q:
        stmt = stmt.where(IncidentCase.title.ilike(f"%{q}%"))
    return list(db.scalars(stmt))


@router.post("/trainings", response_model=TrainingRead)
def create_training(payload: TrainingCreate, db: Session = Depends(get_db)) -> Training:
    return add_record(db, Training(**payload.model_dump()))


@router.get("/trainings", response_model=list[TrainingRead])
def list_trainings(db: Session = Depends(get_db)) -> list[Training]:
    return list(db.scalars(select(Training).order_by(Training.created_at.desc())))


@router.post("/exam-results", response_model=ExamResultRead)
def create_exam_result(payload: ExamResultCreate, db: Session = Depends(get_db)) -> ExamResult:
    return add_record(db, ExamResult(**payload.model_dump()))


@router.get("/exam-results", response_model=list[ExamResultRead])
def list_exam_results(db: Session = Depends(get_db)) -> list[ExamResult]:
    return list(db.scalars(select(ExamResult).order_by(ExamResult.created_at.desc())))


@router.post("/equipment", response_model=EquipmentRead)
def create_equipment(payload: EquipmentCreate, db: Session = Depends(get_db)) -> Equipment:
    return add_record(db, Equipment(**payload.model_dump()))


@router.get("/equipment", response_model=list[EquipmentRead])
def list_equipment(q: str | None = Query(default=None), db: Session = Depends(get_db)) -> list[Equipment]:
    stmt = select(Equipment).order_by(Equipment.created_at.desc())
    if q:
        stmt = stmt.where(Equipment.name.ilike(f"%{q}%"))
    return list(db.scalars(stmt))


@router.post("/equipment-bookings", response_model=EquipmentBookingRead)
def create_equipment_booking(payload: EquipmentBookingCreate, db: Session = Depends(get_db)) -> EquipmentBooking:
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

