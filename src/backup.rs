use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, bail};

use crate::config::Settings;

pub fn create(settings: &Settings, flags: HashMap<String, String>) -> anyhow::Result<()> {
    let upload_dir = flags
        .get("upload-dir")
        .map(PathBuf::from)
        .unwrap_or_else(|| settings.upload_dir.clone());
    fs::create_dir_all(&upload_dir)
        .with_context(|| format!("Create upload directory {}", upload_dir.display()))?;

    let output = flags
        .get("output")
        .map(PathBuf::from)
        .unwrap_or_else(|| default_backup_path(&upload_dir));
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Create backup directory {}", parent.display()))?;
    }
    ensure_output_available(&output, &flags)?;

    let work_dir = std::env::temp_dir().join(format!(
        "lab-safety-backup-{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    fs::create_dir_all(&work_dir)
        .with_context(|| format!("Create temporary directory {}", work_dir.display()))?;

    let archive_output = temporary_archive_path(&output);
    let result = create_archive(settings, &upload_dir, &output, &archive_output, &work_dir)
        .and_then(|_| {
            fs::rename(&archive_output, &output).with_context(|| {
                format!(
                    "Move completed backup {} to {}",
                    archive_output.display(),
                    output.display()
                )
            })
        });
    if result.is_err() && archive_output.exists() {
        let _ = fs::remove_file(&archive_output);
    }
    let cleanup = fs::remove_dir_all(&work_dir)
        .with_context(|| format!("Clean temporary directory {}", work_dir.display()));
    match (result, cleanup) {
        (Ok(()), Ok(())) => {}
        (Err(error), Ok(())) => return Err(error),
        (Ok(()), Err(error)) => return Err(error),
        (Err(error), Err(cleanup_error)) => {
            return Err(error.context(format!(
                "Also failed to clean temporary directory: {cleanup_error}"
            )));
        }
    }
    println!("Created backup: {}", output.display());
    Ok(())
}

fn create_archive(
    settings: &Settings,
    upload_dir: &Path,
    output: &Path,
    archive_output: &Path,
    work_dir: &Path,
) -> anyhow::Result<()> {
    let dump_path = work_dir.join("database.sql");
    run_command(
        Command::new("pg_dump")
            .env("PGCONNECT_TIMEOUT", "10")
            .arg("--format=plain")
            .arg("--no-owner")
            .arg("--no-privileges")
            .arg("--file")
            .arg(&dump_path)
            .arg(&settings.database_url),
        "pg_dump",
    )?;

    fs::write(
        work_dir.join("metadata.json"),
        serde_json::json!({
            "created_at": chrono::Utc::now(),
            "app": env!("CARGO_PKG_NAME"),
            "version": env!("CARGO_PKG_VERSION"),
            "includes": ["database.sql", "uploads/"],
        })
        .to_string(),
    )?;

    let snapshot_dir = work_dir.join("uploads");
    copy_uploads(upload_dir, &snapshot_dir, output)?;
    run_command(
        Command::new("tar")
            .arg("-czf")
            .arg(archive_output)
            .arg("-C")
            .arg(work_dir)
            .arg("database.sql")
            .arg("metadata.json")
            .arg("uploads"),
        "tar",
    )?;
    Ok(())
}

fn copy_uploads(source: &Path, destination: &Path, output: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(destination)
        .with_context(|| format!("Create upload snapshot {}", destination.display()))?;
    let skip_backups = source.join("backups");
    for entry in fs::read_dir(source)
        .with_context(|| format!("Read upload directory {}", source.display()))?
    {
        let entry = entry?;
        let source_path = entry.path();
        if same_path(&source_path, output) || same_path(&source_path, &skip_backups) {
            continue;
        }
        let target_path = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_uploads(&source_path, &target_path, output)?;
        } else {
            fs::copy(&source_path, &target_path).with_context(|| {
                format!(
                    "Copy upload file {} to {}",
                    source_path.display(),
                    target_path.display()
                )
            })?;
        }
    }
    Ok(())
}

fn same_path(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

fn default_backup_path(upload_dir: &Path) -> PathBuf {
    upload_dir.join("backups").join(format!(
        "lab-safety-backup-{}.tar.gz",
        chrono::Utc::now().format("%Y%m%dT%H%M%SZ")
    ))
}

fn temporary_archive_path(output: &Path) -> PathBuf {
    let file_name = output
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("backup.tar.gz");
    let temp_name = format!(
        ".{file_name}.tmp-{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    );
    output.with_file_name(temp_name)
}

fn ensure_output_available(output: &Path, flags: &HashMap<String, String>) -> anyhow::Result<()> {
    if output.exists() && !flag_enabled(flags, "force") {
        bail!(
            "Backup output already exists: {}. Pass --force true to overwrite it.",
            output.display()
        );
    }
    Ok(())
}

fn flag_enabled(flags: &HashMap<String, String>, key: &str) -> bool {
    matches!(
        flags.get(key).map(String::as_str),
        Some("true" | "1" | "yes" | "on")
    )
}

fn run_command(command: &mut Command, label: &str) -> anyhow::Result<()> {
    let output = command
        .output()
        .with_context(|| format!("Run {label}; make sure it is installed and in PATH"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("{label} failed: {}", stderr.trim());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upload_snapshot_excludes_backup_directory() -> anyhow::Result<()> {
        let source = tempfile::tempdir()?;
        fs::create_dir_all(source.path().join("hazards"))?;
        fs::create_dir_all(source.path().join("backups"))?;
        fs::write(source.path().join("hazards").join("issue.txt"), "issue")?;
        fs::write(source.path().join("backups").join("old.tar.gz"), "old")?;

        let destination = tempfile::tempdir()?;
        copy_uploads(
            source.path(),
            &destination.path().join("uploads"),
            &source.path().join("backups").join("new.tar.gz"),
        )?;

        assert!(
            destination
                .path()
                .join("uploads/hazards/issue.txt")
                .exists()
        );
        assert!(
            !destination
                .path()
                .join("uploads/backups/old.tar.gz")
                .exists()
        );
        Ok(())
    }

    #[test]
    fn default_path_uses_timestamped_backup_archive() {
        let path = default_backup_path(Path::new("/var/lib/lab-safety-system/uploads"));
        assert!(path.starts_with("/var/lib/lab-safety-system/uploads/backups"));
        assert_eq!(
            path.extension().and_then(|value| value.to_str()),
            Some("gz")
        );
    }

    #[test]
    fn backup_output_must_not_exist_without_force() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let output = dir.path().join("backup.tar.gz");
        fs::write(&output, "existing")?;

        let error = ensure_output_available(&output, &HashMap::new()).unwrap_err();
        assert!(error.to_string().contains("already exists"));

        let mut flags = HashMap::new();
        flags.insert("force".to_string(), "true".to_string());
        ensure_output_available(&output, &flags)?;
        Ok(())
    }

    #[test]
    fn temporary_archive_path_stays_next_to_final_output() {
        let output = Path::new("/var/backups/lab-safety-system.tar.gz");
        let temporary = temporary_archive_path(output);
        assert_eq!(temporary.parent(), output.parent());
        assert_ne!(temporary, output);
        assert!(
            temporary
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.starts_with(".lab-safety-system.tar.gz.tmp-"))
        );
    }
}
