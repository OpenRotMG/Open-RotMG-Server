import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
const exec=promisify(execFile);
const out=process.env.QA_RESULTS_DIR;
const spec={class:0x31d,level:20,seed:819,world:1,position:'spawn',equipment:[0xb0b,0xb29,0xa7b,-1],backpack:Array(8).fill(-1),stats:[770,252,75,25,50,50,75,0]};
let admissionActor,safePosition;
export async function prepare(api){
  const world=await api({op:'world_info',world:1}),[x,y]=world.spawn_points[0];
  safePosition={x,y};
  admissionActor=(await api({op:'spawn',spec:{...spec,guid:'shrine-admission',position:{x,y},controller:'direct',profile:null}})).result.actor;
  return spec;
}
export async function run(observer){
  let game=observer,shrine;
  const config=JSON.parse(await readFile(join(out,'native/native.json'),'utf8')),token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const headers={Authorization:'Bearer '+token,'Content-Type':'application/json'},records=[];
  async function api(command){
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers,body:JSON.stringify({id:randomUUID(),command})});
    const result=await response.json();assert(response.ok&&result.status!=='failed',JSON.stringify(result));return result;
  }
  async function cheat(actor,change){await api({op:'cheat',actor:actor.actor,change});}
  async function direct(action,duration_ms){
    const request=join(out,randomUUID()+'.json');
    await writeFile(request,JSON.stringify({id:randomUUID(),command:{op:'act',actor:admissionActor,action,duration_ms,pauseAfter:true}}));
    await exec(process.execPath,['scripts/game.mjs',join(out,'native/native.json'),request]);
  }
  async function map(){
    const state=await game.observe(),response=await fetch(config.origin+'/dev/map?player='+state.session.player,{headers});
    assert(response.ok);const scene=await response.json();
    return shrine?{entities:scene.entities.filter(e=>e.quest===shrine.id)}:scene;
  }
  async function save(label){const state=await game.inspect();records.push({label,state});await writeFile(join(out,'encounter.json'),JSON.stringify(records));return state;}
  const protectedUntil=new Map();
  async function protect(actor){
    const before=await actor.observe();let now=before.game_ms,restore;const until=protectedUntil.get(actor.actor)??0;
    if(actor.actor===admissionActor&&now<until&&until-now<6000){
      restore=before.position;
      await cheat(actor,{kind:'teleport',position:safePosition});
      await observer.act({keys:[],duration_ms:until-now+100});
      now=(await actor.observe()).game_ms;
    }
    if(now>=until){await cheat(actor,{kind:'protection',condition:'invulnerable',duration_ms:60000});protectedUntil.set(actor.actor,now+60000);}
    if(restore)await cheat(actor,{kind:'teleport',position:restore});
  }
  async function fire(actor,id,ms=500){
    await protect(observer);await protect(actor);
    const state=await actor.inspect(),target=state.diagnostic.entities.find(e=>e.id===id);assert(target,'Living visible native target required');
    if(Math.hypot(target.pos.x-state.view.position.x,target.pos.y-state.view.position.y)>2)await approach(actor,target.pos,1.5);
    await actor.act({keys:[],duration_ms:ms,target:{entity:id,camera_angle:-Math.PI/4,fire:true}});
  }
  let initial=await map();
  game.check('no Skull Shrine is active before an earned event',!initial.entities.some(e=>e.kind===0xd56));
  let blocked=new Set(initial.blocked.map(p=>p.join(',')));
  function approachPoint(point){
    const position=[[1.5,0],[-1.5,0],[0,1.5],[0,-1.5],[1,1],[-1,1],[1,-1],[-1,-1]].map(([x,y])=>({x:point.x+x,y:point.y+y})).find(p=>!blocked.has([Math.floor(p.x),Math.floor(p.y)].join(',')));
    assert(position,'No walkable prepared attack position beside the observed hero');return position;
  }
  const order=[0x678,0x67d,0x668,0x675,0x928,0x91b];
  const candidates=initial.entities.filter(e=>order.includes(e.kind)).sort((a,b)=>order.indexOf(a.kind)-order.indexOf(b.kind));
  await writeFile(join(out,'initial-map.json'),JSON.stringify({width:initial.width,height:initial.height,candidates,shrines:initial.entities.filter(e=>e.kind===0xd56)}));
  initial=null;
  const events=async()=>(await readFile(join(out,'native/server.log'),'utf8')).split('\n').filter(l=>l.startsWith('{')).map(l=>JSON.parse(l).fields);
  let admission;
  for(const candidate of candidates){
    await cheat({actor:admissionActor},{kind:'teleport',position:approachPoint(candidate.pos)});
    for(let n=0;n<80;n++){
      await protect(observer);await protect({actor:admissionActor,observe:()=>api({op:'observe',actor:admissionActor})});
      const scene=await api({op:'inspect',actor:admissionActor});
      const target=scene.diagnostic.entities.find(e=>e.quest===candidate.id&&[0x929,0x91d].includes(e.kind))||scene.diagnostic.entities.find(e=>e.id===candidate.id)||scene.diagnostic.entities.find(e=>e.quest===candidate.id&&[0x920,0x91c,0x6c1,0x92d].includes(e.kind));
      if(!target)break;
      const position=approachPoint(target.pos);
      await cheat({actor:admissionActor},{kind:'teleport',position});
      await direct({command:'shoot',angle:Math.atan2(target.pos.y-position.y,target.pos.x-position.x)},3000);
      admission=(await events()).find(f=>f.message==='quest defeat summoned a Realm event'&&f.kind===0xd56);if(admission)break;
    }
    if(admission)break;
  }
  assert(admission,'Bounded ordinary hero defeats did not admit a Skull Shrine');
  shrine={id:admission.entity,kind:0xd56,pos:{x:admission.x,y:admission.y}};
  blocked=null;candidates.length=0;
  const logs=await events();
  game.check('ordinary qualifying defeat admits the reserved shrine exactly once',!!admission&&logs.filter(f=>f.message==='quest defeat summoned a Realm event'&&f.entity===shrine.id).length===1);
  await writeFile(join(out,'admission.json'),JSON.stringify({shrine,admission}));
  game=await observer.join({...spec,seed:820,position:{x:shrine.pos.x+8,y:shrine.pos.y}});
  for(let n=0;n<12&&(await game.observe()).character.hp===770;n++)await game.act({keys:[],duration_ms:200});
  const vulnerable=await game.observe();
  game.check('unprotected native shrine encounter removes HP without killing the character',vulnerable.character.hp<770&&vulnerable.lifecycle.alive);
  await writeFile(join(out,'unprotected.json'),JSON.stringify(vulnerable));
  await protect(game);
  await game.act({keys:[],duration_ms:500});
  await game.act({keys:[],duration_ms:500},{name:'shrine-arrival',when:()=>true});
  const start=await save('arrival'),root=s=>s.diagnostic.entities.find(e=>e.id===shrine.id);
  game.check('native client sees the selected 10000-HP stationary shrine',root(start)?.hp===10000&&root(start).kind===0xd56);
  const samples=[];let done=false;
  const action=game.act({keys:[],duration_ms:10000},{name:'shrine-skulls',when:v=>v.game_ms>=start.view.time_ms+8500}).finally(()=>done=true);
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
  const victim=children.filter(e=>e.kind===0xd57).sort((a,b)=>Math.hypot(a.pos.x-shrine.pos.x,a.pos.y-shrine.pos.y)-Math.hypot(b.pos.x-shrine.pos.x,b.pos.y-shrine.pos.y))[0];assert(victim);
  async function approach(actor,point,range=.5){
    for(let n=0;n<80;n++){
      await protect(observer);await protect(actor);const view=await actor.observe(),dx=point.x-view.position.x,dy=point.y-view.position.y;if(Math.hypot(dx,dy)<range)return;
      const x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2,keys=[];
      if(Math.abs(x)>=Math.abs(y)*.4142)keys.push(x>0?'d':'a');
      if(Math.abs(y)>=Math.abs(x)*.4142)keys.push(y>0?'s':'w');
      await actor.act({keys,duration_ms:Math.max(50,Math.min(200,Math.round(Math.hypot(dx,dy)/(4+5.6*view.character.stats.speed/75)*1000)))});
    }
    throw Error('Native walking did not reach the observed shrine target');
  }
  await approach(game,victim.pos,1.5);
  for(let n=0;n<12&&(await game.inspect()).diagnostic.entities.some(e=>e.id===victim.id);n++)await fire(game,victim.id,250);
  await approach(game,{x:shrine.pos.x+2,y:shrine.pos.y});
  await game.act({keys:[],duration_ms:1000});
  const replenished=(await map()).entities.filter(e=>e.quest===shrine.id&&e.kind===0xd57);
  game.check('defeated red skull is replaced with a new owned identity',!replenished.some(e=>e.id===victim.id)&&replenished.length===20&&replenished.some(e=>!children.some(old=>old.id===e.id)));
  const peer=await observer.join({...spec,seed:821,position:{x:shrine.pos.x-2,y:shrine.pos.y}});
  await protect(peer);
  const before=await Promise.all([game.observe(),peer.observe()]);
  for(const actor of [game,peer])for(let n=0;n<12;n++){
    const e=root(await game.inspect());assert(e,'Shrine must survive both qualification contributions');
    if((e.damage[actor.actor]??0)>500)break;await fire(actor,shrine.id,150);
  }
  const contributions=await save('contributors');
  game.check('two native players damage the same shrine',root(contributions).damage[game.actor]>500&&root(contributions).damage[peer.actor]>500);
  for(let n=0;n<100&&root(await game.inspect());n++)await fire(game,shrine.id,250);
  await game.act({keys:[],duration_ms:500},{name:'shrine-defeated',when:()=>true});
  const after=await Promise.all([game.inspect(),peer.inspect()]);
  game.check('native sword fire defeats shrine',!root(after[0]));
  game.check('both contributors receive defeat XP',after[0].view.character.experience>before[0].character.experience&&after[1].view.character.experience>before[1].character.experience);
  const orphaned=(await map()).entities.filter(e=>e.quest===shrine.id&&[0xd57,0xd58].includes(e.kind));
  const bag=after[1].diagnostic.entities.find(e=>e.extra_stats.some(([stat,account])=>stat===54&&account===after[1].view.session.account)&&e.items?.some(item=>item!==-1));
  assert(bag,'Observed shrine roll must supply a peer reward for the pickup check');
  await cheat({actor:admissionActor},{kind:'teleport',position:bag.pos});
  const outsider=await api({op:'inspect',actor:admissionActor});
  game.check('nearby direct noncontributor cannot see the peer private reward',!outsider.diagnostic.entities.some(e=>e.id===bag.id));
  await approach(peer,bag.pos,.4);
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
