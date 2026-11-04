use crate::{
    content::Content,
    state_lock::StateLock,
    store::{
        Store,
        account_save::{AccountSave, DISCLOSURE, ImportExpectation, ImportResult, MAX_BYTES},
    },
};
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OpenFlags};
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::{IsTerminal, Read, Write},
    path::Path,
};

fn readonly(path: &Path) -> Result<Connection> {
    Ok(Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?)
}
fn verify(db: &Connection, content: &Content) -> Result<()> {
    let integrity: String = db.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
    ensure!(
        integrity == "ok",
        "snapshot integrity check failed: {integrity}"
    );
    ensure!(
        !db.prepare("PRAGMA foreign_key_check")?.exists([])?,
        "snapshot foreign key check failed"
    );
    let version: i32 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    ensure!(
        matches!(version, 21 | 22),
        "snapshot requires supported local database schema 21 or 22 (found {version})"
    );
    crate::store::account_save::verify_database(db, content)?;
    Ok(())
}
fn private_create(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    Ok(options.open(path)?)
}
fn sync_parent(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        let absolute = path.canonicalize()?;
        File::open(absolute.parent().context("missing parent directory")?)?.sync_all()?;
    }
    Ok(())
}
pub(crate) fn snapshot(database: &Path, destination: &Path, content: &Content) -> Result<()> {
    let source = readonly(database)?;
    let source = source.unchecked_transaction()?;
    verify(&source, content)?;
    // Reserve privately before SQLite opens it; a partial file is never reported as a verified backup.
    let staging = destination.with_extension(format!("partial-{:016x}", rand::random::<u64>()));
    let reserved = private_create(&staging)?;
    let result = (|| -> Result<()> {
        let mut output = Connection::open(&staging)?;
        let backup = rusqlite::backup::Backup::new(&source, &mut output)?;
        backup.run_to_completion(128, std::time::Duration::from_millis(5), None)?;
        drop(backup);
        verify(&output, content)?;
        output.close().map_err(|(_, e)| e)?;
        reserved.sync_all()?;
        std::fs::hard_link(&staging, destination).context("publish distinct verified snapshot")?;
        sync_parent(destination)?;
        Ok(())
    })();
    std::fs::remove_file(&staging).context("remove snapshot staging file")?;
    result
}
fn restore_database(source: &Connection, target: &mut Connection) -> Result<()> {
    let copy = rusqlite::backup::Backup::new(source, target)?;
    #[cfg(not(test))]
    copy.run_to_completion(128, std::time::Duration::from_millis(5), None)?;
    #[cfg(test)]
    copy.run_to_completion(1, std::time::Duration::from_millis(5), Some(restore_cut))?;
    Ok(())
}
#[cfg(test)]
fn restore_cut(progress: rusqlite::backup::Progress) {
    let Ok(root) = std::env::var("ACCOUNT_SAVE_RESTORE_CUT_ROOT") else {
        return;
    };
    let committed = std::env::var("ACCOUNT_SAVE_RESTORE_CUT").unwrap() == "committed";
    if committed == (progress.remaining == 0) {
        std::fs::write(Path::new(&root).join("ready"), b"ready").unwrap();
        loop {
            std::thread::park();
        }
    }
}
fn read_save(path: &Path) -> Result<(AccountSave, String)> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)?;
    Ok((
        AccountSave::decode(&bytes)?,
        format!("{:x}", Sha256::digest(&bytes)),
    ))
}
#[derive(serde::Serialize)]
struct Preview<'a> {
    #[serde(flatten)]
    result: ImportResult,
    exported_at: u64,
    name: &'a str,
    character_count: usize,
    expected: String,
}
fn destination_plan(
    state: &Path,
    save: &AccountSave,
    content: &Content,
    fresh: ImportResult,
) -> Result<ImportResult> {
    if !state.join("game.db").exists() {
        return Ok(fresh);
    }
    let db = readonly(&state.join("game.db"))?;
    let tx = db.unchecked_transaction()?;
    verify(&tx, content)?;
    crate::store::account_save::preview(&tx, save)
}
fn password() -> Result<String> {
    ensure!(
        !std::io::stdin().is_terminal(),
        "password must arrive over protected stdin/IPC, not an echoing terminal"
    );
    let mut bytes = Vec::new();
    std::io::stdin().take(130).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 129, "password input too long");
    if bytes.last() == Some(&b'\n') {
        bytes.pop();
    }
    let value = String::from_utf8(bytes).context("password must be UTF-8")?;
    Ok(value)
}
pub(crate) async fn run(args: &[String]) -> Result<bool> {
    if args.first().map(String::as_str) != Some("--local-save") {
        return Ok(false);
    }
    let args: Vec<_> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        [_, "verify-account", file, data] => {
            let (saved, _) = read_save(Path::new(file))?;
            let content = Content::load(Path::new(data))?;
            let validation = Store::open(Path::new(":memory:"), &content)?;
            validation
                .import_account_save(saved.clone(), "validation-only-password".into(), false)
                .await?;
            println!(
                "{}",
                serde_json::json!({"format":saved.format,"origin":saved.origin,"name":saved.name,"disclosure":DISCLOSURE})
            );
        }
        [_, "preview", state, data, file] => {
            let (saved, input_sha256) = read_save(Path::new(file))?;
            let content = Content::load(Path::new(data))?;
            let validation = Store::open(Path::new(":memory:"), &content)?;
            let fresh = validation
                .import_account_save(saved.clone(), "validation-only-password".into(), false)
                .await?;
            let state = Path::new(state);
            let _lock = if state.join("game.db").exists() {
                Some(StateLock::acquire(state)?)
            } else {
                None
            };
            let result = destination_plan(state, &saved, &content, fresh)?;
            let expected = crate::store::account_save::expected(&saved, &result, &input_sha256)?;
            println!(
                "{}",
                serde_json::to_string(&Preview {
                    result,
                    exported_at: saved.exported_at,
                    name: &saved.name,
                    character_count: saved.characters.len(),
                    expected
                })?
            );
        }
        [_, "verify-snapshot", file, data] => {
            let content = Content::load(Path::new(data))?;
            let db = readonly(Path::new(file))?;
            let tx = db.unchecked_transaction()?;
            verify(&tx, &content)?;
            let schema: i32 = tx.query_row("PRAGMA user_version", [], |r| r.get(0))?;
            let account_count: i64 =
                tx.query_row("SELECT count(*) FROM accounts", [], |r| r.get(0))?;
            let character_count: i64 =
                tx.query_row("SELECT count(*) FROM characters", [], |r| r.get(0))?;
            println!(
                "{}",
                serde_json::json!({"verified":true,"schema":schema,"scope":"all-local-accounts","account_count":account_count,"character_count":character_count})
            );
        }
        [_, "snapshot", state, data, output] => {
            let content = Content::load(Path::new(data))?;
            let state = Path::new(state);
            let _lock = StateLock::acquire(state)?;
            snapshot(&state.join("game.db"), Path::new(output), &content)?;
            println!("{}", serde_json::json!({"snapshot":output,"verified":true}));
        }
        [_, "restore", state, data, input, backup] => {
            let content = Content::load(Path::new(data))?;
            let state = Path::new(state);
            let _lock = StateLock::acquire(state)?;
            let source = readonly(Path::new(input))?;
            let source = source.unchecked_transaction()?;
            verify(&source, &content)?;
            ensure!(
                std::fs::canonicalize(input)? != std::fs::canonicalize(state.join("game.db"))?,
                "cannot restore database onto itself"
            );
            snapshot(&state.join("game.db"), Path::new(backup), &content)?;
            let mut target = Connection::open(state.join("game.db"))?;
            restore_database(&source, &mut target)?;
            verify(&target, &content)?;
            println!("{}", serde_json::json!({"restored":true,"backup":backup}));
        }
        [_, "export", state, data, account, output] => {
            let state = Path::new(state);
            let _lock = StateLock::acquire(state)?;
            let content = Content::load(Path::new(data))?;
            let db = readonly(&state.join("game.db"))?;
            verify(&db, &content)?;
            let tx = db.unchecked_transaction()?;
            let base = content
                .classes
                .get(&crate::content::WIZARD)
                .context("Wizard missing")?
                .base;
            let save = crate::store::account_save::export(&tx, account.parse()?, base).context("export requires initialized save namespace; start the matching server once to migrate schema 21")?;
            let mut file = private_create(Path::new(output))?;
            file.write_all(&serde_json::to_vec(&save)?)?;
            file.sync_all()?;
            sync_parent(Path::new(output))?;
            println!(
                "{}",
                serde_json::json!({"exported":output,"origin":save.origin,"disclosure":DISCLOSURE})
            );
        }
        [
            _,
            operation @ ("import" | "replace"),
            state,
            data,
            input,
            backup,
            expected,
        ] => {
            crate::store::account_save::validate_expected(expected)?;
            let (save, input_sha256) = read_save(Path::new(input))?;
            let password = password()?;
            let content = Content::load(Path::new(data))?;
            // Run the same domain transaction in isolation before opening or migrating the destination.
            let validation = Store::open(Path::new(":memory:"), &content)?;
            let fresh = validation
                .import_account_save(save.clone(), password.clone(), false)
                .await?;
            let state = Path::new(state);
            if !state.exists() {
                std::fs::create_dir_all(state)?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(state, std::fs::Permissions::from_mode(0o700))?;
                }
            }
            let _lock = StateLock::acquire(state)?;
            let plan = destination_plan(state, &save, &content, fresh)
                .context("local import identity unavailable; re-preview this file")?;
            let expectation = ImportExpectation {
                input_sha256,
                expected: expected.to_string(),
            };
            crate::store::account_save::check_expected(&save, &plan, &expectation)?;
            ensure!(
                plan.replaced == (*operation == "replace"),
                "import/replacement identity changed; re-preview this file and choose the confirmed operation"
            );
            let db = state.join("game.db");
            if db.exists() {
                snapshot(&db, Path::new(backup), &content)?;
            } else {
                ensure!(
                    *operation == "import",
                    "replacement needs existing database"
                );
            }
            let store = Store::open(&db, &content)?;
            let result = store
                .import_expected_account_save(save, password, *operation == "replace", expectation)
                .await?;
            println!("{}", serde_json::to_string(&result)?);
        }
        [_, "import" | "replace", ..] => anyhow::bail!(
            "import/replace requires STATE DATA FILE PREBACKUP EXPECTED; re-preview this file (password via protected stdin)"
        ),
        _ => anyhow::bail!(
            "usage: --local-save verify-account FILE DATA | preview STATE DATA FILE | verify-snapshot FILE DATA | snapshot STATE DATA OUTPUT | restore STATE DATA INPUT PREBACKUP | export STATE DATA ACCOUNT OUTPUT | import/replace STATE DATA INPUT PREBACKUP EXPECTED (password via protected stdin)"
        ),
    }
    Ok(true)
}
