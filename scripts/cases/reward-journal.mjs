import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {join} from 'node:path';
// Soulbound rolls draw their seed from the shared World RNG at defeat (src/world/xml/terminal.rs), so a realmSeed cannot pin them.
// Cases assert the journaled allocation instead; deterministic pickup coverage for empty rolls lives in loot-ownership.mjs.
export async function journaledReward(config,actor,character){
  const events=JSON.parse(execFileSync('python3',['-c',`
import json,sqlite3,sys
db=sqlite3.connect('file:'+sys.argv[1]+'?mode=ro',uri=True)
rows=db.execute("SELECT event,payload FROM world_events WHERE kind='terminal' AND json_extract(payload,'$.payload.data.defeat.actor')=?",(int(sys.argv[2]),)).fetchall()
print(json.dumps([{'event':e,'payload':json.loads(p)['payload']['data'],'entitlements':[{'owner':o,'item':i} for o,i in db.execute('SELECT owner_character,item FROM world_entitlements WHERE event=? ORDER BY ordinal',(e,))]} for e,p in rows]))
`,join(config.state,'game.db'),String(actor)],{encoding:'utf8'}));
  assert.equal(events.length,1,`One journaled terminal event required for actor ${actor}`);
  const [{event,payload,entitlements}]=events;assert(payload.allocation,`Terminal ${event} has no allocation plan`);
  const boss=(await(await fetch(config.origin+'/api/drop-rates')).json()).objects.find(o=>o.id===payload.defeat.kind);assert(boss,`Drop rates lack kind ${payload.defeat.kind}`);
  const contribution=payload.ledger.find(c=>c.character===character);assert(contribution,`Character ${character} absent from terminal ${event} ledger`);
  const share=contribution.effective_damage/boss.selected_max_hp;
  const planned=payload.allocation.allocations.filter(a=>a.owner?.Character===character);
  const committed=entitlements.filter(e=>e.owner===character).map(e=>e.item);
  const valid=planned.every(a=>{const row=boss.rows[a.row]?.source_row;return row?.ownership==='Private'&&row.pool.includes(a.item)&&share>=row.threshold;})
    &&JSON.stringify(planned.map(a=>a.item).sort())===JSON.stringify([...committed].sort());
  return {event,kind:payload.defeat.kind,position:payload.actions.publication.position,share,items:committed,valid};
}
