use blockcell_core::Paths;
use blockcell_storage::AuditLogger;
use chrono::Utc;

pub async fn verify(paths: &Paths, date: Option<String>, all: bool) -> anyhow::Result<()> {
    let audit_dir = paths.audit_dir();

    if all {
        if !audit_dir.exists() {
            println!("No audit logs found at {}", audit_dir.display());
            return Ok(());
        }

        let mut files: Vec<_> = std::fs::read_dir(&audit_dir)?
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "jsonl"))
            .collect();
        files.sort_by_key(|entry| entry.file_name());

        if files.is_empty() {
            println!("No audit logs found at {}", audit_dir.display());
            return Ok(());
        }

        let mut ok_files = 0usize;
        let mut broken_files = 0usize;
        for entry in files {
            let result = AuditLogger::verify_chain(&entry.path());
            let file_name = entry.file_name().to_string_lossy().to_string();
            if result.valid {
                println!(
                    "OK {} ({} records, {} skipped)",
                    file_name, result.total_records, result.skipped_records
                );
                ok_files += 1;
            } else {
                println!(
                    "BROKEN {} ({} records, {} skipped, {} errors)",
                    file_name,
                    result.total_records,
                    result.skipped_records,
                    result.errors.len()
                );
                for error in &result.errors {
                    println!("  - {error}");
                }
                broken_files += 1;
            }
        }

        println!("{ok_files} files OK, {broken_files} files broken");
        if broken_files > 0 {
            anyhow::bail!("audit chain verification failed");
        }
        return Ok(());
    }

    let date = date.unwrap_or_else(|| Utc::now().format("%Y-%m-%d").to_string());
    let log_file = audit_dir.join(format!("{date}.jsonl"));
    let result = AuditLogger::verify_chain(&log_file);

    if result.valid {
        println!(
            "Audit chain verified: {} records, {} skipped",
            result.total_records, result.skipped_records
        );
        Ok(())
    } else {
        println!(
            "AUDIT CHAIN BROKEN: {} records, {} skipped, {} errors",
            result.total_records,
            result.skipped_records,
            result.errors.len()
        );
        for error in &result.errors {
            println!("  - {error}");
        }
        anyhow::bail!("audit chain verification failed")
    }
}
