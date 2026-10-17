use super::*;

pub(super) struct ChatBudget {
    updated: Instant,
    tokens: f32,
}

pub(crate) const ANNOUNCEMENT: &str = "@ANNOUNCEMENT";

pub(crate) fn chat_private(text: &str) -> Packet {
    text_packet("*Discord*", -1, 0, text)
}

pub(crate) fn chat_error(reason: &str) -> Packet {
    text_packet("*Error*", -1, 0, reason)
}

pub(crate) fn chat_info(text: &str) -> Packet {
    text_packet("", -1, 0, text)
}

pub(super) fn text_packet(name: &str, object: i32, bubble: u8, text: &str) -> Packet {
    text_to(name, object, bubble, "", text, 0)
}

fn text_to(name: &str, object: i32, bubble: u8, recipient: &str, text: &str, stars: i32) -> Packet {
    let mut out = Writer::new();
    out.utf(name)
        .i32(object)
        .i32(stars)
        .u8(bubble)
        .utf(recipient)
        .utf(text)
        .utf("");
    Packet {
        id: wire::TEXT,
        body: out.0,
    }
}

pub(crate) fn validate(text: &str) -> Result<()> {
    ensure!(!text.trim().is_empty(), "Chat message is empty.");
    ensure!(
        text.encode_utf16().count() <= 128,
        "Chat messages are limited to 128 characters."
    );
    ensure!(
        !text
            .chars()
            .any(|c| c.is_control()
                || matches!(c, '\u{2028}'..='\u{202e}' | '\u{2066}'..='\u{2069}')),
        "Chat message contains control characters."
    );
    Ok(())
}

impl Player {
    fn can_chat(&self) -> bool {
        self.character.alive
            && self.character.hp > 0
            && !self.death_pending
            && !self.departing
            && self.persistence_failure.is_none()
    }
}

impl World {
    pub(crate) fn dialogue_participants(&self, bot: i32, speaker: i32) -> Result<()> {
        let sender = self.players.get(&bot).context("dialogue bot left world")?;
        let peer = self
            .players
            .get(&speaker)
            .context("dialogue speaker left world")?;
        ensure!(
            sender.controller == Controller::Autonomous
                && sender.can_chat()
                && peer.can_chat()
                && sender.known.contains(&speaker)
                && !peer.account_lists[1].contains(&sender.character.account)
                && sender.pos.distance(peer.pos) <= 12.,
            "dialogue participants unavailable"
        );
        Ok(())
    }
    pub(crate) fn dialogue_notice(&self, speaker: i32, text: &str, failed: bool) -> Result<()> {
        validate(text)?;
        let p = self
            .players
            .get(&speaker)
            .context("dialogue recipient left world")?;
        ensure!(p.can_chat(), "dialogue recipient unavailable");
        let packet = if failed {
            chat_error(text)
        } else {
            chat_info(text)
        };
        send(p, packet.id, packet.body);
        Ok(())
    }
    pub fn account_list_target(&self, id: i32, target: i32) -> Result<i32> {
        let sender = self.players.get(&id).context("player missing")?;
        let target_player = self.players.get(&target).context("player not found")?;
        ensure!(
            sender.can_chat()
                && target_player.can_chat()
                && id != target
                && sender.known.contains(&target),
            "account list target is not an available nearby player"
        );
        Ok(target_player.character.account)
    }

    pub fn sync_account_lists(&mut self, id: i32, lists: Option<AccountLists>) -> Result<()> {
        let player = self.players.get_mut(&id).context("player missing")?;
        if let Some(lists) = lists {
            player.account_lists = lists;
        }
        for (list, accounts) in player.account_lists.iter().enumerate() {
            let mut out = Writer::new();
            out.i32(list as i32).i16(i16::try_from(accounts.len())?);
            for account in accounts {
                out.i32(*account);
            }
            send(player, wire::ACCOUNT_LIST, out.0);
        }
        Ok(())
    }
}

impl Directory {
    pub fn chat_permit(&mut self, id: i32, now: Instant) -> Result<()> {
        let sender = &self.resolve(Target::Player(id))?.players[&id];
        ensure!(
            sender.can_chat(),
            "Dead, departing or quarantined players cannot chat."
        );
        let account = sender.character.account;
        // Account budgets survive world transfers and reconnects without touching durable game saves.
        self.chat_budgets
            .retain(|_, b| now.saturating_duration_since(b.updated).as_secs() < 3);
        ensure!(
            self.chat_budgets.len() < 4096 || self.chat_budgets.contains_key(&account),
            "Chat rate registry is full."
        );
        let budget = self.chat_budgets.entry(account).or_insert(ChatBudget {
            updated: now,
            tokens: 3.,
        });
        let elapsed = now
            .checked_duration_since(budget.updated)
            .context("chat receive clock regressed")?;
        budget.tokens = (budget.tokens + elapsed.as_secs_f32()).min(3.);
        budget.updated = now;
        ensure!(
            budget.tokens >= 1.,
            "Chat rate limit: wait one second before sending again."
        );
        budget.tokens -= 1.;
        Ok(())
    }

    pub fn chat(&mut self, id: i32, text: String, now: Instant) -> Result<()> {
        validate(&text)?;
        let trimmed = text.trim();
        let (command, rest) = trimmed
            .split_once(char::is_whitespace)
            .unwrap_or((trimmed, ""));
        let who = command.eq_ignore_ascii_case("/who");
        let teleport = command.eq_ignore_ascii_case("/teleport");
        let trade = command.eq_ignore_ascii_case("/trade");
        let yell = command.eq_ignore_ascii_case("/yell");
        let tell = command.eq_ignore_ascii_case("/tell") || command.eq_ignore_ascii_case("/t");
        let guild_chat =
            command.eq_ignore_ascii_case("/g") || command.eq_ignore_ascii_case("/guild");
        ensure!(
            !trimmed.starts_with('/')
                || tell
                || who
                || teleport
                || trade
                || yell
                || guild_chat
                || trimmed.eq_ignore_ascii_case("/pause"),
            "Unknown chat command."
        );
        let world = self.resolve(Target::Player(id))?;
        let sender = &world.players[&id];
        ensure!(
            sender.can_chat(),
            "Dead, departing or quarantined players cannot chat."
        );
        if text.trim().eq_ignore_ascii_case("/pause") {
            ensure!(
                sender.paused
                    || !world.entities.values().any(|e| e.hp > 0
                        && world.content.objects[&e.kind].enemy
                        && sender.pos.distance(e.pos) <= 8.),
                "Not safe enough to pause!"
            );
            let p = world.players.get_mut(&id).unwrap();
            p.paused = !p.paused;
            p.last_move_at = world.now;
            let text = format!("Game {}.", if p.paused { "paused" } else { "resumed" });
            let reply = text_packet("", -1, 0, &text);
            send(p, reply.id, reply.body);
            return Ok(());
        }
        let account = sender.character.account;
        let name = sender.character.name.clone();
        let guild = sender.presentation.guild.clone();
        let stars = sender.presentation.stars();
        ensure!(
            !tell || crate::store::name_chosen(account, &name),
            "Choose a name before using /tell."
        );
        self.chat_permit(id, now)?;
        if trade {
            let name = rest.trim();
            ensure!(
                !name.is_empty() && !name.contains(char::is_whitespace),
                "Usage: /trade <player name>"
            );
            return self
                .resolve(Target::Player(id))?
                .request_trade(id, name.into());
        }
        if teleport {
            let name = rest.trim();
            ensure!(
                !name.is_empty() && !name.contains(char::is_whitespace),
                "Usage: /teleport <player name>"
            );
            let target = self
                .resolve(Target::Player(id))?
                .players
                .iter()
                .find(|(_, p)| p.character.name.eq_ignore_ascii_case(name))
                .map(|(id, _)| *id)
                .context("Player not found in this world.")?;
            self.teleport(id, target, now)?;
            return Ok(());
        }
        if who {
            ensure!(rest.trim().is_empty(), "Usage: /who");
            // Period /who lists the current realm, not the cross-world tell/guild audience.
            let world = self.resolve(Target::Player(id))?;
            let names: Vec<_> = world
                .players
                .values()
                .filter(|p| p.can_chat())
                .map(|p| p.character.name.as_str())
                .collect();
            let packet = chat_info(&format!("Players online: {}", names.join(", ")));
            send(&world.players[&id], packet.id, packet.body);
            tracing::info!(player=id, account, world=%world.name, players=names.len(), "who roster sent");
            return Ok(());
        }
        if guild_chat {
            let guild = guild.context("You are not in a guild.")?;
            let message = rest.trim();
            ensure!(!message.is_empty(), "Usage: /g <message>");
            let packet = text_to(&name, -1, 0, "*Guild*", message, stars);
            let mut recipients = 0;
            for p in self
                .worlds
                .values()
                .flat_map(|w| w.players.values())
                .filter(|p| {
                    p.can_chat()
                        && p.presentation
                            .guild
                            .as_ref()
                            .is_some_and(|g| g.id == guild.id)
                        && !p.account_lists[1].contains(&account)
                })
            {
                send(p, packet.id, packet.body.clone());
                recipients += 1;
            }
            tracing::info!(
                player = id,
                account,
                guild = guild.id,
                recipients,
                bytes = message.len(),
                "guild chat delivered"
            );
            return Ok(());
        }
        if tell {
            let (recipient, message) = rest
                .trim_start()
                .split_once(char::is_whitespace)
                .filter(|(_, message)| !message.trim().is_empty())
                .context("Usage: /tell <player> <message>")?;
            let target = self
                .worlds
                .values()
                .flat_map(|w| w.players.values())
                .find(|p| p.can_chat() && p.character.name.eq_ignore_ascii_case(recipient))
                .context("Player not found.")?;
            ensure!(
                target.character.account != account,
                "You cannot tell yourself."
            );
            let packet = text_to(&name, -1, 0, &target.character.name, message.trim(), 0);
            let ignored = target.account_lists[1].contains(&account);
            if !ignored {
                send(target, packet.id, packet.body.clone());
            }
            let sender = &self.resolve(Target::Player(id))?.players[&id];
            send(sender, packet.id, packet.body);
            tracing::info!(
                player = id,
                account,
                delivered = !ignored,
                bytes = message.len(),
                "private chat processed"
            );
            return Ok(());
        }
        let text = if yell {
            // Period sources establish the Nexus audience, not a cross-world relay.
            ensure!(
                self.resolve(Target::World(-2))?.players.contains_key(&id),
                "You can only /yell in the Nexus."
            );
            let message = rest.trim();
            ensure!(!message.is_empty(), "Usage: /yell <message>");
            message
        } else {
            &text
        };
        let packet = text_packet(&name, id, 5, text);
        let world = self.resolve(Target::Player(id))?;
        world.ocean_response(id, text)?;
        let mut recipients = 0;
        for player in world
            .players
            .values()
            .filter(|p| p.can_chat() && !p.account_lists[1].contains(&account))
        {
            send(player, packet.id, packet.body.clone());
            recipients += 1;
        }
        tracing::info!(player=id, account, world=%world.name, recipients, bytes=text.len(), "local chat delivered");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::Reader;

    fn receive(d: &mut Directory, world: i32, account: i32) -> (i32, mpsc::Receiver<Packet>) {
        let w = d.worlds.get_mut(&world).unwrap();
        let mut ch = super::super::tests::ch(w);
        ch.id = account;
        ch.account = account;
        ch.name = format!("Guest{account}");
        let (packets, mut rx) = mpsc::channel(64);
        let (lost, _) = watch::channel(false);
        let id = w.enter(ch, Some(Output { packets, lost })).unwrap();
        while rx.try_recv().is_ok() {}
        (id, rx)
    }
    fn message(packet: Packet, name: &str, id: i32, text: &str) {
        assert_eq!(packet.id, wire::TEXT);
        let mut r = Reader::new(&packet.body);
        assert_eq!(r.utf().unwrap(), name);
        assert_eq!(r.i32().unwrap(), id);
        assert_eq!(r.i32().unwrap(), 0);
        assert_eq!(r.u8().unwrap(), if id < 0 { 0 } else { 5 });
        assert_eq!(r.utf().unwrap(), "");
        assert_eq!(r.utf().unwrap(), text);
        assert_eq!(r.utf().unwrap(), "");
        r.done().unwrap();
    }
    #[test]
    fn dialogue_errors_are_private_and_unavailable_participants_cannot_accept() {
        let mut d = super::super::tests::directory();
        let (bot, mut bot_rx) = receive(&mut d, -2, 1);
        let (human, mut human_rx) = receive(&mut d, -2, 2);
        let (_, mut other_rx) = receive(&mut d, -2, 3);
        let w = d.worlds.get_mut(&-2).unwrap();
        w.dialogue_notice(human, "Bot dialogue failed: timeout.", true)
            .unwrap();
        message(
            human_rx.try_recv().unwrap(),
            "*Error*",
            -1,
            "Bot dialogue failed: timeout.",
        );
        assert!(bot_rx.try_recv().is_err() && other_rx.try_recv().is_err());
        assert!(w.dialogue_participants(bot, human).is_err());
        let position = w.players[&human].pos;
        let p = w.players.get_mut(&bot).unwrap();
        p.controller = Controller::Autonomous;
        p.known.insert(human);
        p.pos = position;
        w.dialogue_participants(bot, human).unwrap();
        w.players.get_mut(&human).unwrap().departing = true;
        assert!(w.dialogue_participants(bot, human).is_err());
        assert!(w.dialogue_notice(human, "No stale reply.", true).is_err());
    }
    #[test]
    fn who_lists_only_available_current_world_players_privately() {
        let mut d = super::super::tests::directory();
        let now = Instant::now();
        let (a, mut ar) = receive(&mut d, -2, 1);
        let (b, mut br) = receive(&mut d, -2, 2);
        let (_, mut remote) = receive(&mut d, -3, 3);
        let w = d.worlds.get_mut(&-2).unwrap();
        w.players.get_mut(&b).unwrap().pos.x += 100.;
        w.players.get_mut(&b).unwrap().paused = true;
        w.players.get_mut(&a).unwrap().account_lists[1].insert(2);
        d.chat(a, " /WhO ".into(), now).unwrap();
        message(
            ar.try_recv().unwrap(),
            "",
            -1,
            "Players online: Guest1, Guest2",
        );
        for mode in 0..5 {
            let p = d.worlds.get_mut(&-2).unwrap().players.get_mut(&b).unwrap();
            p.character.alive = mode != 0;
            p.character.hp = if mode == 1 { 0 } else { 100 };
            p.death_pending = mode == 2;
            p.departing = mode == 3;
            p.persistence_failure = (mode == 4).then(|| "uncertain save".into());
            d.chat(
                a,
                "/who".into(),
                now + std::time::Duration::from_secs(mode + 1),
            )
            .unwrap();
            message(ar.try_recv().unwrap(), "", -1, "Players online: Guest1");
        }
        assert!(ar.try_recv().is_err() && br.try_recv().is_err() && remote.try_recv().is_err());
    }
    #[test]
    fn selected_chat_layout_identity_and_world_routing() {
        let mut d = super::super::tests::directory();
        let (a, mut ar) = receive(&mut d, -2, 1);
        let (_, mut br) = receive(&mut d, -2, 2);
        let (_, mut other) = receive(&mut d, -3, 3);
        d.chat(a, "<Guest2>: café & hello".into(), Instant::now())
            .unwrap();
        message(
            ar.try_recv().unwrap(),
            "Guest1",
            a,
            "<Guest2>: café & hello",
        );
        message(
            br.try_recv().unwrap(),
            "Guest1",
            a,
            "<Guest2>: café & hello",
        );
        assert!(other.try_recv().is_err());
        message(chat_error("Wait."), "*Error*", -1, "Wait.");
    }
    #[test]
    fn lifecycle_and_world_changes_do_not_replay_or_leak_chat() {
        let mut d = super::super::tests::directory();
        let now = Instant::now();
        let (a, mut ar) = receive(&mut d, -2, 1);
        let (b, mut br) = receive(&mut d, -2, 2);
        d.worlds
            .get_mut(&-2)
            .unwrap()
            .players
            .get_mut(&b)
            .unwrap()
            .departing = true;
        d.chat(a, "old world".into(), now).unwrap();
        assert!(ar.try_recv().is_ok());
        assert!(br.try_recv().is_err());
        d.worlds.get_mut(&-2).unwrap().players.remove(&b);
        let (b, mut br) = receive(&mut d, -3, 2);
        assert!(br.try_recv().is_err());
        d.chat(b, "new world".into(), now).unwrap();
        assert!(br.try_recv().is_ok());
        assert!(ar.try_recv().is_err());
        for mode in 0..3 {
            let p = d.worlds.get_mut(&-3).unwrap().players.get_mut(&b).unwrap();
            p.character.alive = mode != 0;
            p.death_pending = mode == 1;
            p.persistence_failure = (mode == 2).then(|| "uncertain save".into());
            assert!(d.chat(b, "/who".into(), now).is_err());
            assert!(br.try_recv().is_err());
        }
    }
    #[test]
    fn chat_validation_utf16_boundary_and_commands_never_broadcast() {
        let mut d = super::super::tests::directory();
        let (a, mut rx) = receive(&mut d, -2, 1);
        for text in [
            "",
            "  ",
            "\nGuest2: spoof",
            "hello\0",
            "\u{202e}spoof",
            "\u{2028}new line",
            " /tell Guest2 hi",
            "/g hi",
        ] {
            assert!(d.chat(a, text.into(), Instant::now()).is_err());
            assert!(rx.try_recv().is_err());
        }
        assert!(validate(&"a".repeat(129)).is_err());
        assert!(validate(&"😀".repeat(65)).is_err());
        validate(&"😀".repeat(64)).unwrap();
        validate(&"a".repeat(128)).unwrap();
    }
    #[test]
    fn tells_cross_worlds_without_leaking_or_spoofing_and_share_public_throttle() {
        let mut d = super::super::tests::directory();
        let now = Instant::now();
        let (a, mut ar) = receive(&mut d, -2, 1);
        let (_, mut spectator) = receive(&mut d, -2, 2);
        let (b, mut br) = receive(&mut d, -3, 3);
        let (_, mut remote_spectator) = receive(&mut d, -3, 4);
        assert!(d.chat(a, "/tell Guest3 unnamed".into(), now).is_err());
        assert!(ar.try_recv().is_err());
        assert!(br.try_recv().is_err());
        d.worlds
            .get_mut(&-2)
            .unwrap()
            .players
            .get_mut(&a)
            .unwrap()
            .character
            .name = "Alice".into();
        let body = "<Guest4>: /tell Guest2 private";
        d.chat(a, format!("/TeLl gUeSt3 {body}"), now).unwrap();
        for packet in [ar.try_recv().unwrap(), br.try_recv().unwrap()] {
            let mut r = Reader::new(&packet.body);
            assert_eq!(packet.id, wire::TEXT);
            assert_eq!(r.utf().unwrap(), "Alice");
            assert_eq!(r.i32().unwrap(), -1);
            assert_eq!(r.i32().unwrap(), 0);
            assert_eq!(r.u8().unwrap(), 0);
            assert_eq!(r.utf().unwrap(), "Guest3");
            assert_eq!(r.utf().unwrap(), body);
            assert_eq!(r.utf().unwrap(), "");
            r.done().unwrap();
        }
        assert!(spectator.try_recv().is_err());
        assert!(remote_spectator.try_recv().is_err());
        d.worlds
            .get_mut(&-3)
            .unwrap()
            .players
            .get_mut(&b)
            .unwrap()
            .account_lists[1]
            .insert(1);
        d.chat(a, "/t Guest3 ignored".into(), now).unwrap();
        assert!(ar.try_recv().is_ok());
        assert!(br.try_recv().is_err());
        d.chat(a, "public".into(), now).unwrap();
        assert!(ar.try_recv().is_ok());
        assert!(spectator.try_recv().is_ok());
        assert!(d.chat(a, "/tell Guest3 throttled".into(), now).is_err());
        assert!(br.try_recv().is_err());
        for text in [
            "/tell",
            "/tell Guest3",
            "/tell Guest3  ",
            "/tell nobody hi",
            "/tell Alice hi",
            "/t Guest3 hi\nspoof",
        ] {
            assert!(
                d.chat(a, text.into(), now + std::time::Duration::from_secs(10))
                    .is_err()
            );
        }
        assert!(ar.try_recv().is_err());
        assert!(spectator.try_recv().is_err());
        assert!(remote_spectator.try_recv().is_err());
    }

    #[test]
    fn account_list_packets_use_accounts_and_ignores_filter_local_chat() {
        let mut d = super::super::tests::directory();
        let (a, mut ar) = receive(&mut d, -2, 1);
        let (b, mut br) = receive(&mut d, -2, 2);
        let (c, _) = receive(&mut d, -3, 3);
        let w = d.worlds.get_mut(&-2).unwrap();
        assert!(w.account_list_target(a, a).is_err());
        assert!(w.account_list_target(a, c).is_err());
        assert!(w.account_list_target(a, b).is_err());
        w.players.get_mut(&a).unwrap().known.insert(b);
        assert_eq!(w.account_list_target(a, b).unwrap(), 2);
        w.sync_account_lists(b, Some([BTreeSet::from([1]), BTreeSet::from([1])]))
            .unwrap();
        for list in 0..2 {
            let p = br.try_recv().unwrap();
            assert_eq!(p.id, wire::ACCOUNT_LIST);
            let mut r = Reader::new(&p.body);
            assert_eq!(r.i32().unwrap(), list);
            assert_eq!(r.i16().unwrap(), 1);
            assert_eq!(r.i32().unwrap(), 1);
            r.done().unwrap();
        }
        let now = Instant::now();
        d.chat(a, "ignored".into(), now).unwrap();
        assert!(ar.try_recv().is_ok());
        assert!(br.try_recv().is_err());
        d.worlds
            .get_mut(&-2)
            .unwrap()
            .sync_account_lists(b, Some([BTreeSet::from([1]), BTreeSet::new()]))
            .unwrap();
        while br.try_recv().is_ok() {}
        d.chat(a, "unignored".into(), now).unwrap();
        message(br.try_recv().unwrap(), "Guest1", a, "unignored");
    }

    #[test]
    fn chat_throttle_survives_transfers_reconnects_and_rejects_departed_senders() {
        let mut d = super::super::tests::directory();
        let now = Instant::now();
        let (a, mut rx) = receive(&mut d, -2, 1);
        for text in ["hi", "/who", "hi"] {
            d.chat(a, text.into(), now).unwrap();
        }
        assert!(d.chat(a, "burst".into(), now).is_err());
        assert_eq!(std::iter::from_fn(|| rx.try_recv().ok()).count(), 3);
        let p = d.worlds.get_mut(&-2).unwrap().players.get_mut(&a).unwrap();
        p.departing = true;
        assert!(d.chat(a, "/who".into(), now).is_err());
        d.worlds.get_mut(&-2).unwrap().players.remove(&a);
        assert!(d.chat(a, "/who".into(), now).is_err());
        let (b, mut rx) = receive(&mut d, -3, 1);
        assert!(d.chat(b, "reconnect".into(), now).is_err());
        assert!(rx.try_recv().is_err());
        d.chat(
            b,
            "refilled".into(),
            now + std::time::Duration::from_secs(1),
        )
        .unwrap();
        let p = d.worlds.get_mut(&-3).unwrap().players.get_mut(&b).unwrap();
        p.death_pending = true;
        assert!(
            d.chat(b, "dead".into(), now + std::time::Duration::from_secs(2))
                .is_err()
        );
    }
}
