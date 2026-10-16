use crate::{content, store};
use anyhow::{Context, Result, ensure};
use serde_json::Value;
use std::{io::Read, path::Path};

// Deserialization ignores unknown fields; exporting their original JSON could leak credentials.
fn known_fields(value: &Value, saved: &Value) -> bool {
    match (value, saved) {
        (Value::Object(input), Value::Object(output)) => input.iter().all(|(key, value)| {
            output
                .get(key)
                .is_some_and(|saved| known_fields(value, saved))
        }),
        (Value::Array(input), Value::Array(output)) => {
            input.len() == output.len() && input.iter().zip(output).all(|(a, b)| known_fields(a, b))
        }
        _ => true,
    }
}

fn validate(values: Vec<Value>) -> Result<()> {
    for (index, value) in values.iter().enumerate() {
        ensure!(
            value.is_object(),
            "Unsupported character payload at record {index}"
        );
        let character = store::decode_character(&value.to_string(), [0; 8])
            .map_err(|_| anyhow::anyhow!("Unsupported character payload at record {index}"))?;
        ensure!(
            character.items.len() == 12
                && character.base.iter().all(|n| *n >= 0)
                && known_fields(value, &serde_json::to_value(character)?),
            "Unsupported character payload fields or inventory at record {index}"
        );
    }
    println!(
        "{}",
        serde_json::json!({"validated_characters": values.len()})
    );
    Ok(())
}

pub async fn run(args: &[String]) -> Result<bool> {
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["--backup-validate"] => {
            let mut input = Vec::new();
            std::io::stdin()
                .take(32 * 1024 * 1024 + 1)
                .read_to_end(&mut input)?;
            ensure!(
                input.len() <= 32 * 1024 * 1024,
                "Backup character input exceeds 32 MiB"
            );
            validate(serde_json::from_slice(&input)?)?;
        }
        ["--backup-validate-updates"] => {
            let mut input = Vec::new();
            std::io::stdin()
                .take(4 * 1024 * 1024 + 1)
                .read_to_end(&mut input)?;
            ensure!(
                input.len() <= 4 * 1024 * 1024,
                "Backup update input exceeds 4 MiB"
            );
            let mut snapshot: store::UpdateSnapshot = serde_json::from_slice(&input)
                .map_err(|_| anyhow::anyhow!("Unsupported update snapshot payload"))?;
            snapshot.validate()?;
            println!("{}", serde_json::json!({"validated_updates": true}));
        }
        ["--backup-fixture", directory, data] => {
            let directory = Path::new(directory);
            std::fs::create_dir(directory).context("Backup fixture requires a new directory")?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))?;
            }
            let content = content::Content::load(Path::new(data))?;
            let store = store::Store::open(&directory.join("game.db"), &content)?;
            for alive in [true, false] {
                let account = store.list(format!("BACKUP_FIXTURE_SECRET_{alive}")).await?;
                let mut character = store
                    .create(account.id, content.get(content::WIZARD)?)
                    .await?;
                character.gain_experience(&content, 400, &mut rand::thread_rng())?;
                character.dungeons = [1, 2, 3];
                character.pirate_caves = 4;
                store.progress(character.clone()).await?;
                let mut items = character.items.clone();
                items[4] = 0xa22;
                store
                    .exchange(
                        vec![store::InventoryWrite {
                            id: character.id,
                            account: account.id,
                            revision: character.revision,
                            items,
                        }],
                        None,
                    )
                    .await?;
                character = store.load(account.id, character.id).await?;
                if !alive {
                    character.alive = false;
                    character.hp = 0;
                    store
                        .death(character, "Backup contract fixture".into())
                        .await?;
                }
            }
            store
                .sync_updates(store::UpdateSnapshot {
                    guild_id: "12345678901234567".into(),
                    channel_id: "23456789012345678".into(),
                    posts: vec![store::UpdatePost {
                        id: "34567890123456789".into(),
                        content: "Backup update\nPersisted public text".into(),
                        published_at: 1700000000,
                        edited_at: None,
                        attachments: vec![],
                    }],
                })
                .await?;
            println!("{}", serde_json::json!({"fixture_characters": 2}));
        }
        ["--backup-load", database, data] => {
            ensure!(
                Path::new(database).is_file(),
                "Restored game database missing"
            );
            let content = content::Content::load(Path::new(data))?;
            let store = store::Store::open(Path::new(database), &content)?;
            let rows = {
                let db = rusqlite::Connection::open_with_flags(
                    database,
                    rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
                )?;
                let mut query =
                    db.prepare("SELECT account,id FROM characters WHERE alive=1 ORDER BY id")?;
                query
                    .query_map([], |row| Ok((row.get::<_, i32>(0)?, row.get::<_, i32>(1)?)))?
                    .collect::<rusqlite::Result<Vec<_>>>()?
            };
            store.updates().await?;
            for (account, id) in &rows {
                store.load(*account, *id).await?.effective(&content)?;
            }
            println!(
                "{}",
                serde_json::json!({"loaded_living_characters": rows.len()})
            );
        }
        [command, ..] if command.starts_with("--backup-") => {
            anyhow::bail!("Invalid backup command arguments")
        }
        _ => return Ok(false),
    }
    Ok(true)
}
