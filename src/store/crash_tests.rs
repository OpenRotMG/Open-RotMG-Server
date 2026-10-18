use super::*;
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    os::unix::process::ExitStatusExt,
    process::{Child, Command, Stdio},
    time::Duration,
};

struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
struct Directory(std::path::PathBuf);
impl Directory {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("rotmg-item-safety-{:016x}", rand::random::<u64>()));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!("Item-safety evidence retained at {}", self.0.display());
        } else {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }
}

// Wall-clock death timestamps are not part of the ownership/durability contract.
fn snapshot(path: &Path) -> Value {
    let db = Connection::open(path).unwrap();
    assert_eq!(
        db.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
    assert!(
        !db.prepare("PRAGMA foreign_key_check")
            .unwrap()
            .exists([])
            .unwrap()
    );
    let mut state = json!({});
    for (name, sql) in [
        (
            "characters",
            "SELECT json_group_array(json_array(id,account,alive,revision,json(items),json_extract(stats,'$.fame'),json_extract(stats,'$.hp'),json_extract(stats,'$.alive'))) FROM (SELECT * FROM characters ORDER BY id)",
        ),
        (
            "vaults",
            "SELECT json_group_array(json_array(account,revision,json(items))) FROM (SELECT * FROM vaults ORDER BY account)",
        ),
        (
            "deaths",
            "SELECT json_group_array(json_array(character,json_extract(snapshot,'$.items'),json_extract(snapshot,'$.fame'))) FROM (SELECT * FROM deaths ORDER BY character)",
        ),
        (
            "fame",
            "SELECT json_group_array(json_array(account,balance,earned)) FROM (SELECT * FROM account_fame ORDER BY account)",
        ),
        (
            "gold",
            "SELECT json_group_array(json_array(id,credits)) FROM (SELECT * FROM accounts ORDER BY id)",
        ),
    ] {
        let raw: String = db.query_row(sql, [], |r| r.get(0)).unwrap();
        state[name] = serde_json::from_str(&raw).unwrap();
    }
    state
}
fn increment(value: &mut Value) {
    *value = json!(value.as_i64().unwrap() + 1);
}
fn expected(mut state: Value, operation: &str) -> Value {
    if operation == "death" {
        let items = state["characters"][0][4].clone();
        state["deaths"] = json!([[1, items, 7]]);
        state["characters"][0][2] = json!(0);
        state["characters"][0][4] = json!(vec![-1; 12]);
        state["characters"][0][6] = json!(0);
        state["characters"][0][7] = json!(0);
        state["fame"][0][1] = json!(7);
        state["fame"][0][2] = json!(7);
    } else {
        let slot = if operation == "withdraw" { 5 } else { 4 };
        let source = state["characters"][0][4][slot].clone();
        if operation == "trade" {
            state["characters"][0][4][slot] = state["characters"][1][4][4].clone();
            state["characters"][1][4][4] = source;
            increment(&mut state["characters"][1][3]);
        } else {
            state["characters"][0][4][slot] = state["vaults"][0][2][0].clone();
            state["vaults"][0][2][0] = source;
            increment(&mut state["vaults"][0][1]);
        }
    }
    increment(&mut state["characters"][0][3]);
    state
}
fn write(ch: &Character) -> InventoryWrite {
    InventoryWrite {
        id: ch.id,
        account: ch.account,
        revision: ch.revision,
        items: ch.items.clone(),
    }
}
#[derive(Clone)]
enum Operation {
    Exchange(Vec<InventoryWrite>, Option<Vault>),
    Death(Character),
}
impl Operation {
    async fn new(store: &Store, operation: &str) -> Self {
        let mut left = store.load(1, 1).await.unwrap();
        if operation == "death" {
            left.hp = 0;
            left.alive = false;
            return Self::Death(left);
        }
        if operation == "trade" {
            let mut right = store.load(2, 2).await.unwrap();
            std::mem::swap(&mut left.items[4], &mut right.items[4]);
            Self::Exchange(vec![write(&left), write(&right)], None)
        } else {
            assert!(matches!(operation, "deposit" | "withdraw"));
            let mut vault = store.vault(1, 0).await.unwrap();
            let slot = if operation == "withdraw" { 5 } else { 4 };
            std::mem::swap(&mut left.items[slot], &mut vault.items[0]);
            Self::Exchange(vec![write(&left)], Some(vault))
        }
    }
    async fn apply(self, store: &Store) -> Result<()> {
        match self {
            Self::Exchange(writes, vault) => store
                .exchange(writes, vault.into_iter().collect())
                .await
                .map(|_| ()),
            Self::Death(ch) => store.death(ch, "item-safety fixture".into()).await,
        }
    }
}
async fn fixture(path: &Path, content: &Content) -> Store {
    let store = Store::open(path, content).unwrap();
    for (index, guid) in ["safety-left", "safety-right"].into_iter().enumerate() {
        let account = store.list(guid.into()).await.unwrap();
        let ch = store
            .create(account.id, content.get(WIZARD).unwrap())
            .await
            .unwrap();
        assert_eq!(ch.id, index as i32 + 1);
    }
    let mut left = store.load(1, 1).await.unwrap();
    left.fame = 7;
    store.progress(left.clone()).await.unwrap();
    let mut right = store.load(2, 2).await.unwrap();
    right.items[4] = 0xa97;
    store
        .exchange(vec![write(&right)], Vec::new())
        .await
        .unwrap();
    let mut vault = store.vault(1, 0).await.unwrap();
    vault.items[0] = 0xa4f;
    store
        .exchange(vec![write(&left)], vec![vault])
        .await
        .unwrap();
    store
        .call(|db| {
            assert_eq!(
                db.query_row("PRAGMA journal_mode", [], |r| r.get::<_, String>(0))?,
                "wal"
            );
            assert_eq!(
                db.query_row("PRAGMA synchronous", [], |r| r.get::<_, i32>(0))?,
                2
            );
            Ok(())
        })
        .await
        .unwrap();
    store
}
fn gate() {
    let mut stdout = std::io::stdout();
    writeln!(stdout, "\nITEM_SAFETY_GATE").unwrap();
    stdout.flush().unwrap();
    std::io::stdin().read_exact(&mut [0]).unwrap();
}
async fn child(path: &Path, operation: &str, cut: &str, content: &Content) {
    let store = Store::open(path, content).unwrap();
    let operation = Operation::new(&store, operation).await;
    let cut = cut.to_owned();
    store
        .call(move |db| {
            match cut.as_str() {
                "first_write" => db.update_hook(Some(|_, _: &str, table: &str, _: i64| {
                    if table == "characters" {
                        gate();
                    }
                })),
                "before_commit" => db.commit_hook(Some(|| {
                    gate();
                    false
                })),
                "committed" => {}
                _ => panic!("Unknown crash boundary"),
            }
            Ok(())
        })
        .await
        .unwrap();
    operation.apply(&store).await.unwrap();
    gate();
}

#[tokio::test]
async fn killed_exchanges_recover_atomically_without_replaying_committed_items() {
    let content = Content::load(Path::new("data")).unwrap();
    if let Ok(path) = std::env::var("ROTMG_ITEM_CRASH_DB") {
        child(
            Path::new(&path),
            &std::env::var("ROTMG_ITEM_CRASH_OP").unwrap(),
            &std::env::var("ROTMG_ITEM_CRASH_CUT").unwrap(),
            &content,
        )
        .await;
        return;
    }
    let directory = Directory::new();
    let template = directory.0.join("template.db");
    let store = fixture(&template, &content).await;
    for operation in ["trade", "deposit", "withdraw", "death"] {
        for cut in ["first_write", "before_commit", "committed"] {
            let path = directory.0.join(format!("{operation}-{cut}.db"));
            Connection::open(&template)
                .unwrap()
                .execute("VACUUM INTO ?1", [path.to_str().unwrap()])
                .unwrap();
            let original = snapshot(&path);
            let wanted = expected(original.clone(), operation);
            let recovery = Store::open(&path, &content).unwrap();
            let replay = Operation::new(&recovery, operation).await;
            drop(recovery);
            let mut process = Process(Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "store::crash_tests::killed_exchanges_recover_atomically_without_replaying_committed_items", "--nocapture"])
                .env("ROTMG_ITEM_CRASH_DB", &path).env("ROTMG_ITEM_CRASH_OP", operation).env("ROTMG_ITEM_CRASH_CUT", cut)
                .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::inherit()).spawn().unwrap());
            let stdout = process.0.stdout.take().unwrap();
            let (tx, rx) = mpsc::channel();
            let reader = std::thread::spawn(move || {
                let reached = BufReader::new(stdout)
                    .lines()
                    .any(|line| line.unwrap() == "ITEM_SAFETY_GATE");
                let _ = tx.send(reached);
            });
            assert!(
                rx.recv_timeout(Duration::from_secs(30)).unwrap(),
                "{operation}/{cut}: child exited before boundary"
            );
            process.0.kill().unwrap();
            assert_eq!(process.0.wait().unwrap().signal(), Some(9));
            reader.join().unwrap();
            assert_eq!(
                snapshot(&path),
                if cut == "committed" {
                    &wanted
                } else {
                    &original
                }
                .clone(),
                "{operation}/{cut}"
            );
            let recovery = Store::open(&path, &content).unwrap();
            assert_eq!(
                replay.clone().apply(&recovery).await.is_err(),
                cut == "committed",
                "{operation}/{cut}"
            );
            assert_eq!(
                snapshot(&path),
                wanted,
                "{operation}/{cut}: exact recovered owners, slots and balances"
            );
            assert!(
                replay.apply(&recovery).await.is_err(),
                "{operation}/{cut}: stale replay accepted"
            );
            assert_eq!(
                snapshot(&path),
                wanted,
                "{operation}/{cut}: replay changed durable state"
            );
        }
    }
    drop(store);
}

#[tokio::test]
async fn seeded_trade_vault_and_death_sequences_preserve_exact_owners() {
    use rand::{Rng, SeedableRng};
    let content = Content::load(Path::new("data")).unwrap();
    let directory = Directory::new();
    for seed in [0, 1, 12351] {
        let path = directory.0.join(format!("seed-{seed}.db"));
        let mut store = fixture(&path, &content).await;
        let mut model = snapshot(&path);
        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
        let mut trace = Vec::new();
        for step in 0..64 {
            let operation = ["trade", "deposit", "withdraw"][rng.gen_range(0..3)];
            trace.push(operation);
            let plan = Operation::new(&store, operation).await;
            plan.clone().apply(&store).await.unwrap();
            model = expected(model, operation);
            assert_eq!(
                snapshot(&path),
                model,
                "seed={seed} step={step} trace={trace:?}"
            );
            assert!(
                plan.apply(&store).await.is_err(),
                "seed={seed} step={step}: stale replay"
            );
            assert_eq!(
                snapshot(&path),
                model,
                "seed={seed} step={step}: rejected replay mutated state"
            );
            if step % 16 == 15 {
                drop(store);
                store = Store::open(&path, &content).unwrap();
                assert_eq!(snapshot(&path), model, "seed={seed}: reopened state");
            }
        }
        let death = Operation::new(&store, "death").await;
        death.clone().apply(&store).await.unwrap();
        model = expected(model, "death");
        assert_eq!(
            snapshot(&path),
            model,
            "seed={seed}: death must destroy only current character items"
        );
        assert!(death.apply(&store).await.is_err());
        drop(store);
        assert_eq!(
            snapshot(&path),
            model,
            "seed={seed}: death must not award fame twice"
        );
    }
}
