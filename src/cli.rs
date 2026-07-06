use std::collections::HashMap;

use anyhow::{Context, bail};
use sqlx::{PgPool, Row};

use crate::{
    config::Settings,
    security::{
        generate_strong_password, hash_password, validate_password_strength, verify_password,
    },
};

pub async fn try_run(args: Vec<String>) -> anyhow::Result<bool> {
    if args.get(1).map(String::as_str) != Some("users") {
        return Ok(false);
    }

    let settings = Settings::from_env()?;
    let pool = crate::db::connect(&settings.database_url).await?;
    crate::db::migrate(&pool).await?;

    match args.get(2).map(String::as_str) {
        Some("bootstrap-super-admin") => {
            bootstrap_super_admin(&pool, parse_flags(&args[3..])?).await?
        }
        Some("create") => create_user(&pool, parse_flags(&args[3..])?).await?,
        Some("list") => list_users(&pool, parse_flags(&args[3..])?).await?,
        Some("set-password") => set_password(&pool, parse_flags(&args[3..])?).await?,
        _ => print_usage(),
    }

    Ok(true)
}

fn print_usage() {
    eprintln!(
        "Usage:
  lab-safety-system users bootstrap-super-admin --generate-password true [--username USER] [--email EMAIL] [--display-name NAME]
  lab-safety-system users bootstrap-super-admin --username USER --password PASS --email EMAIL [--display-name NAME]
  lab-safety-system users create --actor USER --actor-password PASS --username USER --password PASS --email EMAIL --role ROLE [--display-name NAME] [--department NAME]
lab-safety-system users list --actor USER --actor-password PASS
lab-safety-system users set-password --actor USER --actor-password PASS --username USER --password PASS
lab-safety-system users set-password --actor USER --actor-password PASS --username USER --generate-password true

Roles: system_admin, lab_member, visitor. Lab-scoped roles are managed by the HTTP API: lab_admin, lab_member, visitor."
    );
}

fn parse_flags(values: &[String]) -> anyhow::Result<HashMap<String, String>> {
    let mut flags = HashMap::new();
    let mut index = 0;
    while index < values.len() {
        let key = values[index]
            .strip_prefix("--")
            .context("Expected --flag value pair")?;
        let value = values.get(index + 1).context("Missing flag value")?;
        flags.insert(key.to_string(), value.to_string());
        index += 2;
    }
    Ok(flags)
}

fn required(flags: &HashMap<String, String>, key: &str) -> anyhow::Result<String> {
    flags
        .get(key)
        .filter(|value| !value.trim().is_empty())
        .cloned()
        .with_context(|| format!("Missing --{key}"))
}

fn flag_enabled(flags: &HashMap<String, String>, key: &str) -> bool {
    matches!(
        flags.get(key).map(String::as_str),
        Some("true" | "1" | "yes" | "on")
    )
}

async fn bootstrap_super_admin(
    pool: &PgPool,
    flags: HashMap<String, String>,
) -> anyhow::Result<()> {
    let existing: i64 = sqlx::query(
        "select count(*)::bigint as count from users where role in ('system_admin', 'super_admin')",
    )
    .fetch_one(pool)
    .await?
    .get("count");
    if existing > 0 {
        bail!(
            "System administrator already exists; use the existing system administrator for user management"
        );
    }

    let generated = flag_enabled(&flags, "generate-password");
    let username = flags
        .get("username")
        .filter(|value| !value.trim().is_empty())
        .cloned()
        .unwrap_or_else(|| "admin".to_string());
    let password = if generated {
        generate_strong_password()
    } else {
        required(&flags, "password")?
    };
    validate_password_strength(&password).map_err(anyhow::Error::msg)?;
    let email = flags
        .get("email")
        .filter(|value| !value.trim().is_empty())
        .cloned()
        .unwrap_or_else(|| "admin@example.local".to_string());
    let display_name = flags
        .get("display-name")
        .cloned()
        .unwrap_or_else(|| "系统管理员".to_string());

    insert_user(
        pool,
        &username,
        &display_name,
        &email,
        "system_admin",
        flags.get("department").map(String::as_str),
        &password,
    )
    .await?;
    println!("Created system administrator: {username}");
    if generated {
        println!("Generated password: {password}");
    }
    Ok(())
}

async fn create_user(pool: &PgPool, flags: HashMap<String, String>) -> anyhow::Result<()> {
    require_super_admin(pool, &flags).await?;
    let username = required(&flags, "username")?;
    let password = required(&flags, "password")?;
    validate_password_strength(&password).map_err(anyhow::Error::msg)?;
    let email = required(&flags, "email")?;
    let role = required(&flags, "role")?;
    validate_role(&role)?;
    let display_name = flags
        .get("display-name")
        .cloned()
        .unwrap_or_else(|| username.clone());

    insert_user(
        pool,
        &username,
        &display_name,
        &email,
        &role,
        flags.get("department").map(String::as_str),
        &password,
    )
    .await?;
    println!("Created user: {username}");
    Ok(())
}

async fn list_users(pool: &PgPool, flags: HashMap<String, String>) -> anyhow::Result<()> {
    require_super_admin(pool, &flags).await?;
    let rows = sqlx::query(
        "select username, display_name, email, role, auth_provider, is_active from users order by id",
    )
    .fetch_all(pool)
    .await?;
    for row in rows {
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}",
            row.get::<String, _>("username"),
            row.get::<String, _>("display_name"),
            row.get::<String, _>("email"),
            row.get::<String, _>("role"),
            row.get::<String, _>("auth_provider"),
            row.get::<bool, _>("is_active")
        );
    }
    Ok(())
}

async fn set_password(pool: &PgPool, flags: HashMap<String, String>) -> anyhow::Result<()> {
    require_super_admin(pool, &flags).await?;
    let username = required(&flags, "username")?;
    let generated = flag_enabled(&flags, "generate-password");
    let password = if generated {
        generate_strong_password()
    } else {
        required(&flags, "password")?
    };
    validate_password_strength(&password).map_err(anyhow::Error::msg)?;
    let result = sqlx::query(
        "update users set password_hash = $1, auth_provider = 'password', updated_at = now() where username = $2",
    )
    .bind(hash_password(&password))
    .bind(&username)
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        bail!("User not found: {username}");
    }
    println!("Updated password for user: {username}");
    if generated {
        println!("Generated password: {password}");
    }
    Ok(())
}

async fn require_super_admin(pool: &PgPool, flags: &HashMap<String, String>) -> anyhow::Result<()> {
    let actor = required(flags, "actor")?;
    let actor_password = required(flags, "actor-password")?;
    let row = sqlx::query("select role, password_hash, is_active from users where username = $1")
        .bind(&actor)
        .fetch_optional(pool)
        .await?
        .with_context(|| format!("Actor not found: {actor}"))?;
    let role: String = row.get("role");
    let password_hash: Option<String> = row.get("password_hash");
    let active: bool = row.get("is_active");
    if !active
        || !matches!(role.as_str(), "system_admin" | "super_admin")
        || !verify_password(&actor_password, password_hash.as_deref())
    {
        bail!("CLI user management requires a valid active system administrator actor");
    }
    Ok(())
}

async fn insert_user(
    pool: &PgPool,
    username: &str,
    display_name: &str,
    email: &str,
    role: &str,
    department: Option<&str>,
    password: &str,
) -> anyhow::Result<()> {
    validate_role(role)?;
    sqlx::query(
        r#"
        insert into users (username, display_name, email, role, auth_provider, department, password_hash)
        values ($1, $2, $3, $4, 'password', $5, $6)
        "#,
    )
    .bind(username)
    .bind(display_name)
    .bind(email)
    .bind(role)
    .bind(department)
    .bind(hash_password(password))
    .execute(pool)
    .await?;
    Ok(())
}

fn validate_role(role: &str) -> anyhow::Result<()> {
    match role {
        "system_admin" | "lab_member" | "visitor" => Ok(()),
        _ => bail!("Invalid role: {role}. Use system_admin, lab_member, or visitor"),
    }
}
