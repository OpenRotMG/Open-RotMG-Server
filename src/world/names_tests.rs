use super::*;
use crate::world::tests::{ch, world};

#[test]
fn name_publication_preserves_pending_death_progress_and_departure() {
    for lifecycle in 0..4 {
        let mut w = world();
        let id = w.enter(ch(&w), None).unwrap();
        w.players.get_mut(&id).unwrap().presentation.credits = 1000;
        let plan = w.begin_name_choice(id, 1).unwrap().unwrap();
        assert_eq!(w.players[&id].character.name, "Test");
        assert_eq!(w.players[&id].presentation.credits, 1000);
        match lifecycle {
            1 => {
                let p = w.players.get_mut(&id).unwrap();
                p.death_pending = true;
                p.character.hp = 0;
                assert!(w.pending_deaths().is_empty());
            }
            2 => {
                w.players.get_mut(&id).unwrap().experience_queued = 50;
                assert!(w.pending_progress().is_empty());
            }
            3 => {
                assert!(w.begin_departure(id).unwrap().is_none());
            }
            _ => {}
        }
        assert_eq!(
            w.finish_name_choice(
                plan,
                "After".into(),
                Ok(NameChoice::Committed { balance: 0 })
            )
            .unwrap(),
            None
        );
        let p = &w.players[&id];
        assert_eq!(p.character.name, "After");
        assert_eq!(p.presentation.credits, 0);
        assert!(!p.pending);
        assert!(p.persistence_failure.is_none());
        match lifecycle {
            1 => {
                assert!(w.players[&id].death_pending);
                assert_eq!(w.players[&id].character.hp, 0);
                let deaths = w.pending_deaths();
                assert_eq!(deaths.len(), 1);
                assert_eq!(deaths[0].1.name, "After");
                assert!(!deaths[0].1.alive);
            }
            2 => {
                let progress = w.pending_progress();
                assert_eq!(progress.len(), 1);
                assert_eq!(progress[0].1.name, "After");
            }
            3 => {
                assert!(w.players[&id].departing);
                let snapshot = w.begin_departure(id).unwrap().unwrap();
                assert_eq!(snapshot.name, "After");
            }
            _ => {}
        }
    }
}

#[test]
fn name_user_rejections_release_only_their_reservation_but_failures_quarantine() {
    for failure in 0..4 {
        let mut w = world();
        let id = w.enter(ch(&w), None).unwrap();
        w.players.get_mut(&id).unwrap().presentation.credits = 1000;
        let plan = w.begin_name_choice(id, 1).unwrap().unwrap();
        let outcome = match failure {
            0 => Ok(NameChoice::Rejected("name already taken")),
            1 => Err(anyhow::anyhow!("SQLite commit failed")),
            2 => {
                w.players.get_mut(&id).unwrap().character.revision += 1;
                Ok(NameChoice::Committed { balance: 0 })
            }
            _ => {
                w.players.get_mut(&id).unwrap().character.account = 2;
                Ok(NameChoice::Committed { balance: 0 })
            }
        };
        let result = w.finish_name_choice(plan, "After".into(), outcome);
        let p = &w.players[&id];
        assert_eq!(p.character.name, "Test");
        assert_eq!(p.presentation.credits, 1000);
        if failure == 0 {
            assert_eq!(result.unwrap(), Some("name already taken"));
            assert!(!p.pending);
            assert!(p.persistence_failure.is_none());
        } else {
            assert!(result.is_err());
            assert!(p.pending);
            assert!(p.persistence_failure.is_some());
            assert!(w.begin_departure(id).is_err());
        }
    }
}

#[test]
fn name_reservation_rejects_foreign_busy_terminal_and_trading_without_side_effects() {
    let mut masked = Vec::new();
    for reason in 0..8 {
        let mut w = world();
        let id = w.enter(ch(&w), None).unwrap();
        match reason {
            0 => {}
            1 => w.players.get_mut(&id).unwrap().pending = true,
            2 => w.players.get_mut(&id).unwrap().death_pending = true,
            3 => w.players.get_mut(&id).unwrap().departing = true,
            4 => {
                w.players.get_mut(&id).unwrap().persistence_failure =
                    Some("existing failure".into())
            }
            _ => {
                let mut other = ch(&w);
                other.id = 2;
                other.account = 2;
                other.name = "Peer".into();
                let peer = w.enter(other, None).unwrap();
                w.request_trade(id, "Peer".into()).unwrap();
                w.request_trade(peer, "Test".into()).unwrap();
                assert!(w.trade_key(id).is_some());
            }
        }
        if reason == 7 {
            w.players.get_mut(&id).unwrap().persistence_failure =
                Some("existing failure while trading".into());
        }
        let result = w.begin_name_choice(id, if matches!(reason, 0 | 6) { 2 } else { 1 });
        if matches!(reason, 0 | 4 | 6 | 7) {
            if result.is_ok() {
                masked.push(reason);
            }
        } else {
            assert!(result.unwrap().is_err());
        }
        assert_eq!(w.players[&id].character.name, "Test");
        if reason >= 5 {
            assert!(w.trade_key(id).is_some());
            assert!(!w.players[&id].pending);
        }
    }
    assert!(
        masked.is_empty(),
        "authority/persistence failures masked as user rejections: {masked:?}"
    );
}
