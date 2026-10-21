use super::*;
use crate::bots::social::Interaction;

#[tokio::test]
async fn community_social_actual_healing_changes_requests_and_gifts_or_own_need_refuses() {
    for generous in [true, false] {
        let (dir, app) = crate::server::fixture();
        app.world.clock.set_paused(true);
        let log = fs::File::create(dir.join("help-runtime.jsonl")).unwrap();
        let sub = tracing_subscriber::fmt()
            .json()
            .with_max_level(tracing::Level::INFO)
            .with_writer(Mutex::new(log))
            .finish();
        let _logging = tracing::subscriber::set_default(sub);
        let mut initial = vec![];
        let mut accounts = vec![];
        for (i, p) in [PROFILES[3], PROFILES[1], PROFILES[0]]
            .into_iter()
            .enumerate()
        {
            let account = app.store.list(p.identity.into()).await.unwrap().id;
            let class = if i == 0 {
                crate::content::PRIEST
            } else {
                crate::content::WIZARD
            };
            let mut ch = app
                .store
                .create_prepared(
                    account,
                    app.content.get(class).unwrap(),
                    Some(p.name.into()),
                )
                .await
                .unwrap();
            if i == 0 {
                ch.base[1] = 300;
                ch.mp = 300;
                ch.items[1] = 2775;
                ch.items[4..].fill(-1);
                ch.items[4..if generous { 8 } else { 6 }].fill(0xa22);
            } else {
                ch.hp = 60;
                ch.items[4..].fill(-1);
            }
            ch.revision = app
                .store
                .exchange(
                    vec![InventoryWrite {
                        id: ch.id,
                        account,
                        revision: ch.revision,
                        items: ch.items.clone(),
                    }],
                    None,
                )
                .await
                .unwrap()[0];
            app.store.progress(ch.clone()).await.unwrap();
            accounts.push(account);
            initial.push(ch.clone());
            let selected = ch.id;
            launch(
                &app,
                p,
                ch,
                -2,
                Some(Pos {
                    x: 76.5 + i as f32 * 0.5,
                    y: 73.5,
                }),
                Some(selected),
            )
            .await
            .unwrap();
        }
        fs::write(dir.join("help-declaration.json"),serde_json::to_vec_pretty(&json!({"generous":generous,"initial":initial,"max_game_ms":30000,"max_wall_ms":75000,"humans":0,"autonomous":3,"selected":"actual prepared character per controller; not replacement proof","context":"prepared ready Priest, injured independently controlled recipients, held supplies only; no social memory/consent/requests/actions/outcomes supplied"})).unwrap()).unwrap();
        app.world.clock.set_paused(false);
        let start = Instant::now();
        loop {
            tokio::time::sleep(Duration::from_millis(100)).await;
            commits(&app).await;
            let sample=app.world.directory(|d|{
                let views=d.worlds.values().flat_map(|w|w.players.iter()).map(|(id,_)|{let v=d.player_view(*id)?;Ok(json!({"session":v.session,"position":v.position,"character":v.character,"map":v.map,"objects":v.objects,"lifecycle":v.lifecycle}))}).collect::<Result<Vec<_>>>()?;
                Ok(json!({"game_ms":d.tick*50,"views":views}))
            }).await.unwrap();
            use std::io::Write;
            let mut f = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(dir.join("help-sequence.jsonl"))
                .unwrap();
            writeln!(f, "{sample}").unwrap();
            let game = sample["game_ms"].as_u64().unwrap();
            let minds = app.store.bot_mind(accounts[0]).await.unwrap().unwrap();
            let mut received = false;
            for account in &accounts[1..] {
                received |= app
                    .store
                    .bot_mind(*account)
                    .await
                    .unwrap()
                    .unwrap()
                    .social
                    .episodes
                    .iter()
                    .any(|e| e.interaction == Interaction::ReceivedHealth);
            }
            let satisfied = if generous {
                received
            } else {
                minds
                    .social
                    .episodes
                    .iter()
                    .any(|e| e.interaction == Interaction::Refused)
            };
            if (satisfied && game >= 8000)
                || game >= 30000
                || start.elapsed() > Duration::from_secs(75)
            {
                break;
            }
        }
        stop(&app).await;
        let mut minds = vec![];
        let mut durable = vec![];
        for (account, ch) in accounts.iter().zip(&initial) {
            minds.push(app.store.bot_mind(*account).await.unwrap().unwrap());
            durable.push(app.store.load(*account, ch.id).await.unwrap());
        }
        fs::write(dir.join("help-result.json"),serde_json::to_vec_pretty(&json!({"minds":minds,"durable":durable,"cleanup":{"owners":0,"players":0,"quarantined":0}})).unwrap()).unwrap();
        println!(
            "social help evidence generous={generous}: {}",
            dir.display()
        );
        assert!(
            minds[1..].iter().any(|m| m.social.known(accounts[0])),
            "real healing was not remembered: {}",
            dir.display()
        );
        assert!(
            durable[1..].iter().any(|c| c.hp > 60),
            "no actual HP benefit"
        );
        if generous {
            assert!(
                minds[1..].iter().any(|m| m
                    .social
                    .episodes
                    .iter()
                    .any(|e| e.interaction == Interaction::ReceivedHealth)),
                "no gift receipt: {}",
                dir.display()
            );
            assert!(
                minds[0]
                    .social
                    .episodes
                    .iter()
                    .any(|e| e.interaction == Interaction::GaveHealth),
                "giver failed own committed receipt"
            );
        } else {
            assert!(
                minds[0]
                    .social
                    .episodes
                    .iter()
                    .any(|e| e.interaction == Interaction::Refused),
                "no selective refusal: {}",
                dir.display()
            );
            assert!(minds[1..].iter().all(|m| {
                m.social
                    .episodes
                    .iter()
                    .all(|e| e.interaction != Interaction::ReceivedHealth)
            }));
        }
        let before: usize = initial
            .iter()
            .map(|c| c.items.iter().filter(|k| **k == 0xa22).count())
            .sum();
        let after: usize = durable
            .iter()
            .map(|c| c.items.iter().filter(|k| **k == 0xa22).count())
            .sum();
        assert_eq!(before, after, "gift conserves held health");
    }
}
