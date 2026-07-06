use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Serialize, FromRow)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub display_name: String,
    pub email: String,
    pub role: String,
    pub auth_provider: String,
    pub department: Option<String>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct UserCreate {
    pub username: String,
    pub display_name: String,
    pub email: String,
    pub role: Option<String>,
    pub auth_provider: Option<String>,
    pub department: Option<String>,
    pub password: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PasswordLogin {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct AuthToken {
    pub access_token: String,
    pub token_type: &'static str,
    pub expires_in: i64,
    pub user: AuthUser,
}

#[derive(Debug, Serialize)]
pub struct AuthUser {
    pub id: i64,
    pub username: String,
    pub display_name: String,
    pub email: String,
    pub role: String,
    pub auth_provider: String,
}

#[derive(Debug, Serialize)]
pub struct AuthMethods {
    pub password: bool,
    pub sso: bool,
    pub oauth: bool,
    pub sso_login_url: Option<String>,
    pub oauth_login_url: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Regulation {
    pub id: i64,
    pub title: String,
    pub regulation_type: String,
    pub issuing_authority: String,
    pub effective_date: Option<NaiveDate>,
    pub summary: String,
    pub file_url: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct RegulationCreate {
    pub title: String,
    pub regulation_type: String,
    pub issuing_authority: String,
    pub effective_date: Option<NaiveDate>,
    pub summary: String,
    pub file_url: Option<String>,
}

#[derive(Debug, Serialize, FromRow)]
pub struct IncidentCase {
    pub id: i64,
    pub title: String,
    pub lab_name: String,
    pub occurred_on: NaiveDate,
    pub severity: String,
    pub category: String,
    pub root_cause: String,
    pub corrective_actions: String,
    pub file_url: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct IncidentCaseCreate {
    pub title: String,
    pub lab_name: String,
    pub occurred_on: NaiveDate,
    pub severity: String,
    pub category: String,
    pub root_cause: String,
    pub corrective_actions: String,
    pub file_url: Option<String>,
}

#[derive(Debug, Serialize, FromRow)]
pub struct Training {
    pub id: i64,
    pub title: String,
    pub target_role: String,
    pub status: String,
    pub starts_on: Option<NaiveDate>,
    pub exam_required_score: i32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct TrainingCreate {
    pub title: String,
    pub target_role: String,
    pub status: Option<String>,
    pub starts_on: Option<NaiveDate>,
    pub exam_required_score: Option<i32>,
}

#[derive(Debug, Serialize, FromRow)]
pub struct ExamResult {
    pub id: i64,
    pub training_id: i64,
    pub user_id: i64,
    pub score: i32,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct ExamResultCreate {
    pub training_id: i64,
    pub user_id: i64,
    pub score: i32,
    pub status: String,
}

#[derive(Debug, Serialize, FromRow)]
pub struct Equipment {
    pub id: i64,
    pub asset_code: String,
    pub name: String,
    pub lab_name: String,
    pub status: String,
    pub owner: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct EquipmentCreate {
    pub asset_code: String,
    pub name: String,
    pub lab_name: String,
    pub status: Option<String>,
    pub owner: Option<String>,
}

#[derive(Debug, Serialize, FromRow)]
pub struct EquipmentBooking {
    pub id: i64,
    pub equipment_id: i64,
    pub user_id: i64,
    pub starts_at: DateTime<Utc>,
    pub ends_at: DateTime<Utc>,
    pub purpose: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct EquipmentBookingCreate {
    pub equipment_id: i64,
    pub user_id: i64,
    pub starts_at: DateTime<Utc>,
    pub ends_at: DateTime<Utc>,
    pub purpose: String,
}

#[derive(Debug, Serialize, FromRow)]
pub struct RepairTicket {
    pub id: i64,
    pub equipment_id: i64,
    pub reported_by: i64,
    pub description: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct RepairTicketCreate {
    pub equipment_id: i64,
    pub reported_by: i64,
    pub description: String,
    pub status: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RepairTicketUpdate {
    pub status: String,
}

#[derive(Debug, Serialize, FromRow)]
pub struct SafetyHazard {
    pub id: i64,
    pub title: String,
    pub lab_name: String,
    pub category: String,
    pub description: String,
    pub status: String,
    pub reported_by: i64,
    pub responsible_user_id: Option<i64>,
    pub issue_photo_url: Option<String>,
    pub remediation_photo_url: Option<String>,
    pub remediation_note: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct SafetyHazardCreate {
    pub title: String,
    pub lab_name: String,
    pub category: String,
    pub description: String,
    pub reported_by: i64,
    pub issue_photo_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SafetyHazardClaim {
    pub responsible_user_id: i64,
}

#[derive(Debug, Deserialize)]
pub struct SafetyHazardRemediation {
    pub remediation_photo_url: String,
    pub remediation_note: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SafetyHazardStatusUpdate {
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct DashboardStats {
    pub regulation_count: i64,
    pub incident_count: i64,
    pub training_count: i64,
    pub equipment_count: i64,
    pub open_repair_count: i64,
    pub exam_pass_rate: f64,
}

#[derive(Debug, Serialize)]
pub struct CountBucket {
    pub name: String,
    pub count: i64,
}

#[derive(Debug, Serialize)]
pub struct IncidentAnalytics {
    pub by_category: Vec<CountBucket>,
    pub by_severity: Vec<CountBucket>,
}

#[derive(Debug, Serialize)]
pub struct RegulationAnalytics {
    pub by_type: Vec<CountBucket>,
    pub by_authority: Vec<CountBucket>,
}

#[derive(Debug, Serialize)]
pub struct HazardAnalytics {
    pub by_status: Vec<CountBucket>,
    pub by_category: Vec<CountBucket>,
}

#[derive(Debug, Serialize)]
pub struct UploadedFile {
    pub filename: String,
    pub content_type: Option<String>,
    pub size: usize,
    pub url: String,
}
