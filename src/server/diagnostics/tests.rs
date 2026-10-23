use super::*;

fn report(id: char, account: i32, at: u64) -> Value {
    json!({"id":id.to_string().repeat(24),"schema":3,"account_id":account,
        "created_at_unix_ms":at,"message_untrusted":"Original description",
        "context":{"player_id":9},"diagnostics":{"capture":"original"},
        "delivery":{"status":"pending"}})
}
fn receipt(target: &str, id: &str, through: Option<usize>) -> Receipt {
    Receipt {
        target: target.into(),
        id: id.into(),
        through,
    }
}
fn stored(d: &Diagnostics, id: char) -> Value {
    serde_json::from_slice(
        &std::fs::read(d.root.join(format!("{}.json", id.to_string().repeat(24)))).unwrap(),
    )
    .unwrap()
}

#[test]
fn details_follow_the_accounts_latest_report_after_restart() {
    let dir = std::env::temp_dir().join(format!("bug-info-{}", rand::random::<u64>()));
    let d = Diagnostics::open(dir.join("reports")).unwrap();
    d.save(&report('c', 1, 300)).unwrap();
    d.save(&report('a', 1, 100)).unwrap();
    d.save(&report('b', 2, 200)).unwrap();
    drop(d);
    let d = Diagnostics::open(dir.join("reports")).unwrap();
    d.add_info(1, "Account one's newer report").unwrap();
    d.add_info(2, "Account two's own report").unwrap();
    assert!(
        d.add_info(3, "No report for this account")
            .unwrap_err()
            .to_string()
            .contains("No bug report yet")
    );
    assert!(stored(&d, 'a').get("notes").is_none());
    assert_eq!(
        stored(&d, 'b')["notes"][0]["text_untrusted"],
        "Account two's own report"
    );
    assert_eq!(
        stored(&d, 'c')["notes"][0]["text_untrusted"],
        "Account one's newer report"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn frozen_batches_survive_restart_new_details_and_duplicate_receipts() {
    let dir = std::env::temp_dir().join(format!("bug-info-{}", rand::random::<u64>()));
    let d = Diagnostics::open(dir.join("reports")).unwrap();
    let original = report('a', 1, 100);
    let id = original["id"].as_str().unwrap();
    d.save(&original).unwrap();
    d.add_info(1, "First detail").unwrap();
    d.add_info(1, "Second detail").unwrap();
    assert_eq!(
        d.next().unwrap().unwrap()["delivery"]["note_batch"],
        json!({"from":1,"through":2})
    );
    drop(d);
    let d = Diagnostics::open(dir.join("reports")).unwrap();
    d.add_info(1, "Arrived while the publisher was offline")
        .unwrap();
    assert_eq!(
        d.next().unwrap().unwrap()["delivery"]["note_batch"],
        json!({"from":1,"through":2})
    );
    assert!(
        d.acknowledge(id, &receipt("issue_number", "123", None))
            .unwrap()
            .is_some()
    );
    assert!(
        d.acknowledge(id, &receipt("issue_number", "123", None))
            .unwrap()
            .is_none()
    );
    d.acknowledge(
        id,
        &receipt("public_message_id", "1560000000000000000", None),
    )
    .unwrap();
    assert_eq!(stored(&d, 'a')["delivery"]["status"], "pending");
    assert!(
        d.acknowledge(id, &receipt("notes", "100", Some(3)))
            .is_err()
    );
    d.acknowledge(id, &receipt("notes", "100", Some(2)))
        .unwrap();
    assert_eq!(
        d.next().unwrap().unwrap()["delivery"]["note_batch"],
        json!({"from":3,"through":3})
    );
    d.acknowledge(id, &receipt("notes", "100", Some(2)))
        .unwrap();
    assert_eq!(
        d.next().unwrap().unwrap()["delivery"]["note_batch"],
        json!({"from":3,"through":3})
    );
    assert!(
        d.acknowledge(id, &receipt("notes", "999", Some(2)))
            .is_err()
    );
    d.acknowledge(id, &receipt("notes", "101", Some(3)))
        .unwrap();
    assert!(d.next().unwrap().is_none());
    assert_eq!(stored(&d, 'a')["diagnostics"], original["diagnostics"]);
    assert_eq!(
        stored(&d, 'a')["message_untrusted"],
        original["message_untrusted"]
    );
    d.add_info(1, "After completed delivery").unwrap();
    assert_eq!(
        d.next().unwrap().unwrap()["delivery"]["note_batch"],
        json!({"from":4,"through":4})
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn concurrent_details_are_bounded_and_never_overwrite_each_other() {
    let dir = std::env::temp_dir().join(format!("bug-info-{}", rand::random::<u64>()));
    let d = Arc::new(Diagnostics::open(dir.join("reports")).unwrap());
    d.save(&report('a', 1, 100)).unwrap();
    assert!(d.add_info(1, " ").is_err());
    assert!(d.add_info(1, &"🐉".repeat(501)).is_err());
    std::thread::scope(|scope| {
        for n in 0..32 {
            let d = d.clone();
            scope.spawn(move || d.add_info(1, &format!("detail {n}")).unwrap());
        }
    });
    let saved = stored(&d, 'a');
    let notes = saved["notes"].as_array().unwrap();
    assert_eq!(notes.len(), 32);
    assert_eq!(
        notes
            .iter()
            .map(|n| n["text_untrusted"].as_str().unwrap())
            .collect::<BTreeSet<_>>()
            .len(),
        32
    );
    assert!(d.add_info(1, "Too many").is_err());
    assert_eq!(stored(&d, 'a'), saved);
    assert!(receipt("notes", "0", Some(1)).validate().is_err());
    assert!(receipt("notes", "100", None).validate().is_err());
    assert!(receipt("issue_number", "123", Some(1)).validate().is_err());
    std::fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn failed_writes_do_not_acknowledge_or_queue_unsaved_details() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("bug-info-{}", rand::random::<u64>()));
    let d = Diagnostics::open(dir.join("reports")).unwrap();
    let mut original = report('a', 1, 100);
    original["delivery"] = json!({"status":"delivered","issue_number":"123","public_message_id":"1560000000000000000"});
    d.save(&original).unwrap();
    assert!(d.next().unwrap().is_none());
    std::fs::set_permissions(&d.root, std::fs::Permissions::from_mode(0o500)).unwrap();
    let addition = d.add_info(1, "Cannot save this");
    let creation = d.save(&report('b', 2, 200));
    std::fs::set_permissions(&d.root, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert!(addition.is_err());
    assert!(creation.is_err());
    assert_eq!(stored(&d, 'a'), original);
    assert!(
        d.add_info(2, "Unsaved report has no active target")
            .is_err()
    );
    assert!(d.next().unwrap().is_none());
    std::fs::remove_dir_all(dir).unwrap();
}
