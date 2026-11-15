import assert from 'node:assert/strict';
import {writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {setTimeout as delay} from 'node:timers/promises';

export const fixture={class:0x31e,level:20,seed:12351,world:'tomb-of-the-ancients',worldSeed:12,position:{x:80.5,y:16.5},equipment:[0xb0b,0xb22,0xafc,0xacd],backpack:Array(8).fill(-1),stats:[770,252,50,40,50,50,75,50]};
export async function run(game){
  const snapshots=[],samples=[];let treasure,latest;
  const save=async label=>{const s={label,observed:await game.observe(),inspected:await game.inspect()};snapshots.push(s);return s;};
  const safe=s=>assert(s.observed.lifecycle.alive&&s.inspected.view.character.hp>300&&s.observed.game_ms<26000,'Living health-above300 and exposure-below26000 required');
  const jars=s=>s.diagnostic.entities.filter(e=>e.kind>=0xd2d&&e.kind<=0xd30&&e.tomb?.producer===treasure);
  async function sample(){const s=await game.inspect();samples.push(s);latest={count:jars(s).length,time:s.view.time_ms};return s;}
  async function watched(input,capture){
    let active=true;const action=game.act(input,capture).finally(()=>active=false);
    const observing=(async()=>{for(let n=0;active&&(await game.observe()).paused;n++){assert(n<250,'Native actor did not unpause');await delay(20);}while(active){await sample();await delay(80);}})();
    const results=await Promise.allSettled([action,observing]);for(const r of results)if(r.status==='rejected')throw r.reason;
  }
  try{
    const before=await save('prepared');safe(before);
    assert(before.observed.world==='Tomb of the Ancients'&&!before.inspected.diagnostic.entities.some(e=>e.kind===0xd1a),'Fresh Tomb outside Mummy encounters required');
    const initial=before.inspected.diagnostic.entities.find(e=>e.kind===0xd2c&&Math.hypot(e.pos.x-80.5,e.pos.y-16.5)<.1);assert(initial&&initial.hp===50000,'Original full-health treasure missing');treasure=initial.id;
    assert(jars(before.inspected).length===0,'Treasure must begin without jars');
    await watched({keys:[],duration_ms:1800},{name:'tomb-treasure-before-jars',when:v=>v.lifecycle.alive&&v.game_ms>=1500});
    const normal=await save('normal');safe(normal);
    await watched({keys:[],duration_ms:1000,target:{entity:treasure,camera_angle:-Math.PI/4,fire:true}});
    const activated=await save('activated');safe(activated);const hit=activated.inspected.diagnostic.entities.find(e=>e.id===treasure);
    assert(hit&&hit.hp>0&&hit.hp<49500&&activated.inspected.diagnostic.character.shots>0,'Ordinary sword damage must activate, not defeat, the original treasure');
    // Stop firing before the outer ring finishes; the sword cannot reach its radius-five jars.
    await watched({keys:[],duration_ms:8500},{name:'tomb-eight-canopic-jars',when:v=>Boolean(latest&&latest.count===8&&v.lifecycle.alive&&v.game_ms>=latest.time)});
    const spawned=await save('spawned');safe(spawned);const all=jars(spawned.inspected),source=spawned.inspected.diagnostic.entities.find(e=>e.id===treasure);
    game.check('original damaged treasure summons eight surviving jars',Boolean(source&&source.hp>0&&source.hp<49500&&all.length===8&&new Set(all.map(e=>e.id)).size===8&&all.every(e=>e.hp===1)));
    game.check('four original jar kinds occur twice at radius five',Array.from({length:4},(_,i)=>0xd2d+i).every(kind=>all.filter(e=>e.kind===kind).length===2)&&all.every(e=>Math.abs(Math.hypot(e.pos.x-initial.pos.x,e.pos.y-initial.pos.y)-5)<.02));
    game.check('jar census grants no kills or Tomb credit',snapshots.every(s=>s.inspected.diagnostic.character.kills===0&&s.inspected.diagnostic.character.tombs===0));
    await game.act({keys:['Insert'],duration_ms:300});const escaped=await save('escaped');
    game.check('same living legal Knight returns without mana use or clear credit',escaped.observed.world==='Nexus'&&escaped.observed.lifecycle.alive&&escaped.observed.session.character===before.observed.session.character&&snapshots.every(s=>s.inspected.view.character.mp===252&&JSON.stringify(s.inspected.view.character.items.slice(0,4))===JSON.stringify(fixture.equipment))&&escaped.inspected.diagnostic.character.kills===0&&escaped.inspected.diagnostic.character.tombs===0);
  }finally{await writeFile(join(process.env.QA_RESULTS_DIR,'canopic-census.json'),JSON.stringify({qualification:'Prepared single-native-client component. Selected legal Knight base770HP plus140HP ring, supplied seed12 treasure-center position from separate read-only source-generation planning. Ordinary sword, idle and Nexus inputs only. No actors joined, protection/stat/NPC/HP/RNG edits, bots, refill, relocation or adoption. All input through existing SDK; observer read-only, non-atomic polling/capture, holds are not total exposure. Eight-jar census only, not treasure defeat, jar breaking/Scarabs, loot, historical generator fidelity or full dungeon acceptance.',snapshots,samples},null,2));}
}
