import assert from 'node:assert/strict';
import {nativeMap} from './native-map.mjs';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
import {route,walk as nativeWalk} from './native-route.mjs';

export const fixture={class:0x307,level:20,seed:12351,world:-2,position:'spawn',equipment:[0xc02,0xade,0xaf9,0xacd],backpack:[0xc2f,-1,-1,-1,-1,-1,-1,-1],stats:[700,252,75,25,50,50,40,50]};
export async function run(game){
  const output=name=>join(process.env.QA_RESULTS_DIR,name),samples=[],shots=new Map();
  const report=JSON.parse(await readFile(output('native/result.json'),'utf8'));
  const token=(await readFile(output('native/state/control.token'),'utf8')).trim();
  let protectedUntil=0,armoredUntil=0,recordShots=true;
  const distance=(a,b)=>Math.hypot(a.x-b.x,a.y-b.y);
  const entities=async()=>(await game.inspect()).diagnostic.entities;
  async function save(name){const state=await game.inspect();samples.push({name,state});await writeFile(output('lab-journey.json'),JSON.stringify(samples));return state;}
  async function api(command){
    const response=await fetch(report.origin+'/dev/testing',{method:'POST',signal:AbortSignal.timeout(10000),headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command})});
    const result=await response.json();assert(response.ok&&result.status!=='failed',JSON.stringify(result));return result;
  }
  const prepare=change=>api({op:'cheat',actor:game.actor,change});
  async function protect(){
    const v=await game.observe(),now=v.game_ms;
    if(!(v.character.conditions&(1<<23))){await prepare({kind:'protection',condition:'invincible',duration_ms:60000});protectedUntil=now+60000;}
    // Armor spans every protection expiry: loaded hosts stretch acts past it before renewal.
    if(protectedUntil-now<=30000&&now>=armoredUntil){await prepare({kind:'stat_modifier',index:3,amount:100,duration_ms:60000});armoredUntil=now+60000;}
    return protectedUntil-now;
  }
  async function act(input,capture){
    // Wait across the expiry in one bounded idle instead of spending the receipt budget on tiny polls.
    const ms=[input].flat().reduce((t,i)=>t+(i.input??i).duration_ms,0);
    for(let left;(left=await protect())<2*ms+5000;)await game.act({keys:[],duration_ms:Math.max(300,Math.min(10000,left+150))});
    let pending=null,error=null;
    const timer=recordShots?setInterval(()=>{if(!pending)pending=game.inspect().then(s=>{
      for(const p of s.view.projectiles){
        const e=s.diagnostic.entities.find(e=>e.id===p.owner);
        if(e&&e.kind>=0x979&&e.kind<=0x991)shots.set(p.owner+':'+p.id,{kind:e.kind,...p});
      }
    }).catch(e=>{error=e;}).finally(()=>{pending=null;});},100):null;
    try{return await game.act(input,capture);}
    finally{clearInterval(timer);if(pending)await pending;if(error)throw error;await writeFile(output('lab-projectiles.json'),JSON.stringify([...shots.values()]));}
  }
  const scene=async()=>nativeMap(report.origin,{Authorization:'Bearer '+token},(await game.observe()).session.player);
  // Lab Tables and Tesla Coils are destructible OccupySquare scenery; the generated rooms change every run.
  const walk=(to,reach,clearBlockedApproach=false)=>nativeWalk({client:{actor:game.actor,observe:()=>game.observe(),act},map:scene,to,ready:async()=>((await protect())-5000)/2,record:r=>writeFile(output('lab-route.json'),JSON.stringify(r)),
    options:m=>({reach,clearBlockedApproach,soft:new Map(m.entities.filter(e=>e.kind===0x188b||e.kind===0x188a).map(e=>[Math.floor(e.pos.y)*m.width+Math.floor(e.pos.x),e])),hazard:hexed(m)})});
  // A Hexxer's range-2 aura Stuns and Hexes (actors.xml Hexxer, madLab.xml:2045): route beside, never through, its pool.
  const hexed=m=>new Set(m.entities.filter(e=>e.kind===0x1e15).flatMap(e=>Array.from({length:49},(_,i)=>({x:Math.floor(e.pos.x)+i%7-3,y:Math.floor(e.pos.y)+Math.floor(i/7)-3}))
    .filter(t=>Math.hypot(t.x+.5-e.pos.x,t.y+.5-e.pos.y)<=2.75).map(t=>t.y*m.width+t.x)));
  // Shots still in flight when an act pauses can kill the target on resume, before the next act resolves it (local 03a1f195, 146b460f).
  const settle={actor:game.actor,input:{keys:[],duration_ms:600}};
  async function defeat(id){
    const before=(await game.inspect()).diagnostic.character.kills;let hp=0,stalled=0;
    for(let n=0;n<100;n++){
      const state=await game.inspect(),e=state.diagnostic.entities.find(e=>e.id===id);
      if(!e){assert(state.diagnostic.character.kills>before,'Enemy left local inspection without observed kill credit: '+id);return;}
      // A robot in the next room can stand within range behind a wall: close in once three volleys land nothing.
      stalled=e.hp<hp?0:stalled+1;hp=e.hp;
      // Dr Terrible's Phase2 bubble is Invulnerable (actors.xml:5096).
      if(e.condition&((1<<23)|(1<<24))){await act({keys:[],duration_ms:2000});continue;}
      const v=await game.observe();
      if(v.character.conditions&(1<<27)){
        const blue=(await entities()).filter(p=>p.kind===0x1e16&&distance(p.pos,v.position)<15).sort((a,b)=>distance(a.pos,v.position)-distance(b.pos,v.position))[0];
        assert(blue,'No reachable local blue pool for the transformed fighter');
        await walk(blue.pos,.6);continue;
      }
      const target={entity:id,camera_angle:-Math.PI/4,fire:true};
      const blocked=e.kind!==0x976&&stalled>3;
      if(blocked||distance(e.pos,v.position)>(e.kind===0x976?7:6.5)){stalled=0;await walk(e.pos,blocked?2:4.5);}
      // Fresh births can be server-visible before the client neighbourhood publishes them.
      else if(!state.view.objects.some(o=>o.id===id&&o.hp>0))await act({keys:[],duration_ms:300});
      else if(e.kind===0x976){
        await prepare({kind:'vitals',hp:v.character.stats.max_hp,mp:v.character.stats.max_mp});
        if(e.hp>30000&&protectedUntil-v.game_ms>10000)await act(Array.from({length:3},()=>[
          {actor:game.actor,input:{keys:[' '],duration_ms:500,target}},
          {actor:game.actor,input:{keys:[],duration_ms:800,target}},
        ]).flat());
        else await act([{actor:game.actor,input:{keys:[' '],duration_ms:1000,target}},settle]);
      } else await act([{actor:game.actor,input:{keys:[],duration_ms:2000,target}},settle]);
    }
    await save('failed-fight');throw Error('Native fight exceeded its bound: '+id);
  }
  const initial=await save('nexus');
  await game.act({keys:['1'],duration_ms:500});
  await game.act({keys:[],mouse:{x:700,y:575},duration_ms:700});
  for(let n=0;n<8&&(await game.observe()).session.world===initial.view.session.world;n++)await game.act({keys:[],duration_ms:500});
  const entered=await save('key-entry'),[sx,sy]=(await api({op:'world_info',world:entered.view.session.world})).spawn_points[0];
  game.check('ordinary Lab Key opens and admits the same living character at the entrance',entered.view.map.name==='Mad Lab'&&entered.view.session.character===initial.view.session.character&&entered.view.character.items[4]===-1&&distance(entered.view.position,{x:sx,y:sy})<1);
  game.check('entrance retains the selected Portal of Cowardice',(await entities()).some(e=>e.kind===0x703&&distance(e.pos,entered.view.position)<3));
  // The seeded room tree (src/dungeon/lab.rs) varies per run: follow the generated route to Dr Terrible, clearing robots met on the way.
  const boss=(await scene()).entities.find(e=>e.kind===0x976);assert(boss,'Generated Lab has no Dr Terrible');
  const path=route(await scene(),entered.view.position,boss.pos,{reach:9}).points;let rooms=0;
  for(let i=1;i<path.length;i++){
    if(distance(path[i],path[i-1])<6&&i<path.length-1)continue;
    await walk(path[i],1);
    for(let n=0;n<6;n++){
      const v=await game.observe(),robot=(await entities()).filter(e=>e.kind>=0x979&&e.kind<=0x983&&distance(e.pos,v.position)<9).sort((a,b)=>distance(a.pos,v.position)-distance(b.pos,v.position))[0];
      if(!robot)break;if(n===0)rooms++;
      if(rooms<=4&&n===0)await act({keys:[],duration_ms:1000},{name:'lab-room-'+rooms,when:()=>true});
      await defeat(robot.id);
    }
  }
  const traversed=await save('ordinary-rooms');
  game.check('native movement and weapon fire traverse four populated rooms and earn ordinary kills',rooms>=4&&traversed.diagnostic.character.kills>=entered.diagnostic.character.kills+4&&traversed.view.character.experience>entered.view.character.experience);
  game.check('native traversal observes real robot and both acid turret projectiles',[0x990,0x991].every(kind=>[...shots.values()].some(p=>p.kind===kind))&&[...shots.values()].some(p=>p.kind>=0x979&&p.kind<=0x983));
  recordShots=false;
  await defeat(boss.id);
  await act({keys:[],duration_ms:1000},{name:'lab-boss-defeated',when:()=>true});
  const cleared=await save('boss-defeated'),map=await scene();await writeFile(output('lab-cleared-map.json'),JSON.stringify(map));
  game.check('native shots defeat Dr Terrible and commit one Mad Lab clear',!map.entities.some(e=>e.id===boss.id)&&cleared.diagnostic.character.mad_labs===entered.diagnostic.character.mad_labs+1);
  // Mad Lab Open Wall (actors.xml:5288) deletes Emergency Walls only once Dr Terrible and every Tesla Coil are gone.
  game.check('boss defeat without coil clearance keeps Creation sealed',map.entities.some(e=>e.kind===0x18c2)&&map.entities.some(e=>e.kind===0x188a));
  const bag=cleared.diagnostic.entities.find(e=>e.items?.includes(0xa35));assert(bag,'No earned Wisdom reward');
  await walk(bag.pos,.6);await act({keys:[],duration_ms:900});
  const slot=bag.items.indexOf(0xa35),before=await game.observe();
  await act({keys:[],duration_ms:350,drag:{from:{x:634+44*(slot%4),y:528+44*Math.floor(slot/4)},to:{x:634,y:428}}});
  const picked=await save('reward-picked');
  game.check('native drag commits the earned Wisdom potion once',picked.view.character.items[4]===0xa35&&picked.view.character.inventory_revision===before.character.inventory_revision+1);
  await act({keys:[],duration_ms:350,drag:{from:{x:634+44*(slot%4),y:528+44*Math.floor(slot/4)},to:{x:678,y:428}}});
  game.check('dragging the emptied reward slot cannot duplicate the potion',(await game.observe()).character.inventory_revision===picked.view.character.inventory_revision);
  // Recovered Dr Terrible (actors.xml:5059) publishes no exit, so the run leaves through the entrance Portal of Cowardice.
  const cowardice=(await scene()).entities.find(e=>e.kind===0x703);assert(cowardice,'Entrance Portal of Cowardice missing');
  // On the return leg only, use the corner-recovery step and clear blocked approaches after proving the wall stayed sealed.
  await walk(cowardice.pos,.6,true);await act({keys:[],duration_ms:900});
  await act({keys:[],mouse:{x:700,y:575},duration_ms:1700});
  const returned=await save('returned');
  game.check('Portal of Cowardice returns the same living character with its reward and one clear',returned.view.map.name==='Nexus'&&returned.view.session.character===initial.view.session.character&&returned.view.lifecycle.alive&&returned.view.character.items[4]===0xa35&&returned.diagnostic.character.mad_labs===cleared.diagnostic.character.mad_labs);
  await act({keys:[],duration_ms:1000},{name:'lab-returned',when:()=>true});
}
