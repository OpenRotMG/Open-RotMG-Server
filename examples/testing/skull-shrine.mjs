import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
const out=process.env.QA_RESULTS_DIR;
const spec={class:0x31d,level:20,seed:819,world:1,position:'spawn',equipment:[0xb0b,0xb29,0xa7b,-1],backpack:Array(8).fill(-1),stats:[770,252,75,25,50,50,75,0]};
export const fixture=spec;
export async function run(game){
  const config=JSON.parse(await readFile(join(out,'native/native.json'),'utf8')),token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const headers={Authorization:'Bearer '+token,'Content-Type':'application/json'},records=[];
  async function api(command){
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers,body:JSON.stringify({id:randomUUID(),command})});
    const result=await response.json();assert(response.ok&&result.status==='succeeded',JSON.stringify(result));return result;
  }
  async function cheat(actor,change){await api({op:'cheat',actor:actor.actor,change});}
  async function map(){
    const state=await game.observe(),response=await fetch(config.origin+'/dev/map?player='+state.session.player,{headers});
    assert(response.ok);return await response.json();
  }
  async function save(label){const state=await game.inspect();records.push({label,state});await writeFile(join(out,'encounter.json'),JSON.stringify(records));return state;}
  const protectedUntil=new Map();
  async function protect(actor){const now=(await actor.observe()).game_ms;if(now>=(protectedUntil.get(actor.actor)??0)){await cheat(actor,{kind:'protection',condition:'invulnerable',duration_ms:60000});protectedUntil.set(actor.actor,now+60000);}}
  async function fire(actor,id,ms=500){await protect(actor);await actor.act({keys:[],duration_ms:ms,target:{entity:id,camera_angle:-Math.PI/4,fire:true}});}
  const initial=await map();await writeFile(join(out,'initial-map.json'),JSON.stringify(initial));
  game.check('no Skull Shrine is active before an earned event',!initial.entities.some(e=>e.kind===0xd56));
  const order=[0x678,0x67d,0x668,0x91f,0x91b,0x675,0x928];
  const candidates=initial.entities.filter(e=>order.includes(e.kind)).sort((a,b)=>order.indexOf(a.kind)-order.indexOf(b.kind));
  let shrine;
  for(const candidate of candidates){
    await protect(game);await cheat(game,{kind:'teleport',position:candidate.pos});
    for(let n=0;n<80;n++){
      const scene=await map();shrine=scene.entities.find(e=>e.kind===0xd56);if(shrine)break;
      const target=scene.entities.find(e=>e.id===candidate.id)||scene.entities.find(e=>e.quest===candidate.id&&[0x920,0x91c,0x6c1,0x92d].includes(e.kind));
      if(!target)break;
      await fire(game,target.id);
    }
    await game.act({keys:[],duration_ms:250});
    shrine=(await map()).entities.find(e=>e.kind===0xd56);if(shrine)break;
  }
  assert(shrine,'Bounded ordinary hero defeats did not admit a Skull Shrine');
  const logs=(await readFile(join(out,'native/server.log'),'utf8')).split('\n').filter(l=>l.startsWith('{')).map(l=>JSON.parse(l).fields);
  const admission=logs.find(f=>f.message==='quest defeat summoned a Realm event'&&f.entity===shrine.id);
  game.check('ordinary qualifying defeat admits the reserved shrine exactly once',!!admission&&logs.filter(f=>f.message==='quest defeat summoned a Realm event'&&f.entity===shrine.id).length===1);
  await writeFile(join(out,'admission.json'),JSON.stringify({shrine,admission}));
  await cheat(game,{kind:'teleport',position:{x:shrine.pos.x+3,y:shrine.pos.y}});
  await game.act({keys:[],duration_ms:500},{name:'shrine-arrival',when:()=>true});
  const start=await save('arrival'),root=s=>s.diagnostic.entities.find(e=>e.id===shrine.id);
  game.check('native client sees the selected 10000-HP stationary shrine',root(start)?.hp===10000&&root(start).kind===0xd56);
  const samples=[];let done=false;
  const action=game.act({keys:[],duration_ms:10000},{name:'shrine-skulls',when:()=>true}).finally(()=>done=true);
  const sampling=(async()=>{while(!done){samples.push(await game.inspect());await new Promise(ok=>setTimeout(ok,60));}})();
  for(const result of await Promise.allSettled([action,sampling]))if(result.status==='rejected')throw result.reason;
  await writeFile(join(out,'movement-attacks.json'),JSON.stringify(samples));
  const full=await map(),children=full.entities.filter(e=>e.quest===shrine.id&&[0xd57,0xd58].includes(e.kind));
  await writeFile(join(out,'population-map.json'),JSON.stringify(full));
  // Keep the later per-colour cap distinct from the period grouped five-second policy: https://web.archive.org/web/20120117000123id_/http://forums.wildshadow.com/wiki/realm-mad-god/skull-shrine
  for(const kind of [0xd57,0xd58])game.check('current shrine replenishes colour '+kind+' to its 20-skull density cap',children.filter(e=>e.kind===kind).length===20);
  game.check('shrine remains stationary',samples.every(s=>{const e=root(s);return e&&Math.hypot(e.pos.x-shrine.pos.x,e.pos.y-shrine.pos.y)<.01;}));
  const shots=samples.flatMap(s=>s.view.projectiles),owners=new Map([[shrine.id,0xd56],...children.map(e=>[e.id,e.kind])]);
  for(const kind of [0xd56,0xd57,0xd58])game.check('native client receives selected shrine-family projectile '+kind,shots.some(p=>owners.get(p.owner)===kind&&p.speed===100&&p.lifetime_ms===2400));
  for(const kind of [0xd57,0xd58]){
    const seen=new Map();for(const s of samples)for(const e of s.diagnostic.entities.filter(e=>e.quest===shrine.id&&e.kind===kind)){const old=seen.get(e.id);if(old&&Math.hypot(old.x-e.pos.x,old.y-e.pos.y)>.5)seen.set(e.id,{...e.pos,moved:true});else if(!old)seen.set(e.id,e.pos);}
    game.check('native client observes moving skull colour '+kind,[...seen.values()].some(p=>p.moved));
  }
  const victim=children.find(e=>e.kind===0xd57);assert(victim);
  await cheat(game,{kind:'teleport',position:victim.pos});
  for(let n=0;n<12&&(await map()).entities.some(e=>e.id===victim.id);n++)await fire(game,victim.id,250);
  await cheat(game,{kind:'teleport',position:{x:shrine.pos.x+2,y:shrine.pos.y}});
  await game.act({keys:[],duration_ms:1000});
  const replenished=(await map()).entities.filter(e=>e.quest===shrine.id&&e.kind===0xd57);
  game.check('defeated red skull is replaced with a new owned identity',!replenished.some(e=>e.id===victim.id)&&replenished.length===20&&replenished.some(e=>!children.some(old=>old.id===e.id)));
  const peer=await game.join({...spec,seed:820,position:{x:shrine.pos.x-2,y:shrine.pos.y}}),outsider=await game.join({...spec,seed:821,position:{x:shrine.pos.x,y:shrine.pos.y+2}});
  await protect(peer);await protect(outsider);
  const before=await Promise.all([game.observe(),peer.observe(),outsider.observe()]);
  for(const actor of [game,peer])for(let n=0;n<12;n++){
    const e=root(await game.inspect());assert(e,'Shrine must survive both qualification contributions');
    if((e.damage[actor.actor]??0)>500)break;await fire(actor,shrine.id,150);
  }
  const contributions=await save('contributors');
  game.check('two native players damage the same shrine',root(contributions).damage[game.actor]>500&&root(contributions).damage[peer.actor]>500);
  for(let n=0;n<100&&root(await game.inspect());n++)await fire(game,shrine.id,250);
  await game.act({keys:[],duration_ms:500},{name:'shrine-defeated',when:()=>true});
  const after=await Promise.all([game.inspect(),peer.inspect(),outsider.inspect()]);
  game.check('native sword fire defeats shrine',!root(after[0]));
  game.check('both contributors receive defeat XP',after[0].view.character.experience>before[0].character.experience&&after[1].view.character.experience>before[1].character.experience);
  const orphaned=(await map()).entities.filter(e=>e.quest===shrine.id&&[0xd57,0xd58].includes(e.kind));
  const bag=after[1].diagnostic.entities.find(e=>e.extra_stats.some(([stat,account])=>stat===54&&account===after[1].view.session.account)&&e.items?.some(item=>item!==-1));
  assert(bag,'Observed shrine roll must supply a peer reward for the pickup check');
  game.check('noncontributor cannot see the peer private reward',!after[2].diagnostic.entities.some(e=>e.id===bag.id));
  await cheat(outsider,{kind:'teleport',position:bag.pos});
  await outsider.act({keys:[],duration_ms:500},{name:'shrine-ineligible-reward',when:()=>true});
  const outsiderRevision=(await outsider.observe()).character.inventory_revision;
  await outsider.act({keys:[],duration_ms:350,drag:{from:{x:634,y:528},to:{x:634,y:428}}});
  game.check('noncontributor cannot acquire the private reward',(await outsider.observe()).character.inventory_revision===outsiderRevision&&(await peer.inspect()).diagnostic.entities.find(e=>e.id===bag.id)?.items[0]===bag.items[0]);
  await cheat(peer,{kind:'teleport',position:bag.pos});
  await peer.act({keys:[],duration_ms:500},{name:'shrine-eligible-reward',when:()=>true});
  const inventory=(await peer.observe()).character,item=bag.items[0];assert(item!==-1);
  await peer.act({keys:[],duration_ms:350,drag:{from:{x:634,y:528},to:{x:634,y:428}}});
  const picked=await peer.inspect();
  game.check('native pickup commits one eligible reward',picked.view.character.items[4]===item&&picked.view.character.inventory_revision===inventory.inventory_revision+1);
  await peer.act({keys:[],duration_ms:350,drag:{from:{x:634,y:528},to:{x:678,y:428}}});
  game.check('repeated empty-slot drag cannot duplicate reward',(await peer.observe()).character.inventory_revision===picked.view.character.inventory_revision);
  await peer.act({keys:['Insert'],duration_ms:250});
  const retained=await peer.observe();
  game.check('normal Nexus reconnect preserves acquired reward',retained.world==='Nexus'&&retained.session.character===picked.view.session.character&&retained.character.items[4]===item&&retained.character.inventory_revision===picked.view.character.inventory_revision);
  await game.act({keys:[],duration_ms:3000});
  const end=await map(),xp=(await game.observe()).character.experience;
  game.check('defeated shrine does not respawn or replenish skulls',!end.entities.some(e=>e.id===shrine.id)&&end.entities.filter(e=>e.quest===shrine.id&&[0xd57,0xd58].includes(e.kind)).every(e=>orphaned.some(c=>c.id===e.id)));
  game.check('owned orphan skulls survive shrine defeat',end.entities.some(e=>orphaned.some(c=>c.id===e.id)));
  await game.act({keys:[],duration_ms:2000});
  game.check('defeat XP cannot repeat',(await game.observe()).character.experience===xp);
  await writeFile(join(out,'rewards.json'),JSON.stringify({before,contributions,after,orphaned,end,picked,retained}));
  await save('finished');
}
