#[tokio::test]
async fn restart_does_not_hide_incident_write_errors() {
    let (dir, app) = super::super::account_actions::tests::fixture();
    let path = dir.join("incident.json");
    let supervisor = crate::supervision::Supervisor::recovered(path.clone(), &app.store).unwrap();
    supervisor.failure("fixture", anyhow::anyhow!("injected worker failure"));
    assert!(crate::supervision::Supervisor::recovered(path.clone(), &app.store).is_ok());
    std::fs::create_dir(path.with_extension("json.new")).unwrap();
    let fresh = crate::supervision::Supervisor::recovered(path, &app.store).unwrap();
    fresh.failure(
        "fixture-write-error",
        anyhow::anyhow!("injected worker failure"),
    );
    assert!(fresh.ensure_running().is_err());
    let failure = tokio::time::timeout(std::time::Duration::from_secs(1), fresh.failed())
        .await
        .unwrap();
    assert_eq!(failure.component, "fixture-write-error");
}
