use std::time::Duration;

use sqlx::{PgPool, postgres::PgPoolOptions};

pub async fn connect(database_url: &str) -> anyhow::Result<PgPool> {
    Ok(PgPoolOptions::new()
        .max_connections(10)
        .acquire_timeout(Duration::from_secs(8))
        .connect(database_url)
        .await?)
}

pub async fn migrate(pool: &PgPool) -> anyhow::Result<()> {
    for statement in MIGRATIONS {
        sqlx::query(statement).execute(pool).await?;
    }
    Ok(())
}

const MIGRATIONS: &[&str] = &[
    r#"
    create table if not exists users (
        id bigserial primary key,
        username text not null unique,
        display_name text not null,
        email text not null unique,
        role text not null default 'lab_member',
        auth_provider text not null default 'password',
        department text,
        password_hash text,
        is_active boolean not null default true,
        created_at timestamptz not null default now(),
        updated_at timestamptz not null default now()
    )
    "#,
    r#"
    create table if not exists labs (
        id bigserial primary key,
        code text not null unique,
        name text not null,
        location text,
        department text,
        manager_user_id bigint references users(id) on delete set null,
        contact text,
        status text not null default 'active',
        description text,
        created_at timestamptz not null default now(),
        updated_at timestamptz not null default now()
    )
    "#,
    r#"
    create index if not exists idx_labs_status on labs(status)
    "#,
    r#"
    create index if not exists idx_labs_name on labs(name)
    "#,
    r#"
    update users set role = 'system_admin' where role = 'super_admin'
    "#,
    r#"
    create unique index if not exists idx_users_single_system_admin
    on users ((role))
    where role = 'system_admin'
    "#,
    r#"
    create table if not exists lab_users (
        id bigserial primary key,
        lab_id bigint not null references labs(id) on delete cascade,
        user_id bigint not null references users(id) on delete cascade,
        lab_role text not null,
        created_at timestamptz not null default now(),
        updated_at timestamptz not null default now(),
        unique (lab_id, user_id)
    )
    "#,
    r#"
    create index if not exists idx_lab_users_user_id on lab_users(user_id)
    "#,
    r#"
    create index if not exists idx_lab_users_lab_role on lab_users(lab_id, lab_role)
    "#,
    r#"
    create table if not exists regulations (
        id bigserial primary key,
        title text not null,
        regulation_type text not null,
        issuing_authority text not null,
        effective_date date,
        summary text not null,
        file_url text,
        created_at timestamptz not null default now(),
        updated_at timestamptz not null default now()
    )
    "#,
    r#"
    insert into regulations (title, regulation_type, issuing_authority, effective_date, summary, file_url)
    select title, regulation_type, issuing_authority, effective_date, summary, file_url
    from (values
        (
            '危险化学品安全管理条例',
            'administrative_regulation',
            '国务院',
            date '2011-12-01',
            '规范危险化学品生产、储存、使用、经营和运输安全管理，适合作为实验室危化品台账、采购、存储和应急处置制度的基础法规。',
            null::text
        ),
        (
            '中华人民共和国安全生产法',
            'law',
            '全国人民代表大会常务委员会',
            date '2021-09-01',
            '明确生产经营单位安全生产主体责任、从业人员安全权利义务、风险分级管控和隐患排查治理要求。',
            null::text
        ),
        (
            '实验室生物安全通用要求 GB 19489',
            'national_standard',
            '国家市场监督管理总局、国家标准化管理委员会',
            date '2008-07-01',
            '提供实验室生物安全管理体系、设施设备、人员防护、废弃物处置和应急管理的通用要求。',
            null::text
        ),
        (
            '高等学校实验室安全规范',
            'industry_guideline',
            '教育部',
            date '2023-02-01',
            '覆盖高校实验室安全责任体系、准入培训、危险源管理、检查整改和事故报告等管理要求。',
            null::text
        )
    ) as seed(title, regulation_type, issuing_authority, effective_date, summary, file_url)
    where not exists (
        select 1 from regulations existing where existing.title = seed.title
    )
    "#,
    r#"
    create table if not exists incident_cases (
        id bigserial primary key,
        title text not null,
        lab_name text not null,
        occurred_on date not null,
        severity text not null,
        category text not null,
        root_cause text not null,
        corrective_actions text not null,
        created_at timestamptz not null default now(),
        updated_at timestamptz not null default now()
    )
    "#,
    r#"
    alter table incident_cases
    add column if not exists file_url text
    "#,
    r#"
    alter table incident_cases
    add column if not exists lab_id bigint references labs(id) on delete set null
    "#,
    r#"
    create index if not exists idx_incident_cases_lab_id on incident_cases(lab_id)
    "#,
    r#"
    create table if not exists trainings (
        id bigserial primary key,
        title text not null,
        target_role text not null,
        status text not null default 'draft',
        starts_on date,
        exam_required_score integer not null default 80,
        created_at timestamptz not null default now(),
        updated_at timestamptz not null default now()
    )
    "#,
    r#"
    alter table trainings
    add column if not exists lab_id bigint references labs(id) on delete set null
    "#,
    r#"
    create index if not exists idx_trainings_lab_id on trainings(lab_id)
    "#,
    r#"
    create table if not exists exam_results (
        id bigserial primary key,
        training_id bigint not null references trainings(id) on delete cascade,
        user_id bigint not null references users(id) on delete cascade,
        score integer not null,
        status text not null,
        created_at timestamptz not null default now(),
        updated_at timestamptz not null default now()
    )
    "#,
    r#"
    create table if not exists equipment (
        id bigserial primary key,
        asset_code text not null unique,
        name text not null,
        lab_name text not null,
        status text not null default 'available',
        owner text,
        created_at timestamptz not null default now(),
        updated_at timestamptz not null default now()
    )
    "#,
    r#"
    alter table equipment
    add column if not exists lab_id bigint references labs(id) on delete set null
    "#,
    r#"
    create index if not exists idx_equipment_lab_id on equipment(lab_id)
    "#,
    r#"
    create table if not exists equipment_bookings (
        id bigserial primary key,
        equipment_id bigint not null references equipment(id) on delete cascade,
        user_id bigint not null references users(id) on delete cascade,
        starts_at timestamptz not null,
        ends_at timestamptz not null,
        purpose text not null,
        created_at timestamptz not null default now(),
        updated_at timestamptz not null default now()
    )
    "#,
    r#"
    create table if not exists repair_tickets (
        id bigserial primary key,
        equipment_id bigint not null references equipment(id) on delete cascade,
        reported_by bigint not null references users(id) on delete cascade,
        description text not null,
        status text not null default 'open',
        created_at timestamptz not null default now(),
        updated_at timestamptz not null default now()
    )
    "#,
    r#"
    create table if not exists safety_hazards (
        id bigserial primary key,
        title text not null,
        lab_name text not null,
        category text not null,
        description text not null,
        status text not null default 'open',
        reported_by bigint not null references users(id) on delete cascade,
        responsible_user_id bigint references users(id) on delete set null,
        issue_photo_url text,
        remediation_photo_url text,
        remediation_note text,
        created_at timestamptz not null default now(),
        updated_at timestamptz not null default now()
    )
    "#,
    r#"
    alter table safety_hazards
    add column if not exists lab_id bigint references labs(id) on delete set null
    "#,
    r#"
    create index if not exists idx_safety_hazards_lab_id on safety_hazards(lab_id)
    "#,
    r#"
    create table if not exists hazard_status_events (
        id bigserial primary key,
        hazard_id bigint not null references safety_hazards(id) on delete cascade,
        from_status text,
        to_status text not null,
        actor_user_id bigint references users(id) on delete set null,
        created_at timestamptz not null default now()
    )
    "#,
    r#"
    create index if not exists idx_hazard_status_events_hazard_created
    on hazard_status_events(hazard_id, created_at, id)
    "#,
    // Canonical create status is `open`. Migrate legacy default `reported` → `open`.
    r#"
    alter table safety_hazards alter column status set default 'open'
    "#,
    r#"
    update safety_hazards set status = 'open' where status = 'reported'
    "#,
    r#"
    create table if not exists passkeys (
        id bigserial primary key,
        user_id bigint not null references users(id) on delete cascade,
        credential_id text not null unique,
        name text not null,
        credential_json text not null,
        created_at timestamptz not null default now(),
        last_used_at timestamptz
    )
    "#,
    r#"
    create table if not exists site_settings (
        key text primary key,
        value jsonb not null,
        updated_at timestamptz not null default now()
    )
    "#,
    r#"
    create table if not exists invitations (
        id bigserial primary key,
        code text not null unique,
        lab_id bigint not null references labs(id) on delete cascade,
        target_role text not null,
        max_uses integer,
        used_count integer not null default 0,
        memo text,
        created_by bigint not null references users(id) on delete cascade,
        created_at timestamptz not null default now(),
        expires_at timestamptz,
        status text not null default 'active'
    )
    "#,
    r#"
    create index if not exists idx_invitations_code on invitations(code)
    "#,
    r#"
    alter table users
    add column if not exists invitation_id bigint references invitations(id) on delete set null
    "#,
];
