use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Source {
    Dialogue,
    Market,
    Gift,
    Call,
}

pub(super) struct Draft {
    source: Source,
    text: String,
    buffer: String,
    paste: bool,
    next: u64,
    opened: u64,
    revision: i32,
}

impl Brain {
    pub(super) async fn compose(
        &mut self,
        app: &crate::server::App,
        v: &PlayerView,
        source: Source,
        text: &str,
    ) -> Result<bool> {
        ensure!(
            !text.is_empty()
                && text.encode_utf16().count() <= 128
                && !text.trim_start().starts_with('/')
                && !text.chars().any(char::is_control),
            "invalid public composition"
        );
        if let Some(draft) = &self.draft {
            return Ok(draft.source == source
                && draft.text == text
                && draft.buffer == draft.text
                && v.tick >= draft.next);
        }
        self.draft = Some(Draft {
            source,
            text: text.into(),
            buffer: String::new(),
            paste: source == Source::Market && self.clipboard == text,
            next: v.tick + 2,
            opened: v.tick,
            revision: v.character.inventory_revision,
        });
        self.advance_composition(app, v).await?;
        Ok(false)
    }

    pub(super) fn composing(&self, source: Source) -> Option<&str> {
        self.draft
            .as_ref()
            .filter(|d| d.source == source)
            .map(|d| d.text.as_str())
    }

    pub(super) async fn close_composition(
        &mut self,
        app: &crate::server::App,
        sent: bool,
    ) -> Result<()> {
        let Some(draft) = self.draft.take() else {
            return Ok(());
        };
        if draft.source == Source::Call {
            self.call_goal = None;
            self.aftermath = None;
        }
        if draft.source == Source::Market {
            if sent {
                self.clipboard.clone_from(&draft.text);
            } else {
                self.mind.market.outgoing = None;
            }
            self.market_words = None;
        }
        if !sent && draft.source == Source::Gift {
            self.mind.social.cancel_composition();
        }
        let id = self.session.1;
        app.world
            .for_player(id)
            .call(move |w| {
                if let Some(inputs) = w.players.get_mut(&id).and_then(|p| p.inputs.as_mut()) {
                    inputs.composition = Default::default();
                }
                Ok(())
            })
            .await?;
        tracing::info!(player=id, source=?draft.source, sent, "bot chat composer closed");
        Ok(())
    }

    pub(super) async fn advance_composition(
        &mut self,
        app: &crate::server::App,
        v: &PlayerView,
    ) -> Result<()> {
        let Some(draft) = &mut self.draft else {
            return Ok(());
        };
        ensure!(
            (v.session.world, v.session.player, v.session.character) == self.session,
            "composition session replaced"
        );
        if !v.lifecycle.alive
            || v.lifecycle.death_pending
            || v.lifecycle.busy
            || v.lifecycle.departing
            || v.lifecycle.quarantined
            || v.tick.saturating_sub(draft.opened) > 200
            || v.character.inventory_revision != draft.revision
            || (draft.source == Source::Gift
                && self.mind.social.outgoing.as_deref() != Some(&draft.text))
            || (draft.source == Source::Market
                && self
                    .market_words
                    .as_ref()
                    .is_none_or(|(message, _)| self.mind.market.outgoing.as_ref() != Some(message)))
        {
            tracing::info!(player=v.session.player, source=?draft.source, tick=v.tick,
                opened=draft.opened, busy=v.lifecycle.busy, alive=v.lifecycle.alive,
                revision=v.character.inventory_revision, original_revision=draft.revision,
                "bot composition invalidated before submit");
            self.close_composition(app, false).await?;
            return Ok(());
        }
        if v.tick >= draft.next && draft.buffer != draft.text {
            if draft.paste {
                draft.buffer.clone_from(&draft.text);
            } else {
                let ch = draft.text[draft.buffer.len()..]
                    .chars()
                    .next()
                    .expect("unfinished draft");
                draft.buffer.push(ch);
            }
            draft.next = v.tick + if draft.buffer == draft.text { 2 } else { 1 };
        }
        let phase = if draft.paste && !draft.buffer.is_empty() {
            2
        } else {
            1
        };
        let text = draft.buffer.clone();
        let revision = Some(draft.revision);
        let session = (v.session.player, v.session.account, v.session.character);
        let retained = app
            .world
            .for_player(session.0)
            .call(move |w| {
                w.compose_bot_inputs(
                    session,
                    crate::world::bot_inputs::Composition {
                        phase,
                        text,
                        revision,
                    },
                )
            })
            .await?;
        if !retained {
            tracing::info!(
                player = session.0,
                "bot composition canceled at changed source boundary"
            );
            self.close_composition(app, false).await?;
        }
        Ok(())
    }
}
