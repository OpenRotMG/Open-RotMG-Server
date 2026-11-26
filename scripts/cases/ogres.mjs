import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
import {route} from './native-route.mjs';
const spec={class:0x30e,level:20,seed:832,world:1,equipment:[0xa9f,-1,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,75,0,50,75,40,60]};
const root=()=>join(process.env.QA_RESULTS_DIR,'native');
let king;
export async function prepare(api){
  const world=await api({op:'world_info',world:1}),[x,y]=world.spawn_points[0];
  const scout=await api({op:'spawn',spec:{...spec,guid:'ogre-scout',position:{x,y},controller:'direct',profile:null}});
  const state=await api({op:'inspect',actor:scout.result.actor});
  const report=JSON.parse(await readFile(join(root(),'result.json'),'utf8'));
  const headers={Authorization:'Bearer '+(await readFile(join(root(),'state/control.token'),'utf8')).trim()};
  const response=await fetch(report.origin+'/dev/map?player='+state.diagnostic.player,{headers});assert(response.ok);
  const map=await response.json(),blocked=new Set(map.blocked.map(p=>p.join(',')));
  for(const e of map.entities.filter(e=>e.kind===0x645))for(let dx=-2;dx<=2;dx++)for(let dy=-2;dy<=2;dy++){
    const position={x:Math.floor(e.pos.x)+.5+dx,y:Math.floor(e.pos.y)+.5+dy};
    if(Math.hypot(position.x-e.pos.x,position.y-e.pos.y)<1.5)continue;
    if([-1,0,1].every(x=>[-1,0,1].every(y=>!blocked.has([Math.floor(position.x+x),Math.floor(position.y+y)].join(','))))){
      king=e;await writeFile(join(process.env.QA_RESULTS_DIR,'natural-camp.json'),JSON.stringify(map.entities.filter(m=>m.id===e.id||m.parent===e.id)));return {...spec,position};
    }
  }
  throw Error('No naturally allocated Ogre King camp with a clear approach');
}
export async function run(game){
  const samples=[],shots=new Map();let serial=0,protectedUntil=0;
  const save=async name=>{const s=await game.inspect();await writeFile(join(process.env.QA_RESULTS_DIR,name+'.json'),JSON.stringify(s));return s;};
  // XML Ogre King MakeMinions (actors.xml:1310-1312, startWithMinions false) own its escorts, not the natural camp.
  const family=s=>s.diagnostic.entities.filter(e=>e.id===king.id||e.parent===king.id),owned=(s,kind)=>family(s).filter(e=>e.parent===king.id&&e.kind===kind);
  async function protect(){
    const now=(await game.observe()).game_ms;if(now<protectedUntil)return;
    const path=join(process.env.QA_RESULTS_DIR,'protection-'+serial+'.json');
    await writeFile(path,JSON.stringify({id:'ogre-protection-'+serial++,command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}}));
    execFileSync(process.execPath,[resolve('scripts/game.mjs'),join(root(),'native.json'),path]);protectedUntil=now+60000;
  }
  async function step(input,capture){
    await game.act(input,capture);const s=await game.inspect();samples.push(s);for(const p of s.view.projectiles)shots.set(p.owner+':'+p.id,p);return s;
  }
  async function act(input,capture){
    await protect();while(protectedUntil-(await game.observe()).game_ms<input.duration_ms+500){await step({keys:[],duration_ms:100});await protect();}
    return step(input,capture);
  }
  const begin=await save('arrival'),caps=[[0x642,2,300],[0x643,4,270],[0x644,2,300]];
  // Recovered realm/high.xml:1185–1187: offspring belong to the XML spawner, not an ordinary population slot.
  game.check('ordinary root is placed before it owns any offspring',begin.diagnostic.entities.some(e=>e.id===king.id&&e.camp===null&&e.parent===null&&e.hp===500)&&!begin.diagnostic.entities.some(e=>e.parent===king.id));
  await protect();
  await act({keys:[],duration_ms:2500});
  await act({keys:[],duration_ms:800},{name:'ogres-arrival',when:()=>true});
  for(let n=0;n<48;n++)await act({keys:[],duration_ms:250});
  for(let n=0;n<30&&!caps.every(([k])=>owned(samples.at(-1),k).length);n++)await act({keys:[],duration_ms:1000});
  await act({keys:[],duration_ms:800},{name:'ogres-combat',when:()=>true});
  // This exercises today's reconstruction, not Kithio's different King cycle: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/logic/db/BehaviorDb.Highland.cs#L70-L162
  const seen=new Map();for(const s of samples)for(const e of family(s))if(!seen.has(e.id))seen.set(e.id,e);
  for(const [kind,speed,lifetime] of [[0x645,100,400],[0x642,67,1500],[0x643,100,400],[0x644,50,2200]]){
    const ids=new Set([...seen.values()].filter(e=>e.kind===kind).map(e=>e.id));
    game.check(kind+': activates and sends selected projectile to native client',samples.some(s=>family(s).some(e=>ids.has(e.id)&&e.activated!==null))&&[...shots.values()].some(p=>ids.has(p.owner)&&p.speed===speed&&p.lifetime_ms===lifetime));
    game.check(kind+': moves from where it first appeared',samples.some(s=>family(s).some(e=>ids.has(e.id)&&Math.hypot(e.pos.x-seen.get(e.id).pos.x,e.pos.y-seen.get(e.id).pos.y)>.2)));
  }
  game.check('XML children have selected HP',caps.every(([k,,hp])=>[...seen.values()].filter(e=>e.kind===k).every(e=>e.hp===hp)));
  game.check('living King adds each XML child kind one at a time within its 2/4/2 caps',samples.every((s,i)=>caps.every(([k,n])=>owned(s,k).length<=n&&(!i||owned(s,k).length<=owned(samples[i-1],k).length+1))));
  await writeFile(join(process.env.QA_RESULTS_DIR,'combat.json'),JSON.stringify({samples,shots:[...shots.values()]}));
  async function defeat(id){
    const xp=(await game.observe()).character.experience;
    for(let n=0;n<100;n++){
      const s=await game.inspect(),e=family(s).find(e=>e.id===id);if(!e)break;
      const p=s.view.position,goal=e.pos;
      const offsets=s.view.objects.filter(o=>o.blocks).map(o=>[Math.floor(o.position.x),Math.floor(o.position.y)]);
      const walls=new Set(offsets.map(o=>o.join(',')));
      const clear=Array.from({length:21},(_,i)=>i/20).every(t=>!walls.has([Math.floor(p.x+t*(goal.x-p.x)),Math.floor(p.y+t*(goal.y-p.y))].join(',')));
      if(clear&&Math.hypot(goal.x-p.x,goal.y-p.y)<=2.2&&s.view.objects.some(o=>o.id===id&&o.hp>0)){
        await act({keys:[],duration_ms:600,target:{entity:id,camera_angle:-Math.PI/4,fire:true}});continue;
      }
      let waypoint=goal;
      if(!clear){
        // After the King dies, a survivor can stop behind a tree instead of following the player.
        const ox=Math.floor(p.x)-16,oy=Math.floor(p.y)-16,local=q=>({x:q.x-ox,y:q.y-oy});
        const blocked=offsets.map(([x,y])=>[x-ox,y-oy]).filter(([x,y])=>x>=0&&y>=0&&x<33&&y<33);
        const {points}=route({width:33,height:33,blocked},local(p),local(goal),{reach:1.6});
        waypoint=points.map(q=>({x:q.x+ox,y:q.y+oy})).find(q=>Math.hypot(q.x-p.x,q.y-p.y)>.25);
        assert(waypoint,'Tree-obstructed Ogre requires a distinct native waypoint');
      }
      const dx=waypoint.x-p.x,dy=waypoint.y-p.y,x=dx-dy,y=dx+dy;
      await act({keys:[Math.abs(x)>=Math.abs(y)*.4142?(x>0?'d':'a'):null,Math.abs(y)>=Math.abs(x)*.4142?(y>0?'s':'w'):null].filter(Boolean),duration_ms:Math.min(150,Math.max(50,Math.round(125*Math.hypot(dx,dy))))});
    }
    const dead=await save('defeated-'+id);game.check(id+': ordinary native staff defeats actor and earns XP',!family(dead).some(e=>e.id===id)&&dead.view.character.experience>xp);return dead;
  }
  const dead=await defeat(king.id),survivors=family(dead).map(e=>e.id);
  game.check('ordinary followers survive King defeat',survivors.length>0);
  for(const kind of [0x642,0x643,0x644]){
    const s=await game.inspect(),victim=family(s).find(e=>e.kind===kind);
    if(victim)await defeat(victim.id);
    else game.check(kind+': staff collateral already defeated observed role',[...seen.values()].filter(e=>e.kind===kind).every(e=>!family(s).some(p=>p.id===e.id))&&s.view.character.experience>begin.view.character.experience);
  }
  const cleared=await save('rewards');await act({keys:[],duration_ms:10000});
  await act({keys:[],duration_ms:800},{name:'ogres-defeated',when:()=>true});const end=await save('end');
  game.check('King stays dead and cannot duplicate XP',!family(end).some(e=>e.kind===0x645)&&end.view.character.experience===cleared.view.character.experience);
  game.check('dead camp cannot replace defeated escorts',family(end).every(e=>family(cleared).some(p=>p.id===e.id)));
  game.check('same prepared living native Wizard completes encounter',end.view.lifecycle.alive&&end.view.session.character===begin.view.session.character);
}
