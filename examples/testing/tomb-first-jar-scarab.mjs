import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {setTimeout as delay} from 'node:timers/promises';

export const fixture={class:0x31e,level:20,seed:12351,world:'tomb-of-the-ancients',worldSeed:12,position:{x:81.5,y:18.5},equipment:[0xa00,0xb22,0xafc,0xacd],backpack:Array(8).fill(-1),stats:[770,252,50,40,50,50,75,50]};
export async function run(game){
  const out=process.env.QA_RESULTS_DIR,snapshots=[],samples=[],abilities=[];let treasure,jar,player,born,failure;
  const save=async label=>{const s={label,observed:await game.observe(),inspected:await game.inspect()};snapshots.push(s);return s;};
  const safe=s=>assert(s.view.lifecycle.alive&&s.view.character.hp>300&&s.view.time_ms<26000,'Living health-above300 and exposure-below26000 required');
  const jars=s=>s.diagnostic.entities.filter(e=>e.kind>=0xd2d&&e.kind<=0xd30&&e.tomb?.producer===treasure);
  const scarabs=s=>s.diagnostic.entities.filter(e=>e.kind===0xd1b&&e.tomb?.producer===jar);
  async function events(){const text=await readFile(join(out,'native/server.log'),'utf8');assert(text.endsWith('\n'),'Complete accepted-ability records required');abilities.splice(0,abilities.length,...text.trimEnd().split('\n').filter(x=>x.startsWith('{')).map(JSON.parse).filter(x=>x.fields?.message==='ability activated'&&x.fields.player===player).map(x=>x.fields));return abilities;}
  async function sample(){const s=await game.inspect();safe(s);samples.push(s);const spawned=scarabs(s);if(!born&&spawned.length===1)born={id:spawned[0].id,time:s.view.time_ms};}
  async function watched(input,capture){
    let active=true;const action=game.act(input,capture).finally(()=>active=false);
    const observing=(async()=>{for(let n=0;active&&(await game.observe()).paused;n++){assert(n<250,'Native actor did not unpause');await delay(20);}while(active){await sample();await delay(80);}})();
    const results=await Promise.allSettled([action,observing]);for(const r of results)if(r.status==='rejected')throw r.reason;
  }
  try{
    const before=await save('prepared');safe(before.inspected);player=before.inspected.diagnostic.player;
    assert(before.observed.world==='Tomb of the Ancients'&&!before.inspected.diagnostic.entities.some(e=>e.kind===0xd1a),'Fresh Tomb outside Mummy encounters required');
    const original=before.inspected.diagnostic.entities.find(e=>e.kind===0xd2c&&Math.hypot(e.pos.x-80.5,e.pos.y-16.5)<.1);assert(original&&original.hp===50000,'Original full-health treasure missing');treasure=original.id;assert(jars(before.inspected).length===0,'No initial jars');
    await watched({keys:[],duration_ms:300});
    await watched({keys:[' '],duration_ms:100,target:{entity:treasure,camera_angle:-Math.PI/4,fire:false}});
    const activated=await save('activated');safe(activated.inspected);const hit=activated.inspected.diagnostic.entities.find(e=>e.id===treasure);await events();
    assert(hit&&hit.hp>0&&hit.hp<49500&&abilities.length===1&&abilities[0].item===0xb22&&abilities[0].mana_cost===100&&abilities[0].cooldown_ms===500&&activated.inspected.diagnostic.character.shots===5,'One ordinary selected shield must activate the living treasure');
    for(let n=0;n<6&&!jars((await game.inspect())).some(e=>e.kind===0xd2d);n++)await watched({keys:[],duration_ms:300});
    const ready=await save('first-jar-ready');safe(ready.inspected);const source=ready.inspected.diagnostic.entities.find(e=>e.id===treasure),first=jars(ready.inspected).filter(e=>e.kind===0xd2d);
    assert(source&&source.stage<6&&first.length===1&&first[0].hp===1&&jars(ready.inspected).length<5,'First original jar required before a repeated jar kind or full census');jar=first[0].id;
    assert(Math.hypot(first[0].pos.x-ready.inspected.view.position.x,first[0].pos.y-ready.inspected.view.position.y)<3.5,'Short Sword must reach the stationary jar');
    await watched({keys:[],duration_ms:200},{name:'tomb-first-canopic-jar',when:v=>v.lifecycle.alive});
    const intact=await save('intact');safe(intact.inspected);assert(intact.inspected.diagnostic.entities.some(e=>e.id===jar&&e.hp===1),'Original jar must remain intact before ordinary sword input');
    // The starter sword leaves a 300-HP Scarab alive even if two trailing shots connect.
    await watched([{actor:game.actor,input:{keys:[],duration_ms:100,target:{entity:jar,camera_angle:-Math.PI/4,fire:true}}},{actor:game.actor,input:{keys:[],duration_ms:500}}]);
    const broken=await save('broken');safe(broken.inspected);const shots=broken.inspected.diagnostic.character.shots-5;
    assert(shots>=1&&shots<=3&&!broken.inspected.diagnostic.entities.some(e=>e.id===jar)&&scarabs(broken.inspected).length===1&&scarabs(broken.inspected)[0].hp>0,'One bounded sword burst must break the original jar and leave its one Scarab alive');
    await watched({keys:[],duration_ms:500},{name:'tomb-first-jar-scarab',when:v=>Boolean(born&&v.lifecycle.alive&&v.game_ms>=born.time+250)});
    const emerged=await save('emerged');safe(emerged.inspected);const offspring=scarabs(emerged.inspected);
    game.check('first original jar releases exactly one living selected Scarab',offspring.length===1&&offspring[0].id===born.id&&offspring[0].hp>0&&offspring[0].hp<=300&&offspring[0].tomb.producer===jar);
    game.check('original treasure survives without dungeon-clear credit',snapshots.every(s=>s.inspected.diagnostic.entities.some(e=>e.id===treasure&&e.hp>0)&&s.inspected.diagnostic.character.tombs===0&&s.inspected.diagnostic.character.kills<=1));
    await game.act({keys:['Insert'],duration_ms:300});const escaped=await save('escaped');safe(escaped.inspected);await events();
    game.check('same living legal Knight returns with unchanged gear and one finite shield',escaped.observed.world==='Nexus'&&escaped.observed.session.character===before.observed.session.character&&abilities.length===1&&escaped.inspected.diagnostic.character.tombs===0&&JSON.stringify(escaped.inspected.view.character.items)===JSON.stringify(before.inspected.view.character.items));
  }catch(error){failure=error;throw error;}finally{
    let auditError;try{await events();}catch(error){auditError=error;}
    await writeFile(join(out,'first-jar-scarab.json'),JSON.stringify({qualification:'Fresh prepared single native Knight with legal base770HP, selected gear including starter Short Sword and Colossus Shield, supplied seed12 position81.5,18.5. One ordinary shield activates the treasure, one bounded sword burst breaks its first jar; not the accepted eight-jar census replay. SDK-only input; read-only non-atomic observer/capture, holds not total exposure. No joins/bots/NPC/HP/RNG/stat edits/refill/relocation/adoption/revival. First jar/oneScarab only, not all jar variants, loot, treasure defeat, earned entry, historical fidelity or full dungeon acceptance.',snapshots,samples,abilities,born,error:failure?String(failure):null,auditError:auditError?String(auditError):null},null,2));
    if(auditError)throw new AggregateError([...(failure?[failure]:[]),auditError],'Case and final accepted-ability audit must both remain observable');
  }
}
