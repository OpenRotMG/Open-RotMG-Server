use super::*;

#[tokio::test]
async fn community_guild_mutual_publication_precedes_transaction_clock_pause() {
    let (dir, a) = super::super::account_actions::tests::fixture();
    let mut players = Vec::new();
    let mut receivers = Vec::new();
    let mut owners = Vec::new();
    for (guid, name) in [
        ("guild-native-alice", "Alice"),
        ("guild-native-bobby", "Bobby"),
        ("guild-native-carol", "Carol"),
    ] {
        let account = a.store.list(guid.into()).await.unwrap();
        a.store.rename(account.id, name.into()).await.unwrap();
        let ch = a
            .store
            .create(account.id, a.content.get(WIZARD).unwrap())
            .await
            .unwrap();
        let presentation = a.store.presentation(account.id).await.unwrap();
        owners.push(a.claim(account.id).unwrap());
        let bind = crate::server::lifecycle::binding(&a, account.id).unwrap();
        let (packets, mut rx) = mpsc::channel(256);
        let (lost, _) = tokio::sync::watch::channel(false);
        let id = a
            .world
            .in_world(-2)
            .call(move |w| {
                let pos = w
                    .entities
                    .values()
                    .find(|e| e.kind == 0x1df)
                    .context("Guild Register absent")?
                    .pos;
                let id = w.enter_at(
                    ch,
                    Some(crate::world::Output { packets, lost }),
                    Some(Pos {
                        x: pos.x + 1.,
                        y: pos.y,
                    }),
                    presentation,
                )?;
                bind(id)?;
                Ok(id)
            })
            .await
            .unwrap();
        while rx.try_recv().is_ok() {}
        players.push(id);
        receivers.push(rx);
    }
    let actors: [i32; 3] = players.try_into().unwrap();
    let alice = actors[0];
    let publication_start = Instant::now();
    let publication = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let observed = a.world.directory(move |d| {
                let world = d.location(alice)?;
                let w = &d.worlds[&world];
                let ready = actors.iter().all(|id| {
                    d.location(*id).ok() == Some(world) && d.guild_actor(*id).is_ok() && w.players.get(id).is_some_and(|p| actors.iter().all(|peer| peer == id || p.known.contains(peer)))
                });
                Ok(ready.then(|| serde_json::json!({"actors":actors.map(|id| {
                    let p = &w.players[&id];
                    serde_json::json!({"player":id,"account":p.character.account,"position":p.pos,"known_peers":actors.into_iter().filter(|peer| *peer != id && p.known.contains(peer)).collect::<Vec<_>>()})
                }),"world":w.name})))
            }).await.unwrap();
            if let Some(observed) = observed { break observed; }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }).await.expect("all guild participants must observe both peers before pausing within two seconds");
    println!(
        "guild initial mutual publication: {}",
        serde_json::json!({"snapshot":publication,"elapsed_wall_ms":publication_start.elapsed().as_millis(),"max_wall_ms":2000,"poll_ms":50})
    );
    a.world.clock.set_paused(true);
    for id in actors {
        crate::server::depart(&a, id).await.unwrap();
    }
    drop(owners);
    drop(receivers);
    drop(a);
    std::fs::remove_dir_all(dir).unwrap();
}
