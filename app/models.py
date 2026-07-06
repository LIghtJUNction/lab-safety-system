from datetime import date, datetime
from enum import StrEnum

from sqlalchemy import Date, DateTime, Enum, ForeignKey, Integer, String, Text
from sqlalchemy.orm import Mapped, mapped_column, relationship
from sqlalchemy.sql import func

from app.database import Base


class UserRole(StrEnum):
    admin = "admin"
    safety_officer = "safety_officer"
    lab_manager = "lab_manager"
    researcher = "researcher"


class RegulationType(StrEnum):
    law = "law"
    regulation = "regulation"
    policy = "policy"
    standard = "standard"


class IncidentSeverity(StrEnum):
    low = "low"
    medium = "medium"
    high = "high"
    critical = "critical"


class TrainingStatus(StrEnum):
    draft = "draft"
    active = "active"
    archived = "archived"


class ExamResultStatus(StrEnum):
    passed = "passed"
    failed = "failed"
    pending = "pending"


class EquipmentStatus(StrEnum):
    available = "available"
    reserved = "reserved"
    maintenance = "maintenance"
    retired = "retired"


class RepairStatus(StrEnum):
    open = "open"
    in_progress = "in_progress"
    resolved = "resolved"
    closed = "closed"


class TimestampMixin:
    created_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), server_default=func.now())
    updated_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), server_default=func.now(), onupdate=func.now())


class User(TimestampMixin, Base):
    __tablename__ = "users"

    id: Mapped[int] = mapped_column(primary_key=True)
    username: Mapped[str] = mapped_column(String(80), unique=True, index=True)
    display_name: Mapped[str] = mapped_column(String(120))
    email: Mapped[str] = mapped_column(String(240), unique=True, index=True)
    role: Mapped[UserRole] = mapped_column(Enum(UserRole), default=UserRole.researcher)
    auth_provider: Mapped[str] = mapped_column(String(40), default="password")
    department: Mapped[str | None] = mapped_column(String(160))
    password_hash: Mapped[str | None] = mapped_column(String(260))
    is_active: Mapped[bool] = mapped_column(default=True)


class Regulation(TimestampMixin, Base):
    __tablename__ = "regulations"

    id: Mapped[int] = mapped_column(primary_key=True)
    title: Mapped[str] = mapped_column(String(240), index=True)
    regulation_type: Mapped[RegulationType] = mapped_column(Enum(RegulationType), index=True)
    issuing_authority: Mapped[str] = mapped_column(String(180))
    effective_date: Mapped[date | None] = mapped_column(Date)
    summary: Mapped[str] = mapped_column(Text)
    file_url: Mapped[str | None] = mapped_column(String(500))


class IncidentCase(TimestampMixin, Base):
    __tablename__ = "incident_cases"

    id: Mapped[int] = mapped_column(primary_key=True)
    title: Mapped[str] = mapped_column(String(240), index=True)
    lab_name: Mapped[str] = mapped_column(String(180), index=True)
    occurred_on: Mapped[date] = mapped_column(Date)
    severity: Mapped[IncidentSeverity] = mapped_column(Enum(IncidentSeverity), index=True)
    category: Mapped[str] = mapped_column(String(120), index=True)
    root_cause: Mapped[str] = mapped_column(Text)
    corrective_actions: Mapped[str] = mapped_column(Text)


class Training(TimestampMixin, Base):
    __tablename__ = "trainings"

    id: Mapped[int] = mapped_column(primary_key=True)
    title: Mapped[str] = mapped_column(String(240), index=True)
    target_role: Mapped[str] = mapped_column(String(120))
    status: Mapped[TrainingStatus] = mapped_column(Enum(TrainingStatus), default=TrainingStatus.draft)
    starts_on: Mapped[date | None] = mapped_column(Date)
    exam_required_score: Mapped[int] = mapped_column(Integer, default=80)

    results: Mapped[list["ExamResult"]] = relationship(back_populates="training")


class ExamResult(TimestampMixin, Base):
    __tablename__ = "exam_results"

    id: Mapped[int] = mapped_column(primary_key=True)
    training_id: Mapped[int] = mapped_column(ForeignKey("trainings.id"))
    user_id: Mapped[int] = mapped_column(ForeignKey("users.id"))
    score: Mapped[int] = mapped_column(Integer)
    status: Mapped[ExamResultStatus] = mapped_column(Enum(ExamResultStatus), index=True)

    training: Mapped[Training] = relationship(back_populates="results")


class Equipment(TimestampMixin, Base):
    __tablename__ = "equipment"

    id: Mapped[int] = mapped_column(primary_key=True)
    asset_code: Mapped[str] = mapped_column(String(80), unique=True, index=True)
    name: Mapped[str] = mapped_column(String(200), index=True)
    lab_name: Mapped[str] = mapped_column(String(180), index=True)
    status: Mapped[EquipmentStatus] = mapped_column(Enum(EquipmentStatus), default=EquipmentStatus.available)
    owner: Mapped[str | None] = mapped_column(String(120))


class EquipmentBooking(TimestampMixin, Base):
    __tablename__ = "equipment_bookings"

    id: Mapped[int] = mapped_column(primary_key=True)
    equipment_id: Mapped[int] = mapped_column(ForeignKey("equipment.id"))
    user_id: Mapped[int] = mapped_column(ForeignKey("users.id"))
    starts_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), index=True)
    ends_at: Mapped[datetime] = mapped_column(DateTime(timezone=True), index=True)
    purpose: Mapped[str] = mapped_column(String(240))


class RepairTicket(TimestampMixin, Base):
    __tablename__ = "repair_tickets"

    id: Mapped[int] = mapped_column(primary_key=True)
    equipment_id: Mapped[int] = mapped_column(ForeignKey("equipment.id"))
    reported_by: Mapped[int] = mapped_column(ForeignKey("users.id"))
    description: Mapped[str] = mapped_column(Text)
    status: Mapped[RepairStatus] = mapped_column(Enum(RepairStatus), default=RepairStatus.open, index=True)
