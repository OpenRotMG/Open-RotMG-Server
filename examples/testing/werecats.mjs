import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
const spec={class:0x30e,level:20,seed:837,world:1,equipment:[0xa9f,0xa2e,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,75,25,50,75,40,60]};
const root=()=>join(process.env.QA_RESULTS_DIR,'native');
let lion;
export async function prepare(api){
  const info=await api({op:'world_info',world:spec.world}),[x,y]=info.spawn_points[0];
  const scout=await api({op:'spawn',spec:{...spec,guid:'werecat-scout',position:{x,y},controller:'direct',profile:null}});
  const state=await api({op:'inspect',actor:scout.result.actor});
  const report=JSON.parse(await readFile(join(root(),'result.json'),'utf8'));
  const headers={Authorization:'Bearer '+(await readFile(join(root(),'state/control.token'),'utf8')).trim()};
  const response=await fetch(report.origin+'/dev/map?player='+state.diagnostic.player,{headers});assert(response.ok);
  const map=await response.json(),blocked=new Set(map.blocked.map(p=>p.join(',')));
  await writeFile(join(process.env.QA_RESULTS_DIR,'natural-map.json'),JSON.stringify(map));
  lion=map.entities.find(e=>e.kind===0x637&&[-2,-1,0,1,2].every(dx=>[-2,-1,0,1,2].every(dy=>!blocked.has([Math.floor(e.pos.x)+dx,Math.floor(e.pos.y)+dy].join(',')))));
  assert(lion,'Sampled natural Realm lacks a Werecat camp with a clear approach');
  await writeFile(join(process.env.QA_RESULTS_DIR,'natural-camp.json'),JSON.stringify({world:spec.world,layout:info.realm_layout,members:map.entities.filter(e=>e.camp===lion.camp)}));
  return {...spec,position:{x:lion.pos.x-2,y:lion.pos.y}};
}
export async function run(game){
  const samples=[],shots=new Map();
  const family=s=>s.diagnostic.entities.filter(e=>e.camp===lion.camp);
  const save=async name=>{const s=await game.inspect();await writeFile(join(process.env.QA_RESULTS_DIR,name+'.json'),JSON.stringify(s));return s;};
  let sampling=true;
  const record=s=>{samples.push(s);for(const p of s.view.projectiles)shots.set(p.owner+':'+p.id,p);};
  async function act(input,capture){
    let pending,error;
    const timer=setInterval(()=>{if(sampling&&!pending)pending=game.inspect().then(record).catch(e=>{error=e;}).finally(()=>{pending=null;});},25);
    try{await game.act(input,capture);}finally{clearInterval(timer);if(pending)await pending;}
    if(error)throw error;const s=await game.inspect();record(s);return s;
  }
  try{
    const begin=await save('arrival'),initial=family(begin);
    for(const [kind,min,max,hp] of [[0x637,1,1,400],[0x635,1,3,220],[0x636,1,2,220],[0x639,1,2,240]]){
      const members=initial.filter(e=>e.kind===kind);
      game.check(kind+': natural population and selected HP',members.length>=min&&members.length<=max&&members.every(e=>e.hp===hp));
    }
    await act({keys:[],duration_ms:1500},{name:'werecats-arrival',when:()=>true});
    game.check('native enemy fire damages the exposed Wizard',(await game.observe()).character.hp<begin.view.character.hp);
    const path=join(process.env.QA_RESULTS_DIR,'protection.json');
    await writeFile(path,JSON.stringify({id:'werecat-protection',command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}}));
    execFileSync(process.execPath,[resolve('scripts/game.mjs'),join(root(),'native.json'),path]);
    for(let n=0;n<24;n++)await act({keys:[],duration_ms:250},n===16?{name:'werecats-attacks',when:()=>true}:undefined);
    for(const [kind,speed,lifetime] of [[0x635,65,2200],[0x636,80,1000],[0x639,70,2000],[0x637,55,2700]]){
      const ids=new Set(initial.filter(e=>e.kind===kind).map(e=>e.id));
      game.check(kind+': activation and movement observed',samples.some(s=>family(s).some(e=>ids.has(e.id)&&e.activated!==null&&initial.some(p=>p.id===e.id&&Math.hypot(e.pos.x-p.pos.x,e.pos.y-p.pos.y)>.1))));
      game.check(kind+': selected projectile observed',[...shots.values()].some(p=>ids.has(p.owner)&&p.speed===speed&&p.lifetime_ms===lifetime));
    }
    game.check('Werelion alternates its second selected projectile',[...shots.values()].some(p=>p.owner===lion.id&&p.speed===60&&p.lifetime_ms===2500));
    game.check('Tiger and Leopard choose both current chase/orbit stages',[0x635,0x636].every(k=>[0,1].every(stage=>samples.some(s=>family(s).some(e=>e.kind===k&&e.stage===stage)))));
    // Movement resumes in the teleport tick, so compare the spatial reset, not exact equality: https://github.com/trapped/rotmg_svr/blob/593e490803fe0ad168ead408cacd5287d778acfe/wServer/logic/movement/Planewalk.cs
    game.check('Panther visibly relocates to the player on its seven-second clock',samples.some((s,i)=>i>0&&family(s).some(e=>{
      if(e.kind!==0x639||e.next_aux!==e.activated+14000||s.view.time_ms-e.activated>7100)return false;
      const before=samples[i-1],p=family(before).find(p=>p.id===e.id);
      return p?.next_aux===0&&s.view.time_ms-before.view.time_ms<=100&&Math.hypot(p.pos.x-before.view.position.x,p.pos.y-before.view.position.y)>.8&&Math.hypot(e.pos.x-s.view.position.x,e.pos.y-s.view.position.y)<.3;
    })));
    sampling=false;
    await act({keys:[],duration_ms:10000});
    const filled=await save('filled'),tigers=family(filled).filter(e=>e.kind===0x635),beforeIds=new Set(tigers.map(e=>e.id));
    game.check('current living Tiger slots reach their cap before the replacement test',tigers.length===3);
    const victim=tigers.sort((a,b)=>Math.hypot(b.pos.x-lion.pos.x,b.pos.y-lion.pos.y)-Math.hypot(a.pos.x-lion.pos.x,a.pos.y-lion.pos.y))[0];assert(victim);
    for(let n=0;n<60;n++){
      const s=await game.inspect(),cat=family(s).find(e=>e.id===victim.id);if(!cat)break;
      const owner=family(s).find(e=>e.id===lion.id);assert(owner,'Owner died before follower check');
      const p=s.view.position,ownerBehindAim=(cat.pos.x-p.x)*(owner.pos.x-p.x)+(cat.pos.y-p.y)*(owner.pos.y-p.y)<-.5;
      await act(ownerBehindAim?{keys:[],duration_ms:50,target:{entity:cat.id,camera_angle:-Math.PI/4,fire:true}}:{keys:[],duration_ms:100});
      if(ownerBehindAim)await act({keys:[],duration_ms:300});
    }
    game.check('native staff defeats a Tiger while owner lives',!family(await game.inspect()).some(e=>e.id===victim.id)&&family(await game.inspect()).some(e=>e.id===lion.id));
    await act({keys:[],duration_ms:10000});
    const refill=await save('reinforced');
    game.check('living owner replaces an actually vacated Tiger slot',family(refill).filter(e=>e.kind===0x635).length===3&&family(refill).some(e=>e.kind===0x635&&!beforeIds.has(e.id)));
    for(let n=0;n<30&&family(await game.inspect()).some(e=>e.id===lion.id);n++)await act({keys:[],duration_ms:200,target:{entity:lion.id,camera_angle:-Math.PI/4,fire:true}});
    const orphaned=await save('owner-defeated');
    game.check('ordinary native staff defeats owner',!family(orphaned).some(e=>e.id===lion.id));
    const survivors=new Set(family(orphaned).map(e=>e.id));
    for(let n=0;n<120;n++){
      const victim=family(await game.inspect())[0];if(!victim)break;
      await act({keys:[],duration_ms:200,target:{entity:victim.id,camera_angle:-Math.PI/4,fire:true}});
    }
    const cleared=await save('family-defeated');
    game.check('native staff defeats all surviving owned cats',family(cleared).length===0&&survivors.size>0);
    game.check('combat awards XP',cleared.view.character.experience>begin.view.character.experience);
    await act({keys:[],duration_ms:10000},{name:'werecats-defeated',when:()=>true});
    const end=await save('final');
    game.check('dead owner cannot replenish its camp',family(end).length===0);
    game.check('defeat awards no duplicate XP',end.view.character.experience===cleared.view.character.experience);
    game.check('same prepared living Wizard completes the encounter',end.view.lifecycle.alive&&end.view.session.character===begin.view.session.character);
  }finally{await writeFile(join(process.env.QA_RESULTS_DIR,'combat.json'),JSON.stringify({samples,shots:[...shots.values()]}));}
}
