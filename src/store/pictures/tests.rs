use super::*;

fn fixture() -> (std::path::PathBuf, Content, Store) {
    let path = std::env::temp_dir().join(format!("pictures-{}.db", rand::random::<u64>()));
    let content = Content::load(Path::new("data")).unwrap();
    let store = Store::open(&path, &content).unwrap();
    (path, content, store)
}

#[tokio::test]
async fn save_load_roundtrip_reopen_ownership_and_overwrite_identity() {
    let (path, content, store) = fixture();
    let alice = store.list("alice".into()).await.unwrap().id;
    let bob = store.list("bob".into()).await.unwrap().id;
    let png = test_png(8, 8);
    let id = store
        .save_picture(
            alice,
            "First".into(),
            2,
            "elf, wizard".into(),
            png.clone(),
            true,
        )
        .await
        .unwrap();
    assert_eq!(store.picture(id).await.unwrap(), png);
    assert!(
        store
            .save_picture(alice, "First".into(), 2, "".into(), png.clone(), false)
            .await
            .unwrap_err()
            .to_string()
            .contains("overwrite permission")
    );
    let other = store
        .save_picture(bob, "First".into(), 2, "".into(), png.clone(), true)
        .await
        .unwrap();
    assert_ne!(id, other);
    let replacement = test_png(16, 8);
    assert_eq!(
        store
            .save_picture(
                alice,
                "First".into(),
                3,
                "tree".into(),
                replacement.clone(),
                true
            )
            .await
            .unwrap(),
        id
    );
    assert_eq!(store.picture(other).await.unwrap(), png);
    drop(store);
    let reopened = Store::open(&path, &content).unwrap();
    assert_eq!(reopened.picture(id).await.unwrap(), replacement);
    assert_eq!(reopened.picture(other).await.unwrap(), png);
    let db = Connection::open(&path).unwrap();
    let row: (i32, String, i32, String) = db
        .query_row(
            "SELECT account,name,data_type,tags FROM pictures WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!(row, (alice, "First".into(), 3, "tree".into()));
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, i32>(0))
            .unwrap(),
        15
    );
}

#[test]
fn native_png_dimensions_types_crc_inflate_and_metadata_bounds() {
    for kind in 1..=7 {
        let sizes = if kind == 5 {
            vec![(4, 4), (5, 5), (9, 9), (10, 10)]
        } else {
            vec![(8, 8), (16, 8), (16, 16)]
        };
        for (w, h) in sizes {
            validate_png(kind, &test_png(w, h)).unwrap();
        }
    }
    for (w, h) in [(56, 8), (112, 8), (112, 16)] {
        validate_png(1, &test_png(w, h)).unwrap();
    }
    for (kind, w, h) in [
        (0, 8, 8),
        (8, 8, 8),
        (2, 56, 8),
        (5, 8, 8),
        (1, 112, 16 + 1),
        (2, 1, 1),
    ] {
        assert!(validate_png(kind, &test_png(w, h)).is_err());
    }
    let png = test_png(8, 8);
    for n in 0..png.len() {
        assert!(validate_png(2, &png[..n]).is_err(), "truncation {n}");
    }
    let mut bad = png.clone();
    bad[44] ^= 1;
    assert!(
        validate_png(2, &bad)
            .unwrap_err()
            .to_string()
            .contains("CRC")
    );
    bad = png.clone();
    bad.extend([0]);
    assert!(validate_png(2, &bad).is_err());
    bad = png.clone();
    bad.resize(MAX_PNG + 1, 0);
    assert!(validate_png(2, &bad).is_err());
    // A valid CRC/header cannot disguise a decompression bomb or truncated scanlines.
    for raw_height in [7, 9, 10000] {
        let mut bad = test_png(8, raw_height);
        bad[20..24].copy_from_slice(&8_u32.to_be_bytes());
        let crc = crc32fast::hash(&bad[12..29]);
        bad[29..33].copy_from_slice(&crc.to_be_bytes());
        assert!(validate_png(2, &bad).is_err());
    }
    metadata("Name 123", 1, "elf, wizard").unwrap();
    for name in ["", "   ", "<x>", "../a", &"a".repeat(33)] {
        assert!(metadata(name, 1, "").is_err());
    }
    for tags in ["UPPER", "a\nb", "<tag>", &"a".repeat(257)] {
        assert!(metadata("A", 1, tags).is_err());
    }
}

#[tokio::test]
async fn malformed_current_picture_state_is_rejected_not_repaired() {
    for mutation in [
        "DROP TABLE pictures",
        "DROP TABLE picture_sequence",
        "DELETE FROM picture_sequence",
        "UPDATE picture_sequence SET next_id=1",
        "UPDATE pictures SET png=x'00'",
        "PRAGMA foreign_keys=OFF; UPDATE pictures SET account=99999",
    ] {
        let (path, content, store) = fixture();
        let alice = store.list("alice".into()).await.unwrap().id;
        store
            .save_picture(alice, "A".into(), 2, "".into(), test_png(8, 8), true)
            .await
            .unwrap();
        drop(store);
        let db = Connection::open(&path).unwrap();
        db.execute_batch(mutation).unwrap();
        assert!(Store::open(&path, &content).is_err(), "{mutation}");
    }
}

fn query(guid: Option<&str>, kind: Option<i32>, tags: &str, offset: i32) -> PictureQuery {
    PictureQuery {
        my_guid: "alice".into(),
        guid: guid.map(str::to_owned),
        data_type: kind,
        tags: tags.into(),
        offset,
        num: 24,
    }
}

fn ids(xml: &str) -> Vec<i32> {
    roxmltree::Document::parse(xml)
        .unwrap()
        .descendants()
        .filter(|n| n.has_tag_name("Pic"))
        .map(|n| n.attribute("id").unwrap().parse().unwrap())
        .collect()
}

#[tokio::test]
async fn gallery_pages_filters_canonical_identity_and_owner_delete_share_storage() {
    let (path, content, store) = fixture();
    let alice = store.list("alice".into()).await.unwrap().id;
    let bob = store
        .register(
            "bob".into(),
            "bob@example.org".into(),
            "valid-password".into(),
        )
        .await
        .unwrap();
    let mut saved = Vec::new();
    for n in 0..27 {
        saved.push(
            store
                .save_picture(
                    if n == 0 { bob } else { alice },
                    format!("Art {n}"),
                    if n % 2 == 0 { 2 } else { 3 },
                    if n % 3 == 0 {
                        "elf, dark wizard"
                    } else {
                        "wizard"
                    }
                    .into(),
                    test_png(8, 8),
                    true,
                )
                .await
                .unwrap(),
        );
    }
    let xml = store.picture_list(query(None, None, "", 0)).await.unwrap();
    assert_eq!(
        ids(&xml),
        saved[3..].iter().rev().copied().collect::<Vec<_>>()
    );
    assert_eq!(xml.matches("<Mine/>").count(), 24);
    assert!(!xml.contains("Admin") && !xml.contains("bob@example.org") && !xml.contains("account"));
    let second = store.picture_list(query(None, None, "", 24)).await.unwrap();
    assert_eq!(
        ids(&second),
        saved[..3].iter().rev().copied().collect::<Vec<_>>()
    );
    assert_eq!(second.matches("<Mine/>").count(), 2);
    assert!(second.contains("offset=\"24\""));
    assert!(ids(&store.picture_list(query(None, None, "", 48)).await.unwrap()).is_empty());
    let result = store
        .picture_list(query(Some("BOB@EXAMPLE.ORG"), None, "", 0))
        .await
        .unwrap();
    assert_eq!(ids(&result), vec![saved[0]]);
    assert!(!result.contains("Mine"));
    assert!(
        ids(&store
            .picture_list(query(Some("bob"), None, "", 0))
            .await
            .unwrap())
        .is_empty()
    );
    assert_eq!(
        ids(&store
            .picture_list(query(None, Some(2), "elf, dark wizard", 0))
            .await
            .unwrap()),
        vec![saved[24], saved[18], saved[12], saved[6], saved[0]]
    );
    assert!(
        ids(&store
            .picture_list(query(None, None, "dark", 0))
            .await
            .unwrap())
        .is_empty()
    );
    assert!(
        ids(&store
            .picture_list(query(Some("administrator@wildshadow.com"), None, "", 0))
            .await
            .unwrap())
        .is_empty()
    );
    // An ordinary registered account bearing the reserved string is not a studio corpus.
    let studio = store
        .register(
            "not-studio".into(),
            "administrator@wildshadow.com".into(),
            "valid-password".into(),
        )
        .await
        .unwrap();
    store
        .save_picture(
            studio,
            "Ordinary".into(),
            2,
            "".into(),
            test_png(8, 8),
            true,
        )
        .await
        .unwrap();
    assert!(
        ids(&store
            .picture_list(query(Some("administrator@wildshadow.com"), None, "", 0))
            .await
            .unwrap())
        .is_empty()
    );
    assert!(store.delete_picture(bob, saved[1]).await.is_err());
    assert!(store.picture(saved[1]).await.is_ok());
    store.delete_picture(alice, saved[1]).await.unwrap();
    assert!(store.picture(saved[1]).await.is_err());
    assert!(store.delete_picture(alice, saved[1]).await.is_err());
    drop(store);
    let reopened = Store::open(&path, &content).unwrap();
    assert!(reopened.picture(saved[1]).await.is_err());
    for q in [
        query(None, None, "", -1),
        query(None, Some(0), "", 0),
        query(None, None, "UPPER", 0),
        PictureQuery {
            num: 25,
            ..query(None, None, "", 0)
        },
    ] {
        assert!(reopened.picture_list(q).await.is_err());
    }
}

#[tokio::test]
async fn picture_high_water_survives_delete_reopen_and_signed_id_exhaustion() {
    let (path, content, store) = fixture();
    let alice = store.list("alice".into()).await.unwrap().id;
    let first = store
        .save_picture(alice, "A".into(), 2, "".into(), test_png(8, 8), true)
        .await
        .unwrap();
    store.delete_picture(alice, first).await.unwrap();
    drop(store);
    let store = Store::open(&path, &content).unwrap();
    let second = store
        .save_picture(alice, "B".into(), 2, "".into(), test_png(8, 8), true)
        .await
        .unwrap();
    assert!(second > first);
    let db = Connection::open(&path).unwrap();
    db.execute("UPDATE picture_sequence SET next_id=2147483647", [])
        .unwrap();
    let last = store
        .save_picture(alice, "Last".into(), 2, "".into(), test_png(8, 8), true)
        .await
        .unwrap();
    assert_eq!(last, i32::MAX);
    store.delete_picture(alice, last).await.unwrap();
    drop(store);
    let store = Store::open(&path, &content).unwrap();
    assert!(
        store
            .save_picture(alice, "Overflow".into(), 2, "".into(), test_png(8, 8), true)
            .await
            .unwrap_err()
            .to_string()
            .contains("exhausted")
    );
    assert_eq!(
        store
            .save_picture(alice, "B".into(), 3, "elf".into(), test_png(8, 8), true)
            .await
            .unwrap(),
        second
    );
    assert_eq!(
        db.query_row("SELECT next_id FROM picture_sequence", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        2147483648
    );
}

#[test]
fn valid_crc_does_not_hide_apng_bad_zlib_filter_or_trailing_stream() {
    use std::io::Write;
    let chunk = |kind: &[u8; 4], data: &[u8]| {
        let mut out = Vec::from((data.len() as u32).to_be_bytes());
        out.extend(kind);
        out.extend(data);
        let mut crc = crc32fast::Hasher::new();
        crc.update(kind);
        crc.update(data);
        out.extend(crc.finalize().to_be_bytes());
        out
    };
    let png = test_png(8, 8);
    let mut apng = png[..33].to_vec();
    apng.extend(chunk(b"acTL", &[0; 8]));
    apng.extend(&png[33..]);
    assert!(validate_png(2, &apng).is_err());
    let mut pixels = vec![0; 264];
    pixels[0] = 5;
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&pixels).unwrap();
    let encoded = encoder.finish().unwrap();
    let mut invalid = png[..33].to_vec();
    invalid.extend(chunk(b"IDAT", &encoded));
    invalid.extend(chunk(b"IEND", &[]));
    assert!(
        validate_png(2, &invalid)
            .unwrap_err()
            .to_string()
            .contains("filter")
    );
    for (index, mut data) in [encoded.clone(), encoded].into_iter().enumerate() {
        if index == 0 {
            data.push(0);
        } else {
            let last = data.len() - 1;
            data[last] ^= 1;
        }
        invalid = png[..33].to_vec();
        invalid.extend(chunk(b"IDAT", &data));
        invalid.extend(chunk(b"IEND", &[]));
        assert!(validate_png(2, &invalid).is_err());
    }
}

async fn capacity_fixture(
    count: usize,
    accounts: usize,
) -> (std::path::PathBuf, Content, Store, Vec<i32>) {
    let (path, content, store) = fixture();
    let mut owners = Vec::new();
    for i in 0..accounts {
        owners.push(store.list(format!("capacity-{i}")).await.unwrap().id);
    }
    let saved = owners.clone();
    store
        .call(move |db| {
            let tx = db.transaction()?;
            for i in 0..count {
                tx.execute(
                    "INSERT INTO pictures VALUES(?1,?2,?3,2,'capacity',?4)",
                    params![
                        i + 1,
                        saved[(i / MAX_PER_ACCOUNT) % saved.len()],
                        format!("Art {i}"),
                        test_png(8, 8)
                    ],
                )?;
            }
            tx.execute("UPDATE picture_sequence SET next_id=?1", [count + 1])?;
            tx.commit()?;
            Ok(())
        })
        .await
        .unwrap();
    (path, content, store, owners)
}

type PictureState = (Vec<(i32, i32, String, i32, String, Vec<u8>)>, i64, i64);

fn picture_state(path: &Path) -> PictureState {
    let db = Connection::open(path).unwrap();
    let mut q = db
        .prepare("SELECT id,account,name,data_type,tags,png FROM pictures ORDER BY id")
        .unwrap();
    let rows = q
        .query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ))
        })
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    let next = db
        .query_row("SELECT next_id FROM picture_sequence", [], |r| r.get(0))
        .unwrap();
    let credits = db
        .query_row("SELECT sum(credits) FROM accounts", [], |r| r.get(0))
        .unwrap();
    (rows, next, credits)
}

#[tokio::test]
async fn capacity_exact_bounds_atomic_rejection_overwrite_delete_and_restart() {
    let (path, content, store, owners) = capacity_fixture(MAX_TOTAL - 1, 9).await;
    let before = picture_state(&path);
    let error = store
        .save_picture(
            owners[0],
            "Art 128".into(),
            2,
            "".into(),
            test_png(8, 8),
            true,
        )
        .await
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("account capacity exceeded: 129 rows, limit 128")
    );
    assert_eq!(picture_state(&path), before);
    let mut tasks = Vec::new();
    for i in 0..8 {
        let store = store.clone();
        let owner = owners[8];
        tasks.push(tokio::spawn(async move {
            store
                .save_picture(
                    owner,
                    format!("Race {i}"),
                    2,
                    "".into(),
                    test_png(8, 8),
                    true,
                )
                .await
        }));
    }
    let mut created = Vec::new();
    for task in tasks {
        match task.await.unwrap() {
            Ok(id) => created.push(id),
            Err(e) => assert!(
                e.to_string()
                    .contains("global capacity exceeded: 1025 rows, limit 1024")
            ),
        }
    }
    assert_eq!(created, vec![1024]);
    assert_eq!(picture_state(&path).1, 1025);
    let before = picture_state(&path);
    assert!(
        store
            .save_picture(
                owners[8],
                "Art 0".into(),
                2,
                "".into(),
                test_png(8, 8),
                true
            )
            .await
            .is_err()
    );
    assert_eq!(picture_state(&path), before);
    assert_eq!(
        store
            .save_picture(
                owners[0],
                "Art 0".into(),
                3,
                "changed".into(),
                test_png(16, 8),
                true
            )
            .await
            .unwrap(),
        1
    );
    assert!(
        store
            .save_picture(
                owners[0],
                "Art 0".into(),
                2,
                "".into(),
                test_png(8, 8),
                false
            )
            .await
            .unwrap_err()
            .to_string()
            .contains("overwrite permission")
    );
    store.delete_picture(owners[8], 1024).await.unwrap();
    assert_eq!(
        store
            .save_picture(
                owners[8],
                "Reclaimed".into(),
                2,
                "".into(),
                test_png(8, 8),
                true
            )
            .await
            .unwrap(),
        1025
    );
    drop(store);
    let store = Store::open(&path, &content).unwrap();
    assert_eq!(store.picture(1).await.unwrap(), test_png(16, 8));
    let before = picture_state(&path);
    assert!(
        store
            .save_picture(
                owners[8],
                "Blocked after restart".into(),
                2,
                "".into(),
                test_png(8, 8),
                true
            )
            .await
            .is_err()
    );
    assert_eq!(picture_state(&path), before);
    store.delete_picture(owners[0], 1).await.unwrap();
    assert_eq!(
        store
            .save_picture(
                owners[0],
                "New after restart".into(),
                2,
                "".into(),
                test_png(8, 8),
                true
            )
            .await
            .unwrap(),
        1026
    );
    let state = picture_state(&path);
    assert_eq!((state.0.len(), state.1, state.2), (1024, 1027, before.2));
}

#[tokio::test]
async fn capacity_concurrent_last_account_slot_cannot_be_bypassed_by_other_owner_names() {
    let (path, content, store, owners) = capacity_fixture(MAX_PER_ACCOUNT - 1, 2).await;
    let mut tasks = Vec::new();
    for i in 0..16 {
        let store = store.clone();
        let owner = owners[0];
        tasks.push(tokio::spawn(async move {
            store
                .save_picture(
                    owner,
                    format!("Race {i}"),
                    2,
                    "".into(),
                    test_png(8, 8),
                    true,
                )
                .await
        }));
    }
    let mut created = Vec::new();
    for task in tasks {
        match task.await.unwrap() {
            Ok(id) => created.push(id),
            Err(e) => assert!(e.to_string().contains("account capacity exceeded")),
        }
    }
    assert_eq!(created, vec![128]);
    assert_eq!(picture_state(&path).1, 129);
    assert_eq!(
        store
            .save_picture(
                owners[1],
                "Art 0".into(),
                3,
                "other".into(),
                test_png(16, 8),
                true
            )
            .await
            .unwrap(),
        129
    );
    assert_eq!(store.picture(1).await.unwrap(), test_png(8, 8));
    drop(store);
    Store::open(&path, &content).unwrap();
}

#[tokio::test]
async fn capacity_oversized_current_state_is_rejected_without_truncation() {
    for (count, accounts, message) in [
        (129, 1, "account capacity exceeded"),
        (1025, 9, "global capacity exceeded"),
    ] {
        let (path, content, store, _) = capacity_fixture(count, accounts).await;
        drop(store);
        let before = picture_state(&path);
        assert!(
            Store::open(&path, &content)
                .err()
                .unwrap()
                .to_string()
                .contains(message)
        );
        assert_eq!(picture_state(&path), before);
    }
}
