import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
const roots=[0x62f,0x631,0x62d,0x62e,0x632,0x633];
const spec={class:0x30e,level:20,seed:838,world:1,equipment:[0xb08,-1,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,75,0,50,75,40,60]};
const output=name=>join(process.env.QA_RESULTS_DIR,name);
const native=name=>output('native/'+name);
let camps;
export async function prepare(api){
  const report=JSON.parse(await readFile(native('result.json'),'utf8'));
  const info=await api({op:'world_info',world:spec.world}),[x,y]=info.spawn_points[0];
  const scout=await api({op:'spawn',spec:{...spec,guid:'golem-scout',position:{x,y},controller:'direct',profile:null}});
  const state=await api({op:'inspect',actor:scout.result.actor});
  const token=(await readFile(native('state/control.token'),'utf8')).trim();
  const response=await fetch(report.origin+'/dev/map?player='+state.diagnostic.player,{headers:{Authorization:'Bearer '+token}});assert(response.ok);
  const map=await response.json(),blocked=new Set(map.blocked.map(p=>p.join(',')));
  await writeFile(output('natural-map.json'),JSON.stringify(map));
  camps=roots.map(kind=>{
    for(const root of map.entities.filter(e=>e.kind===kind)){
      for(let dx=-3;dx<=3;dx++)for(let dy=-3;dy<=3;dy++){
        if(Math.hypot(dx,dy)<2||Math.hypot(dx,dy)>3)continue;
        const position={x:Math.floor(root.pos.x)+dx+.5,y:Math.floor(root.pos.y)+dy+.5};
        if(!blocked.has([Math.floor(position.x),Math.floor(position.y)].join(',')))return {root,position,members:map.entities.filter(e=>e.camp===root.camp)};
      }
    }
    throw Error('No walkable natural camp for '+kind.toString(16));
  });
  await writeFile(output('natural-camps.json'),JSON.stringify({world:spec.world,layout:info.realm_layout,camps}));
  return {...spec,position:camps[0].position};
}
export async function run(game){
  const samples=[],shots=new Map(),encounters=[];let serial=0,protectedUntil=0;
  const command=async change=>{
    const path=output('mutation-'+serial+'.json');
    await writeFile(path,JSON.stringify({id:'golem-'+serial++,command:{op:'cheat',actor:game.actor,change}}));
    execFileSync(process.execPath,[resolve('scripts/game.mjs'),native('native.json'),path]);
  };
  const family=(s,camp)=>s.diagnostic.entities.filter(e=>e.camp===camp);
  async function step(input,capture){
    let pending,error;
    const record=s=>{samples.push(s);for(const p of s.view.projectiles)shots.set(p.owner+':'+p.id,p);};
    const timer=setInterval(()=>{if(!pending)pending=game.inspect().then(record).catch(e=>{error=e;}).finally(()=>{pending=null;});},100);
    try{await game.act(input,capture);}finally{clearInterval(timer);if(pending)await pending;}
    if(error)throw error;const s=await game.inspect();record(s);return s;
  }
  async function act(input,capture){
    async function protect(){if((await game.observe()).game_ms>=protectedUntil){await command({kind:'protection',condition:'invulnerable',duration_ms:60000});protectedUntil=(await game.observe()).game_ms+60000;}}
    await protect();
    while(protectedUntil-(await game.observe()).game_ms<input.duration_ms+500){await step({keys:[],duration_ms:500});await protect();}
    return step(input,capture);
  }
  try{
    for(const {root,position,members} of camps){
      const tag=root.kind.toString(16),camp=root.camp,start=samples.length;
      if(root.kind!==roots[0])await command({kind:'teleport',position});
      const arrival=await game.inspect();
      game.check(tag+': natural root and four satellites admitted',family(arrival,camp).some(e=>e.id===root.id&&e.hp===220)&&members.filter(e=>e.kind>=0x6d2&&e.kind<=0x6d7).length===4);
      await act({keys:[],duration_ms:2000},{name:'golem-'+tag,when:()=>true});
      await act({keys:[],duration_ms:2500});
      const combat=samples.slice(start),ids=new Set(members.map(e=>e.id));
      game.check(tag+': root activates and moves',combat.some(s=>family(s,camp).some(e=>e.id===root.id&&e.activated!==null&&Math.hypot(e.pos.x-root.pos.x,e.pos.y-root.pos.y)>.2)));
      game.check(tag+': native combat observes root projectile',[...shots.values()].some(p=>p.owner===root.id));
      for(const kind of [0x6d5,0x6d6,0x6d7])game.check(tag+': gray '+kind.toString(16)+' fires selected missile',[...shots.values()].some(p=>members.some(e=>e.id===p.owner&&e.kind===kind)&&p.speed===65&&p.lifetime_ms===2500));
      const hub=members.find(e=>e.kind>=0x6d2&&e.kind<=0x6d4);assert(hub);
      game.check(tag+': colored hub orbits its own root',combat.some(s=>{const f=family(s,camp),h=f.find(e=>e.id===hub.id),r=f.find(e=>e.id===root.id);return h&&r&&Math.abs(Math.hypot(h.pos.x-r.pos.x,h.pos.y-r.pos.y)-3)<.15;}));
      game.check(tag+': gray satellites move around owned hub',combat.some(s=>{const f=family(s,camp),h=f.find(e=>e.id===hub.id);return h&&f.some(e=>e.kind>=0x6d5&&e.kind<=0x6d7&&Math.abs(Math.hypot(e.pos.x-h.pos.x,e.pos.y-h.pos.y)-1)<.15);}));
      for(let n=0;n<25&&family(await game.inspect(),camp).some(e=>e.id===root.id);n++)await act({keys:[],duration_ms:300,target:{entity:root.id,camera_angle:-Math.PI/4,fire:true}});
      const defeated=await game.inspect();
      game.check(tag+': ordinary native fire defeats root',!family(defeated,camp).some(e=>e.id===root.id));
      const survivors=family(defeated,camp).filter(e=>e.kind>=0x6d2&&e.kind<=0x6d7);
      game.check(tag+': satellites survive root death with bounded cleanup',survivors.length>0&&survivors.every(e=>e.expires>defeated.view.time_ms&&e.expires<=defeated.view.time_ms+30000));
      // This verifies the retained Trapped reconstruction, not Kithio's conflicting 16-second decay: https://github.com/trapped/rotmg_svr/blob/593e490803fe0ad168ead408cacd5287d778acfe/wServer/logic/cond/GolemSatellites.cs
      const target=survivors.find(e=>e.id===hub.id);assert(target,'Colored satellite must survive root defeat');
      const xp=defeated.view.character.experience;
      for(let n=0;n<40&&family(await game.inspect(),camp).some(e=>e.id===target.id);n++)await act({keys:[],duration_ms:250,target:{entity:target.id,camera_angle:-Math.PI/4,fire:true}});
      const satelliteDead=await game.inspect(),remaining=family(satelliteDead,camp).filter(e=>survivors.some(p=>p.id===e.id));
      game.check(tag+': native satellite defeat awards ten XP each',!remaining.some(e=>e.id===target.id)&&satelliteDead.view.character.experience-xp===10*(survivors.length-remaining.length));
      const cleanupXp=satelliteDead.view.character.experience;
      for(let n=0;n<4;n++)await act({keys:[],duration_ms:10000});
      const end=await game.inspect();
      game.check(tag+': orphan cleanup removes satellites without kill XP',!family(end,camp).some(e=>ids.has(e.id)&&e.kind>=0x6d2&&e.kind<=0x6d7)&&end.view.character.experience===cleanupXp);
      game.check(tag+': same prepared living Wizard completes encounter',end.view.lifecycle.alive&&end.view.session.character===arrival.view.session.character);
      encounters.push({kind:root.kind,arrival,defeated,satelliteDead,end});
    }
  }finally{await writeFile(output('combat.json'),JSON.stringify({encounters,samples,shots:[...shots.values()]}));}
}
