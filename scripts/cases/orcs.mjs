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
    // Inside the King's Shoot range 3 (actors.xml:864): StayAbove 85 can hold him back from lower ground.
    if(Math.hypot(dx,dy)<2||Math.hypot(dx,dy)>3)continue;
    if(Array.from({length:32},(_,i)=>i*Math.PI/16).every(a=>!blocked.has([Math.floor(position.x+2*Math.cos(a)),Math.floor(position.y+2*Math.sin(a))].join(',')))){
      king=e;await writeFile(join(process.env.QA_RESULTS_DIR,'natural-camp.json'),JSON.stringify(map.entities.filter(m=>m.id===e.id||m.parent===e.id)));
      return {...spec,position};
    }
  }
  throw Error('No natural MidPlains Orc camp with clear combat space');
}
export async function run(game){
  const samples=[],shots=new Map();let protectedUntil=0,serial=0;
  // XML ownership (actors.xml:852-853,868): the King owns Queens, each Queen owns its Mages and Warriors; the natural camp is separate.
  const family=s=>{
    const top=s.diagnostic.entities.filter(e=>e.id===king.id||e.parent===king.id),queens=new Set(top.filter(e=>e.kind===0x62b).map(e=>e.id));
    return top.concat(s.diagnostic.entities.filter(e=>queens.has(e.parent)&&!top.includes(e)));
  },entity=(s,id)=>family(s).find(e=>e.id===id),owned=(s,parent,kind)=>family(s).filter(e=>e.parent===parent&&e.kind===kind);
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
  try{
  const begin=await save('arrival');
  // Recovered realm/mid.xml:826: offspring belong to the XML spawner, not an ordinary population slot.
  game.check('ordinary root is placed before it owns any offspring',begin.diagnostic.entities.some(e=>e.id===king.id&&e.camp===null&&e.parent===null&&e.hp===240)&&!begin.diagnostic.entities.some(e=>e.parent===king.id));
  // The King now stands inside his Shoot range 3 with his whole escort: stop exposure at the first hit.
  await step({keys:[],duration_ms:100},{name:'orcs-arrival',when:()=>true});
  for(let n=0;n<40&&!samples.some(s=>s.view.character.hp<begin.view.character.hp);n++)await step({keys:[],duration_ms:100});
  game.check('native character takes ordinary Orc damage',samples.some(s=>s.view.character.hp<begin.view.character.hp));
  for(let n=0;n<24;n++)await act({keys:[],duration_ms:500});
  await act({keys:[],duration_ms:500},{name:'orcs-combat',when:()=>true});
  const first=new Map();for(const s of samples)for(const e of family(s))if(!first.has(e.id))first.set(e.id,e.pos);
  for(const [kind,speed,lifetime] of [[0x62c,70,1200],[0x629,70,1200],[0x62a,60,3000]]){
    const ids=new Set(samples.flatMap(family).filter(e=>e.kind===kind).map(e=>e.id));
    game.check(kind+': actor activates, moves from where it first appeared and fires selected projectiles',samples.some(s=>family(s).some(e=>ids.has(e.id)&&e.activated!==null&&Math.hypot(first.get(e.id).x-e.pos.x,first.get(e.id).y-e.pos.y)>.2))&&[...shots.values()].some(p=>ids.has(p.owner)&&p.speed===speed&&p.lifetime_ms===lifetime));
  }
  const queens=new Set(samples.flatMap(family).filter(e=>e.kind===0x62b).map(e=>e.id));
  game.check('support Queen activates without firing while protecting its King',samples.some(s=>family(s).some(e=>e.kind===0x62b&&e.activated!==null&&entity(s,king.id)&&Math.hypot(e.pos.x-entity(s,king.id).pos.x,e.pos.y-entity(s,king.id).pos.y)<=7.05))&&![...shots.values()].some(p=>queens.has(p.owner)));
  const mages=new Set(samples.flatMap(family).filter(e=>e.kind===0x62a).map(e=>e.id));
  // Recovered Orc Mage: single aimed shots in circle_player, three-way 120-degree bursts in circle_queen (actors.xml:833,840).
  game.check('native Mage fires three-way 120-degree bursts',samples.some(s=>s.view.projectiles.some(p=>mages.has(p.owner)&&[1,-1].every(sign=>s.view.projectiles.some(q=>q.owner===p.owner&&Math.hypot(p.start.x-q.start.x,p.start.y-q.start.y)<.01&&Math.abs(Math.atan2(Math.sin(q.angle-p.angle),Math.cos(q.angle-p.angle))-sign*2*Math.PI/3)<.001)))));
  const caps=s=>owned(s,king.id,0x62b).length<=2&&family(s).filter(e=>e.kind===0x62b).every(q=>owned(s,q.id,0x62a).length<=2&&owned(s,q.id,0x629).length<=3);
  game.check('King owns two Queens and a Queen fills two Mages and three Warriors',owned(samples.at(-1),king.id,0x62b).length===2&&family(samples.at(-1)).some(q=>q.kind===0x62b&&owned(samples.at(-1),q.id,0x62a).length===2&&owned(samples.at(-1),q.id,0x629).length===3));
  // The 300 in Kithio is a cooldown, not HP; this case verifies our older 100-HP reconstruction, not fidelity: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/logic/behaviors/Heal.cs
  const healStart=samples.length;
  for(let n=0;n<8;n++){
    await act({keys:[],duration_ms:100,target:{entity:king.id,camera_angle:-Math.PI/4,fire:true}});
    await act({keys:[],duration_ms:1000});
    if(samples.slice(healStart).some((s,i,a)=>i&&entity(s,king.id)?.hp>entity(a[i-1],king.id)?.hp))break;
  }
  await save('king-healed');
  game.check('wounded living King receives support healing after native staff damage',samples.slice(healStart).some((s,i,a)=>i&&entity(s,king.id)?.hp>entity(a[i-1],king.id)?.hp));
  async function defeat(id){
    const before=await game.inspect();
    for(let n=0;n<100;n++){
      const s=await game.inspect(),victim=entity(s,id),boss=entity(s,king.id);if(!victim)break;
      const p=s.view.position,dx=victim.pos.x-p.x,dy=victim.pos.y-p.y;
      const move=async(x,y)=>act({keys:[[['d',1,-1],['s',1,1],['a',-1,1],['w',-1,-1]].sort((a,b)=>(b[1]-a[1])*x+(b[2]-a[2])*y)[0][0]],duration_ms:150});
      if(Math.hypot(dx,dy)>4){await move(dx,dy);continue;}
      if(id!==king.id){
        assert(boss,'King must survive escort targeting');
        const t=((boss.pos.x-p.x)*dx+(boss.pos.y-p.y)*dy)/(dx*dx+dy*dy);
        if(t>=0&&t<=1&&Math.hypot(boss.pos.x-p.x-t*dx,boss.pos.y-p.y-t*dy)<.8){await move(-dy,dx);continue;}
      }
      await act({keys:[],duration_ms:100,target:{entity:id,camera_angle:-Math.PI/4,fire:true}});
      await act({keys:[],duration_ms:500});
    }
    const after=await game.inspect();
    game.check(id+': native staff defeats target and earns XP',!entity(after,id)&&after.view.character.experience>before.view.character.experience);
    return after;
  }
  // Queens refill Mages/Warriors on an 8s cooldown, the King refills Queens on a 60s cooldown.
  for(const [kind,wait] of [[0x629,14],[0x62a,14],[0x62b,65]]){
    let before,victim;
    for(let n=0;n<14;n++){
      before=await game.inspect();victim=family(before).find(e=>e.kind===kind&&e.parent!==null&&entity(before,e.parent)&&before.view.objects.some(o=>o.id===e.id&&o.hp>0));if(victim)break;
      await act({keys:[],duration_ms:1000});
    }
    assert(victim,'No visible living owned escort after the ordinary refill window');
    const old=new Set(family(before).map(e=>e.id));await defeat(victim.id);
    for(let n=0;n<wait&&!owned(await game.inspect(),victim.parent,kind).some(e=>!old.has(e.id));n++)await act({keys:[],duration_ms:1000});
    const after=await save('replenished-'+kind);
    game.check(kind+': living owner replenishes defeated role within its XML cap',entity(after,king.id)&&owned(after,victim.parent,kind).some(e=>!old.has(e.id))&&caps(after));
  }
  await cheat({kind:'stats',values:[670,385,75,0,50,75,40,60]});
  await defeat(king.id);const dead=await save('king-defeated');
  await act({keys:[],duration_ms:500},{name:'orcs-defeated',when:()=>true});
  for(let n=0;n<12;n++)await act({keys:[],duration_ms:1000});
  const end=await save('survivors');
  game.check('King defeat preserves followers but stops Queen replacement',family(end).length>0&&!family(end).some(e=>e.kind===0x62b&&!family(dead).some(p=>p.id===e.id))&&!entity(end,king.id));
  game.check('surviving followers move after losing their King',family(end).some(e=>family(dead).some(p=>p.id===e.id&&Math.hypot(p.pos.x-e.pos.x,p.pos.y-e.pos.y)>.2)));
  game.check('King XP cannot be duplicated after death',end.view.character.experience===dead.view.character.experience);
  game.check('same supplied living native Wizard completes encounter',end.view.lifecycle.alive&&end.view.session.character===begin.view.session.character);
  game.check('bounded natural camp caps hold throughout',samples.every(caps));
  }finally{await writeFile(join(process.env.QA_RESULTS_DIR,'combat.json'),JSON.stringify({samples:samples.length,shots:[...shots.values()]}));}
}
