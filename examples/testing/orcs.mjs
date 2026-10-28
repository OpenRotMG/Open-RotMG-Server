import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
const spec={class:0x30e,level:20,seed:830,world:1,equipment:[0xa9f,-1,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,0,0,50,0,40,60]};
const root=()=>join(process.env.QA_RESULTS_DIR,'native');
let king;
export async function prepare(api){
  const info=await api({op:'world_info',world:1}),[x,y]=info.spawn_points[0];
  const scout=await api({op:'spawn',spec:{...spec,guid:'orc-scout',position:{x,y},controller:'direct',profile:null}});
  const state=await api({op:'inspect',actor:scout.result.actor});
  const report=JSON.parse(await readFile(join(root(),'result.json'),'utf8'));
  const headers={Authorization:'Bearer '+(await readFile(join(root(),'state/control.token'),'utf8')).trim()};
  const response=await fetch(report.origin+'/dev/map?player='+state.diagnostic.player,{headers});assert(response.ok);
  const map=await response.json(),blocked=new Set(map.blocked.map(p=>p.join(',')));
  for(const e of map.entities.filter(e=>e.kind===0x62c))for(let dx=-4;dx<=4;dx++)for(let dy=-4;dy<=4;dy++){
    const position={x:Math.floor(e.pos.x)+dx+.5,y:Math.floor(e.pos.y)+dy+.5};
    if(Math.hypot(dx,dy)<2||Math.hypot(dx,dy)>4)continue;
    if(Array.from({length:32},(_,i)=>i*Math.PI/16).every(a=>!blocked.has([Math.floor(position.x+2*Math.cos(a)),Math.floor(position.y+2*Math.sin(a))].join(',')))){
      king=e;await writeFile(join(process.env.QA_RESULTS_DIR,'natural-camp.json'),JSON.stringify(map.entities.filter(m=>m.camp===e.camp)));
      return {...spec,position};
    }
  }
  throw Error('No natural MidPlains Orc camp with clear combat space');
}
export async function run(game){
  const samples=[],shots=new Map();let protectedUntil=0,serial=0;
  const family=s=>s.diagnostic.entities.filter(e=>e.camp===king.camp),entity=(s,id)=>family(s).find(e=>e.id===id);
  const save=async name=>{const s=await game.inspect();await writeFile(join(process.env.QA_RESULTS_DIR,name+'.json'),JSON.stringify(s));return s;};
  const record=s=>{samples.push(s);for(const p of s.view.projectiles)shots.set(p.owner+':'+p.id,p);};
  async function step(input,capture){
    let pending=null,error;
    const timer=setInterval(()=>{if(!pending)pending=game.inspect().then(record).catch(e=>{error=e;}).finally(()=>{pending=null;});},100);
    try{await game.act(input,capture);}finally{clearInterval(timer);if(pending)await pending;}
    if(error)throw error;const s=await game.inspect();record(s);return s;
  }
  async function cheat(change){
    const path=join(process.env.QA_RESULTS_DIR,'cheat.json');
    await writeFile(path,JSON.stringify({id:'orc-cheat-'+serial++,command:{op:'cheat',actor:game.actor,change}}));
    execFileSync(process.execPath,[resolve('scripts/game.mjs'),join(root(),'native.json'),path]);
  }
  async function protect(){
    const now=(await game.observe()).game_ms;if(now<protectedUntil)return;
    await cheat({kind:'protection',condition:'invulnerable',duration_ms:60000});protectedUntil=now+60000;
  }
  async function act(input,capture){
    await protect();while(protectedUntil-(await game.observe()).game_ms<input.duration_ms+500){await step({keys:[],duration_ms:100});await protect();}
    return step(input,capture);
  }
  const begin=await save('arrival');
  game.check('natural MidPlains camp contains selected King240, Warriors90, Mage80 and Queen130',[[0x62c,1,240],[0x629,2,90],[0x62a,1,80],[0x62b,1,130]].every(([k,n,hp])=>family(begin).filter(e=>e.kind===k&&e.hp===hp).length===n));
  await step({keys:[],duration_ms:500},{name:'orcs-arrival',when:()=>true});
  for(let n=0;n<12&&!samples.some(s=>s.view.character.hp<begin.view.character.hp);n++)await step({keys:[],duration_ms:250});
  game.check('native character takes ordinary Orc damage',samples.some(s=>s.view.character.hp<begin.view.character.hp));
  for(let n=0;n<24;n++)await act({keys:[],duration_ms:500});
  await act({keys:[],duration_ms:500},{name:'orcs-combat',when:()=>true});
  for(const [kind,speed,lifetime] of [[0x62c,70,1200],[0x629,70,1200],[0x62a,60,3000]]){
    const ids=new Set(samples.flatMap(family).filter(e=>e.kind===kind).map(e=>e.id));
    game.check(kind+': natural actor activates, moves and fires selected projectiles',samples.some(s=>family(s).some(e=>ids.has(e.id)&&e.activated!==null&&family(begin).some(p=>p.id===e.id&&Math.hypot(p.pos.x-e.pos.x,p.pos.y-e.pos.y)>.2)))&&[...shots.values()].some(p=>ids.has(p.owner)&&p.speed===speed&&p.lifetime_ms===lifetime));
  }
  const queens=new Set(samples.flatMap(family).filter(e=>e.kind===0x62b).map(e=>e.id));
  game.check('support Queen moves without firing',samples.some(s=>family(s).some(e=>e.kind===0x62b&&family(begin).some(p=>p.id===e.id&&Math.hypot(p.pos.x-e.pos.x,p.pos.y-e.pos.y)>.2)))&&![...shots.values()].some(p=>queens.has(p.owner)));
  const caps=s=>[[0x62c,1],[0x629,3],[0x62a,2],[0x62b,2]].every(([k,n])=>family(s).filter(e=>e.kind===k).length<=n);
  game.check('current King-owned roles fill to 3/2/2 within caps',[[0x629,3],[0x62a,2],[0x62b,2]].every(([k,n])=>family(samples.at(-1)).filter(e=>e.kind===k).length===n));
  // The 300 in Kithio is a cooldown, not HP; this case verifies our older 100-HP reconstruction, not fidelity: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/logic/behaviors/Heal.cs
  const healStart=samples.length;
  for(let n=0;n<8;n++){
    await act({keys:[],duration_ms:100,target:{entity:king.id,camera_angle:-Math.PI/4,fire:true}});
    await act({keys:[],duration_ms:1000});
    if(samples.slice(healStart).some((s,i,a)=>i&&entity(s,king.id)?.hp>entity(a[i-1],king.id)?.hp))break;
  }
  await save('king-healed');
  game.check('wounded living King receives support healing after native staff damage',samples.slice(healStart).some((s,i,a)=>i&&entity(s,king.id)?.hp>entity(a[i-1],king.id)?.hp));
  await cheat({kind:'stats',values:[670,385,75,0,50,75,40,60]});
  async function defeat(id){
    const before=await game.inspect();
    for(let n=0;n<100&&entity(await game.inspect(),id);n++)await act({keys:[],duration_ms:100,target:{entity:id,camera_angle:-Math.PI/4,fire:true}});
    const after=await game.inspect();
    game.check(id+': native staff defeats target and earns XP',!entity(after,id)&&after.view.character.experience>before.view.character.experience);
    return after;
  }
  for(const kind of [0x629,0x62a,0x62b]){
    const before=await game.inspect(),victim=family(before).find(e=>e.kind===kind);assert(victim);
    const old=new Set(family(before).map(e=>e.id));await defeat(victim.id);
    for(let n=0;n<14&&!family(await game.inspect()).some(e=>e.kind===kind&&!old.has(e.id));n++)await act({keys:[],duration_ms:1000});
    const after=await save('replenished-'+kind);
    game.check(kind+': living King replenishes defeated role within current cap',entity(after,king.id)&&family(after).some(e=>e.kind===kind&&!old.has(e.id))&&caps(after));
  }
  await defeat(king.id);const dead=await save('king-defeated');
  await act({keys:[],duration_ms:500},{name:'orcs-defeated',when:()=>true});
  for(let n=0;n<12;n++)await act({keys:[],duration_ms:1000});
  const end=await save('survivors');
  game.check('King defeat preserves followers but stops all replacement births',family(end).length>0&&family(end).every(e=>family(dead).some(p=>p.id===e.id))&&!entity(end,king.id));
  game.check('King XP cannot be duplicated after death',end.view.character.experience===dead.view.character.experience);
  game.check('same supplied living native Wizard completes encounter',end.view.lifecycle.alive&&end.view.session.character===begin.view.session.character);
  game.check('bounded natural camp caps hold throughout',samples.every(caps));
  await writeFile(join(process.env.QA_RESULTS_DIR,'combat.json'),JSON.stringify({samples,shots:[...shots.values()]}));
}
