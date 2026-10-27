use super::*;

fn fixture() -> (Content, PlayerView) {
    let (content, mut v) = crate::bots::quest_tests::fixture();
    v.character.class = PRIEST;
    v.character.items[0] = 0xa04;
    v.character.items[1] = 0xa31;
    let p = &mut v.objects[0];
    p.account = Some(2);
    p.kind = crate::content::WIZARD;
    p.name = "Patient".into();
    p.equipped = vec![-1; 4];
    p.position = v.position;
    p.hp = 200;
    p.max_hp = 500;
    (content, v)
}
fn request(text: &str, recipient: &str) -> Packet {
    let mut w = wire::Writer::new();
    w.utf("Patient")
        .i32(2)
        .i32(0)
        .u8(5)
        .utf(recipient)
        .utf(text)
        .utf("");
    Packet {
        id: wire::TEXT,
        body: w.0,
    }
}

#[test]
fn priest_heal_chat_routes_only_real_visible_requests() {
    let (content, mut v) = fixture();
    for (text, recipient, accepted) in [
        ("heal", "", true),
        (" HEAL ", "", true),
        ("heal pls", "", true),
        ("heal please", "Priest", true),
        ("Priest, heal please", "", true),
        ("Priest, heal", "", true),
        ("Other, heal", "", false),
        ("heal", "Other", false),
        ("don't heal", "", false),
        ("thanks for the heal", "", false),
    ] {
        let mut life = Life::default();
        assert!(life.healing(&v, &content, 0.).unwrap().is_none());
        assert_eq!(
            life.receive(&request(text, recipient), &v, "Priest", 0., false)
                .unwrap(),
            accepted,
            "{text}"
        );
        assert_eq!(
            life.healing(&v, &content, 0.).unwrap().is_some(),
            accepted,
            "{text}"
        );
    }
    for invalid in 0..4 {
        let mut life = Life::default();
        v.objects[0].hp = if invalid == 0 { 0 } else { 200 };
        v.objects[0].account = (invalid != 1).then_some(2);
        v.objects[0].name = if invalid == 2 { "Impostor" } else { "Patient" }.into();
        v.objects[0].equipped = if invalid == 3 { vec![] } else { vec![-1; 4] };
        assert!(
            !life
                .receive(&request("heal", ""), &v, "Priest", 0., false)
                .unwrap()
        );
    }
}

#[test]
fn priest_heal_requests_preserve_tome_limits_and_expire() {
    let (content, mut v) = fixture();
    let mut life = Life::default();
    assert!(
        life.receive(&request("heal", ""), &v, "Priest", 0., false)
            .unwrap()
    );
    assert!(matches!(
        life.healing(&v, &content, 0.).unwrap(),
        Some(Action::Use { slot: 1, .. })
    ));
    for gate in 0..8 {
        let (_, mut blocked) = fixture();
        match gate {
            0 => blocked.character.mp = 119,
            1 => blocked.character.ability_ready_ms = blocked.time_ms + 1,
            2 => blocked.character.conditions = crate::condition::bit(crate::condition::QUIET),
            3 => blocked.objects[0].position.x += 6.,
            4 => blocked.objects[0].hp = 500,
            5 => blocked.elapsed_ms += 10000,
            6 => blocked.character.class = crate::content::WIZARD,
            7 => blocked.character.items[1] = -1,
            _ => unreachable!(),
        }
        assert!(
            life.healing(&blocked, &content, 0.).unwrap().is_none(),
            "gate {gate}"
        );
    }
    life.bind();
    assert!(life.healing(&v, &content, 0.).unwrap().is_none());
    v.character.hp = 300;
    assert!(life.healing(&v, &content, 0.).unwrap().is_some());
}
