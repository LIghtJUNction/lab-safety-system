from datetime import date, datetime

from pydantic import BaseModel, ConfigDict, EmailStr

from app.models import (
    EquipmentStatus,
    ExamResultStatus,
    IncidentSeverity,
    RegulationType,
    RepairStatus,
    TrainingStatus,
    UserRole,
)


class UserCreate(BaseModel):
    username: str
    display_name: str
    email: EmailStr
    role: UserRole = UserRole.researcher
    auth_provider: str = "password"
    department: str | None = None


class UserRead(UserCreate):
    model_config = ConfigDict(from_attributes=True)
    id: int
    created_at: datetime


class RegulationCreate(BaseModel):
    title: str
    regulation_type: RegulationType
    issuing_authority: str
    effective_date: date | None = None
    summary: str
    file_url: str | None = None


class RegulationRead(RegulationCreate):
    model_config = ConfigDict(from_attributes=True)
    id: int
    created_at: datetime


class IncidentCaseCreate(BaseModel):
    title: str
    lab_name: str
    occurred_on: date
    severity: IncidentSeverity
    category: str
    root_cause: str
    corrective_actions: str


class IncidentCaseRead(IncidentCaseCreate):
    model_config = ConfigDict(from_attributes=True)
    id: int
    created_at: datetime


class TrainingCreate(BaseModel):
    title: str
    target_role: str
    status: TrainingStatus = TrainingStatus.draft
    starts_on: date | None = None
    exam_required_score: int = 80


class TrainingRead(TrainingCreate):
    model_config = ConfigDict(from_attributes=True)
    id: int
    created_at: datetime


class ExamResultCreate(BaseModel):
    training_id: int
    user_id: int
    score: int
    status: ExamResultStatus


class ExamResultRead(ExamResultCreate):
    model_config = ConfigDict(from_attributes=True)
    id: int
    created_at: datetime


class EquipmentCreate(BaseModel):
    asset_code: str
    name: str
    lab_name: str
    status: EquipmentStatus = EquipmentStatus.available
    owner: str | None = None


class EquipmentRead(EquipmentCreate):
    model_config = ConfigDict(from_attributes=True)
    id: int
    created_at: datetime


class EquipmentBookingCreate(BaseModel):
    equipment_id: int
    user_id: int
    starts_at: datetime
    ends_at: datetime
    purpose: str


class EquipmentBookingRead(EquipmentBookingCreate):
    model_config = ConfigDict(from_attributes=True)
    id: int
    created_at: datetime


class RepairTicketCreate(BaseModel):
    equipment_id: int
    reported_by: int
    description: str
    status: RepairStatus = RepairStatus.open


class RepairTicketRead(RepairTicketCreate):
    model_config = ConfigDict(from_attributes=True)
    id: int
    created_at: datetime


class DashboardStats(BaseModel):
    regulation_count: int
    incident_count: int
    training_count: int
    equipment_count: int
    open_repair_count: int
    exam_pass_rate: float

