import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
import {DatabaseSync} from 'node:sqlite';
const spec={class:0x30e,level:20,seed:859,world:-3,equipment:[0xb08,0xb24,0xb05,0xacd],backpack:Array(8).fill(-1),stats:[670,385,75,25,50,75,40,60]};
export const realmSeed=105;
const worldSeed=104n;
// This prepared seed supplies an ordinary Medusa with a qualifying .04 terminal draw.
// First f64 from rand 0.8 StdRng: PCG32 expands the seed, then ChaCha12 supplies two words.
// Medusa root MetamorphoseAtDeath is local 5; no runtime output enters the reference roll.
export function medusaDraw(id, preparedSeed) {
  let seed = BigInt.asUintN(64, (preparedSeed ^ BigInt(id)) + 5n * 0x9e3779b97f4a7c15n);
  const rotate = (x,n) => (x << n | x >>> (32-n)) >>> 0;
  const initial = new Uint32Array([0x61707865,0x3320646e,0x79622d32,0x6b206574,...Array(12).fill(0)]);
  for(let i=4;i<12;i++) {
    seed=BigInt.asUintN(64,seed*6364136223846793005n+11634580027462260723n);
    initial[i]=rotate(Number(BigInt.asUintN(32,((seed>>18n)^seed)>>27n)),32-Number(seed>>59n));
  }
  const x=initial.slice();
  function quarter(a,b,c,d) {
    x[a]+=x[b];x[d]=rotate(x[d]^x[a],16);x[c]+=x[d];x[b]=rotate(x[b]^x[c],12);
    x[a]+=x[b];x[d]=rotate(x[d]^x[a],8);x[c]+=x[d];x[b]=rotate(x[b]^x[c],7);
  }
  for(let i=0;i<6;i++) {
    for(let j=0;j<4;j++)quarter(j,j+4,j+8,j+12);
    for(let j=0;j<4;j++)quarter(j,4+(j+1)%4,8+(j+2)%4,12+(j+3)%4);
  }
  const word=(BigInt((x[1]+initial[1])>>>0)<<32n)|BigInt((x[0]+initial[0])>>>0);
  return Number(word>>11n)/2**53;
}
let medusaId;
export async function prepare(api){
  const {medusa,...fixture}=await prepareAbyssApproach(api,worldSeed,draw=>draw<.04);
  medusaId=medusa;
  return fixture;
}
export async function prepareAbyssApproach(api, preparedSeed, choose){
  const info=await api({op:'world_info',world:spec.world}),[x,y]=info.spawn_points[0];
  assert.equal(info.prepared_seed,String(preparedSeed));
  const scout=await api({op:'spawn',spec:{...spec,guid:'abyss-entry-scout',position:{x,y},controller:'direct',profile:null}});
  const state=await api({op:'inspect',actor:scout.result.actor}),root=join(process.env.QA_RESULTS_DIR,'native'),report=JSON.parse(await readFile(join(root,'result.json'),'utf8'));
  const response=await fetch(report.origin+'/dev/map?player='+state.diagnostic.player,{headers:{Authorization:'Bearer '+(await readFile(join(root,'state/control.token'),'utf8')).trim()}});assert(response.ok);
  const map=await response.json(),blocked=new Set(map.blocked.map(p=>p.join(',')));

  const clear=p=>[-.45,0,.45].every(dx=>[-.45,0,.45].every(dy=>!blocked.has([Math.floor(p.x+dx),Math.floor(p.y+dy)].join(','))));
  const clearApproach=medusa=>[[1,0],[-1,0],[0,1],[0,-1]].map(([dx,dy])=>Array.from({length:6},(_,i)=>({x:medusa.pos.x+dx*i*.5,y:medusa.pos.y+dy*i*.5}))).find(line=>line.every(clear))?.at(-1);
  const medusa=map.entities.find(e=>e.kind===0x654&&choose(medusaDraw(e.id,preparedSeed))&&clearApproach(e));
  assert(medusa,'No ordinary Medusa matches the independent draw and clear approach');
  const approach=clearApproach(medusa);
  await writeFile(join(process.env.QA_RESULTS_DIR,'prepared-abyss.json'),JSON.stringify({provenance:'prepared: ordinary Realm and actor-local RNG plus starting placement',worldSeed:info.prepared_seed,medusa,draw:medusaDraw(medusa.id,preparedSeed),approach}));
  return {...spec,position:approach,medusa:medusa.id};
}
export async function run(game){await abyssJourney(game,medusaId,true);}
export async function abyssJourney(game,medusaId,expectPortal){
  const out=process.env.QA_RESULTS_DIR,root=join(out,'native'),start=await game.observe();let serial=0,protectedUntil=0;
  async function command(command){const path=join(out,'request.json');await writeFile(path,JSON.stringify({id:'entry-'+serial++,command}));return JSON.parse(execFileSync(process.execPath,[resolve('scripts/game.mjs'),join(root,'native.json'),path],{encoding:'utf8'}));}
  async function protect(){
    const v=await game.observe();assert(v.lifecycle.alive);
    if(!protectedUntil){await command({op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invincible',duration_ms:60000}});protectedUntil=v.game_ms+60000;}
    if(v.game_ms>=protectedUntil-5000){
      await game.act({keys:[],duration_ms:Math.max(50,protectedUntil-v.game_ms)});
      const now=await game.observe();assert(now.lifecycle.alive&&now.game_ms>=protectedUntil);
      await command({op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invincible',duration_ms:60000}});
      protectedUntil=now.game_ms+60000;
    }
  }
  async function walk(goal){
    for(let n=0;n<300;n++){
      const v=await game.observe(),dx=goal.x-v.position.x,dy=goal.y-v.position.y;if(Math.hypot(dx,dy)<.6)return;
      const x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2;await protect();
      await game.act({keys:[Math.abs(x)>.2?(x>0?'d':'a'):null,Math.abs(y)>.2?(y>0?'s':'w'):null].filter(Boolean),duration_ms:Math.max(50,Math.min(500,Math.round(Math.hypot(dx,dy)/7.5*1000)))});
    }
    throw Error('Native approach blocked');
  }
  const prior=new Set((await game.inspect()).diagnostic.entities.filter(e=>e.kind===0x71b).map(e=>e.id));
  for(let n=0;n<20;n++){
    await protect();const s=await game.inspect(),enemy=s.diagnostic.entities.find(e=>e.id===medusaId);if(!enemy)break;
    if(Math.hypot(enemy.pos.x-s.view.position.x,enemy.pos.y-s.view.position.y)>4)await walk(enemy.pos);
    await game.act([{actor:game.actor,input:{keys:s.view.character.mp>=85?[' ']:[],duration_ms:500,target:{entity:medusaId,camera_angle:-Math.PI/4,fire:true}}},{actor:game.actor,input:{keys:[],duration_ms:600}}]);
  }
  await game.act({keys:[],duration_ms:700});
  const dead=await game.inspect(),portal=dead.diagnostic.entities.find(e=>e.kind===0x71b&&!prior.has(e.id));
  const events=(await readFile(join(root,'server.log'),'utf8')).split('\n').filter(s=>s.startsWith('{')).map(s=>JSON.parse(s).fields);
  assert(events.some(e=>e.message==='enemy defeated'&&e.entity===medusaId&&e.owner===start.session.player),'Native player did not defeat the pinned Medusa');
  const published=events.find(e=>e.message==='XML terminal effects published from exact durable journal receipt'&&e.actor===medusaId);
  assert(published,'Medusa terminal publication was not logged');
  const db=new DatabaseSync(join(root,'state/game.db'),{readOnly:true});
  let terminal;try{terminal=db.prepare('SELECT status,payload FROM world_events WHERE event = ?').get(published.event);}finally{db.close();}
  const births=terminal&&JSON.parse(terminal.payload).payload.data.actions?.publication?.births;
  const journaled=Boolean(births?.some(([kind,pos])=>kind===0x71b&&(!portal||pos.x===portal.pos.x&&pos.y===portal.pos.y)));
  game.check('the native Medusa death and journal agree with the independent portal expectation',terminal?.status===2&&journaled===expectPortal&&(expectPortal?portal?.published:!portal));
  if(!expectPortal) return;
  for(let n=0;n<60;n++){
    const v=await game.observe(),dx=portal.pos.x-v.position.x,dy=portal.pos.y-v.position.y;if(Math.hypot(dx,dy)<.6)break;
    const x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2;
    await game.act({keys:[Math.abs(x)>.2?(x>0?'d':'a'):null,Math.abs(y)>.2?(y>0?'s':'w'):null].filter(Boolean),duration_ms:80});
  }
  const at=await game.observe();game.check('native walking reaches the earned portal',Math.hypot(at.position.x-portal.pos.x,at.position.y-portal.pos.y)<1);
  await game.act({keys:[],duration_ms:800},{name:'abyss-earned-portal',when:()=>true});
  await game.act({keys:[],mouse:{x:700,y:575},duration_ms:1700});
  const entered=await game.inspect();
  game.check('earned portal enters Abyss with the same living character',entered.view.map.name==='Abyss of Demons'&&entered.view.session.character===start.session.character&&entered.view.lifecycle.alive&&entered.view.map.allow_teleport);
  await game.act({keys:[],duration_ms:900},{name:'abyss-earned-entry',when:()=>true});
  await game.act({keys:[],mouse:{x:700,y:575},duration_ms:1700});
  const returned=await game.inspect();
  game.check('Cowardice escape returns to the original Realm without clear credit',returned.view.session.world===start.session.world&&returned.view.lifecycle.alive&&returned.diagnostic.character.abysses===0);
  await writeFile(join(out,'earned-entry.json'),JSON.stringify({medusaId,terminalEvent:published.event,portal,entered,returned,scope:'Prepared level, gear, invincibility, approach and seeded ordinary Realm RNG. Native defeat, durable XML portal roll, walking, entry and exit; not a drop-rate estimate.'}));
}
