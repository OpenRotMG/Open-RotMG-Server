use super::*;
use crate::store::TradeWitness;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ReplyKind {
    Counter([Goods; 2]),
    Bank([Goods; 2]),
    Thanks(TradeWitness),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct BoundReply {
    message: Message,
    kind: ReplyKind,
    session: [i32; 3],
    account: i32,
    revision: i32,
    peer: (i32, i32, String),
    until: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::bots) struct PendingTrade {
    pub contact: Incoming,
    pub witness: TradeWitness,
}
impl Market {
    pub(super) fn needs_room(v: &PlayerView, q: &[Goods; 2]) -> bool {
        let carried = v.character.items.iter().skip(4).filter(|k| **k == q[0].kind).count();
        let free = v.character.items.iter().skip(4).filter(|k| **k < 0).count();
        (carried < usize::from(q[0].quantity) && free == 0)
            || free + usize::from(q[0].quantity) < usize::from(q[1].quantity)
    }
    fn reply_text(kind: &ReplyKind, c: &Content) -> Result<String> {
        Ok(match kind {
            ReplyKind::Counter(q) => format!("nty wts {} for {}", goods_text(&q[0], c)?, goods_text(&q[1], c)?),
            ReplyKind::Bank(_) => "full inv".into(),
            ReplyKind::Thanks(_) => "ty".into(),
        })
    }
    pub(super) fn respond(
        &mut self, v: &PlayerView, peer: (i32, i32, String), kind: ReplyKind, c: &Content,
    ) -> Result<()> {
        let message = Message { text: Self::reply_text(&kind, c)?, channel: Channel::Tell(peer.2.clone()) };
        let reply_kind=match &kind { ReplyKind::Counter(_)=>"counter", ReplyKind::Bank(_)=>"bank", ReplyKind::Thanks(_)=>"thanks" };
        let quote=match &kind { ReplyKind::Counter(q)|ReplyKind::Bank(q)=>Some(q), ReplyKind::Thanks(_)=>None };
        let transaction=match &kind { ReplyKind::Thanks(w)=>Some(w.transaction.as_str()), _=>None };
        tracing::info!(account=v.session.account,session=?v.session,revision=v.character.inventory_revision,?peer,reply_kind,transaction,?quote,capacity=v.character.items.iter().skip(4).filter(|k| **k<0).count(),"market state-bound reply queued");
        self.reply = Some(BoundReply { message: message.clone(), kind,
            session: [v.session.world,v.session.player,v.session.character],account:v.session.account,
            revision:v.character.inventory_revision,peer,until:v.elapsed_ms+20_000 });
        self.outgoing = Some(message);
        Ok(())
    }
    pub(super) fn response_current(
        &self, message: &Message, v: &PlayerView, c: &Content, future: Option<u16>,
    ) -> Result<Option<bool>> {
        let Some(r) = self.reply.as_ref().filter(|r| &r.message == message) else { return Ok(None); };
        ensure!(v.session.account>0 && v.session.player>0 && v.session.character>0
            && v.character.inventory_revision>=0 && v.character.items.len()==12
            && v.position.x.is_finite() && v.position.y.is_finite(),"invalid own market reply identity or geometry");
        ensure!(r.account>0 && r.session[1]>0 && r.session[2]>0 && r.revision>=0
            && r.peer.0>0 && r.peer.1>0 && r.peer.1!=r.account
            && !r.peer.2.is_empty() && r.peer.2.len()<=16
            && r.peer.2.bytes().all(|b| b.is_ascii_alphanumeric())
            && message.channel==Channel::Tell(r.peer.2.clone())
            && message.text==Self::reply_text(&r.kind,c)?,"invalid bound market reply intent");
        if let ReplyKind::Counter(q) | ReplyKind::Bank(q) = &r.kind {
            ensure!(q[0].kind!=q[1].kind && q.iter().all(|g| (0..=65535).contains(&g.kind) && (1..=8).contains(&g.quantity)),"invalid market reply quote");
            for g in q { c.get(u16::try_from(g.kind)?)?; }
        }
        if r.session != [v.session.world,v.session.player,v.session.character] || r.account!=v.session.account
            || r.revision!=v.character.inventory_revision || v.elapsed_ms>=r.until
            || v.map.name!="Nexus" || !super::super::settled(v) { return Ok(Some(false)); }
        let Some(peer)=v.objects.iter().find(|p| p.id==r.peer.0 && p.account==Some(r.peer.1) && p.name==r.peer.2 && p.hp>0 && !p.equipped.is_empty()) else { return Ok(Some(false)); };
        ensure!(peer.position.x.is_finite() && peer.position.y.is_finite(),"invalid observed market reply geometry");
        if peer.position.distance(v.position)>=12. { return Ok(Some(false)); }
        Ok(Some(match &r.kind {
            ReplyKind::Counter(q) => self.quote(v,c,future)?.as_ref()==Some(q)
                && v.character.items.iter().skip(4).filter(|k| **k==q[0].kind).count()>=usize::from(q[0].quantity),
            ReplyKind::Bank(q) => self.preparing && self.quote(v,c,future)?.as_ref()==Some(q)
                && Self::needs_room(v,q) && v.objects.iter().any(|p| p.kind==0x720 && p.usable),
            ReplyKind::Thanks(witness) => witness.writes.iter().any(|w| w.account==v.session.account
                && w.id==v.session.character && w.revision.checked_add(1)==Some(v.character.inventory_revision)
                && w.items==v.character.items),
        }))
    }
    pub(in crate::bots) async fn recover_trade_reply(
        &mut self, store: &crate::store::Store, v: &PlayerView, c: &Content,
    ) -> Result<()> {
        let Some(p)=self.pending_trade.clone() else { return Ok(()); };
        ensure!(p.contact.session==[v.session.world,v.session.player,v.session.character]
            && p.contact.account>0 && p.contact.account!=v.session.account && p.contact.player>0
            && !p.contact.name.is_empty() && p.contact.name.len()<=16
            && p.contact.name.bytes().all(|b| b.is_ascii_alphanumeric())
            && p.witness.writes.iter().any(|w| w.account==p.contact.account)
            && p.witness.writes.iter().any(|w| w.account==v.session.account && w.id==v.session.character),"market acceptance belongs to another session");
        if !store.trade_committed(p.witness.clone()).await? { return Ok(()); }
        let own = p.witness.writes.iter().find(|w| w.account==v.session.account).expect("market witness owner validated");
        // Store can commit while the controller still holds its pre-publication view.
        if !super::super::settled(v) || v.character.inventory_revision<=own.revision { return Ok(()); }
        self.pending_trade=None;
        tracing::info!(account=v.session.account,peer=p.contact.account,transaction=p.witness.transaction,revision=v.character.inventory_revision,"market confirmed exchange receipt");
        if super::super::settled(v) && p.witness.writes.iter().any(|w| w.account==v.session.account
            && w.id==v.session.character && w.revision.checked_add(1)==Some(v.character.inventory_revision)
            && w.items==v.character.items) {
            self.respond(v,(p.contact.player,p.contact.account,p.contact.name),ReplyKind::Thanks(p.witness),c)?;
        }
        Ok(())
    }
}
