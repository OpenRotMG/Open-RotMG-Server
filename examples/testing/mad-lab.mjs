import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';

export const fixture={class:0x307,level:20,seed:12351,world:-2,position:'spawn',equipment:[0xc02,0xade,0xaf9,0xacd],backpack:[0xc2f,-1,-1,-1,-1,-1,-1,-1],stats:[700,252,75,25,50,50,40,50]};
export async function run(game){
  const output=name=>join(process.env.QA_RESULTS_DIR,name),samples=[],shots=new Map();
  const report=JSON.parse(await readFile(output('native/result.json'),'utf8'));
  const token=(await readFile(output('native/state/control.token'),'utf8')).trim();
  let protectedUntil=0,armoredUntil=0;
  const distance=(a,b)=>Math.hypot(a.x-b.x,a.y-b.y);
  const entities=async()=>(await game.inspect()).diagnostic.entities;
  async function save(name){const state=await game.inspect();samples.push({name,state});await writeFile(output('lab-journey.json'),JSON.stringify(samples));return state;}
  async function prepare(change){
    const response=await fetch(report.origin+'/dev/testing',{method:'POST',signal:AbortSignal.timeout(10000),headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:game.actor,change}})});
    const result=await response.json();assert(response.ok&&result.status==='succeeded',JSON.stringify(result));
  }
  async function act(input,capture){
    const now=(await game.observe()).game_ms;
    if(now>=protectedUntil){
      await prepare({kind:'protection',condition:'invincible',duration_ms:60000});protectedUntil=now+60000;
    }
    if(protectedUntil-now<=5000&&now>=armoredUntil){
      await prepare({kind:'stat_modifier',index:3,amount:100,duration_ms:60000});armoredUntil=now+60000;
    }
    let pending=null,error=null;
    const timer=setInterval(()=>{if(!pending)pending=game.inspect().then(s=>{
      for(const p of s.view.projectiles){
        const e=s.diagnostic.entities.find(e=>e.id===p.owner);
        if(e&&e.kind>=0x979&&e.kind<=0x991)shots.set(p.owner+':'+p.id,{kind:e.kind,...p});
      }
    }).catch(e=>{error=e;}).finally(()=>{pending=null;});},100);
    try{return await game.act(input,capture);}
    finally{clearInterval(timer);if(pending)await pending;if(error)throw error;await writeFile(output('lab-projectiles.json'),JSON.stringify([...shots.values()]));}
  }
  async function walk(x,y){
    for(let n=0;n<60;n++){
      const v=await game.observe(),dx=x-v.position.x,dy=y-v.position.y;
      if(Math.hypot(dx,dy)<.6)return;
      let keys=Math.abs(dx)>Math.abs(dy)?(dx>0?['d','s']:['a','w']):(dy>0?['a','s']:['d','w']);
      if(v.character.conditions&(1<<10))keys=keys.map(k=>({d:'w',a:'s',s:'a',w:'d'})[k]);
      const speed=v.character.conditions&(1<<3)?4:(4+5.6*v.character.stats.speed/75)*(v.character.conditions&(1<<14)?1.5:1);
      await act({keys,duration_ms:Math.max(100,Math.min(400,Math.round(Math.max(Math.abs(dx),Math.abs(dy))/speed*1000)))});
    }
    await save('failed-walk');throw Error('Native walk did not reach '+x+','+y);
  }
  async function defeat(id){
    for(let n=0;n<65;n++){
      const e=(await entities()).find(e=>e.id===id);if(!e)return;
      if(e.condition&(1<<23)){await act({keys:[],duration_ms:2000});continue;}
      const v=await game.observe();
      if(v.character.conditions&(1<<27)){
        const blue=(await entities()).filter(p=>p.kind===0x1e16&&distance(p.pos,v.position)<15).sort((a,b)=>distance(a.pos,v.position)-distance(b.pos,v.position))[0];
        assert(blue,'No reachable local blue pool for the transformed fighter');
        await walk(blue.pos.x,blue.pos.y);continue;
      }
      if(distance(e.pos,v.position)>6.5)await walk(e.pos.x-1,e.pos.y+1);
      else await act({keys:[],duration_ms:2000,target:{entity:id,camera_angle:-Math.PI/4,fire:true}});
    }
    await save('failed-fight');throw Error('Native fight exceeded its bound: '+id);
  }
  const initial=await save('nexus');
  await game.act({keys:['1'],duration_ms:500});
  await game.act({keys:[],mouse:{x:700,y:575},duration_ms:700});
  for(let n=0;n<8&&(await game.observe()).session.world===initial.view.session.world;n++)await game.act({keys:[],duration_ms:500});
  const entered=await save('key-entry');
  game.check('ordinary Lab Key opens and admits the same living character at the entrance',entered.view.map.name==='Mad Lab'&&entered.view.session.character===initial.view.session.character&&entered.view.character.items[4]===-1&&distance(entered.view.position,{x:12.5,y:66.5})<1);
  game.check('entrance retains the selected Portal of Cowardice',(await entities()).some(e=>e.kind===0x703&&distance(e.pos,entered.view.position)<3));
  const robots=entered.diagnostic.entities.filter(e=>e.kind>=0x979&&e.kind<=0x983);
  game.check('natural middle rooms contain sixteen selected robots and four coils',robots.length===16&&entered.diagnostic.entities.filter(e=>e.kind===0x188a).length===4);
  for(const [x,y] of [[12.5,48.5],[32.5,48.5],[32.5,30.5],[56.5,30.5]]){
    await walk(x,y);
    const room=(await entities()).filter(e=>e.kind>=0x979&&e.kind<=0x983&&distance(e.pos,{x,y})<12);
    assert(room.length,'No natural robots encountered in room '+x+','+y);
    await act({keys:[],duration_ms:1000},{name:'lab-room-'+x+'-'+y,when:()=>true});
    for(const e of room)await defeat(e.id);
    await walk(x,y);
  }
  const traversed=await save('ordinary-rooms');
  game.check('native movement and weapon fire traverse populated rooms and defeat ordinary robots',traversed.diagnostic.character.kills>entered.diagnostic.character.kills&&traversed.view.character.experience>entered.view.character.experience&&!traversed.diagnostic.entities.some(e=>robots.some(r=>r.id===e.id)));
  game.check('ordinary robot deaths do not fabricate equipment bags',!traversed.diagnostic.entities.some(e=>e.items?.some(i=>i>=0)));
  game.check('acid-room turrets survive ordinary combat',traversed.diagnostic.entities.filter(e=>e.kind===0x990||e.kind===0x991).length===2);
  game.check('native traversal observes real robot and both acid turret projectiles',[0x990,0x991].every(kind=>[...shots.values()].some(p=>p.kind===kind))&&[...shots.values()].some(p=>p.kind>=0x979&&p.kind<=0x983));
  await walk(56.5,23.5);await walk(57.5,20.5);await walk(60.5,20.5);await walk(62.5,12.5);
  const boss=(await entities()).find(e=>e.kind===0x976);assert(boss);
  await defeat(boss.id);
  await act({keys:[],duration_ms:1000},{name:'lab-boss-defeated',when:()=>true});
  const cleared=await save('boss-defeated');
  game.check('native shots defeat Dr Terrible and commit one Mad Lab clear',!cleared.diagnostic.entities.some(e=>e.id===boss.id)&&cleared.diagnostic.character.mad_labs===entered.diagnostic.character.mad_labs+1);
  game.check('boss defeat without coil clearance keeps Creation sealed',cleared.diagnostic.entities.some(e=>e.kind===0x18c2)&&cleared.diagnostic.entities.find(e=>e.kind===0x5e1c)?.stage===null);
  const bag=cleared.diagnostic.entities.find(e=>e.items?.includes(0xa35));assert(bag,'No earned Wisdom reward');
  const exit=cleared.diagnostic.entities.find(e=>e.kind===0x71d);assert(exit,'No committed boss exit');
  await walk(bag.pos.x,bag.pos.y);await act({keys:[],duration_ms:900});
  const slot=bag.items.indexOf(0xa35),before=await game.observe();
  await act({keys:[],duration_ms:350,drag:{from:{x:634+44*(slot%4),y:528+44*Math.floor(slot/4)},to:{x:634,y:428}}});
  const picked=await save('reward-picked');
  game.check('native drag commits the earned Wisdom potion once',picked.view.character.items[4]===0xa35&&picked.view.character.inventory_revision===before.character.inventory_revision+1);
  await walk(exit.pos.x,exit.pos.y);await act({keys:[],duration_ms:900});
  await act({keys:[],mouse:{x:700,y:575},duration_ms:1700});
  const returned=await save('returned');
  game.check('usable boss exit returns the same living character with its reward and one clear',returned.view.map.name==='Nexus'&&returned.view.session.character===initial.view.session.character&&returned.view.lifecycle.alive&&returned.view.character.items[4]===0xa35&&returned.diagnostic.character.mad_labs===cleared.diagnostic.character.mad_labs);
  await act({keys:[],duration_ms:1000},{name:'lab-returned',when:()=>true});
}
