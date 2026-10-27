import {writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
export const fixture = {
  class:0x307,level:20,seed:12351,world:'mad-lab',position:{x:12.5,y:48.5},
  equipment:[0xc02,0xade,0xaf9,0xacd],backpack:Array(8).fill(-1),
  stats:[700,252,75,25,50,50,40,50],
};
export async function run(game) {
  let serial=0,camera=0;
  const protections=new Map([[game.actor,{until:0,armorUntil:0}]]);
  async function prepare(actor,change) {
    const request=join(process.env.QA_RESULTS_DIR,'preparation-'+serial+'.json');
    await writeFile(request,JSON.stringify({id:'explicit-native-preparation-'+serial++,command:{op:'cheat',actor,change}}));
    execFileSync(process.execPath,[resolve('scripts/game.mjs'),join(process.env.QA_RESULTS_DIR,'native/native.json'),request]);
  }
  async function act(input,capture) {
    const now=(await game.observe()).game_ms;
    for(const [actor,window] of protections) {
      if(now>=window.until) {
        await prepare(actor,{kind:'protection',condition:'invincible',duration_ms:60000});
        window.until=now+60000;
      }
      // Protection cannot be renewed early; stagger DEF across its expiry.
      if(window.until-now<=15000&&now>=window.armorUntil) {
        await prepare(actor,{kind:'stat_modifier',index:3,amount:100,duration_ms:60000});
        window.armorUntil=now+60000;
      }
    }
    return game.act(input,capture);
  }
  const save=async name=>writeFile(join(process.env.QA_RESULTS_DIR,name+'.json'),JSON.stringify(await game.inspect(),null,2));
  const entities=async()=> (await game.inspect()).diagnostic.entities;
  const wait=async()=>act({keys:[],duration_ms:protections.get(game.actor).until-(await game.observe()).game_ms>5000?2000:100});
  const alignEast=async()=>act({keys:(await game.observe()).character.conditions&(1<<10)?['a','w']:['d','s'],duration_ms:400});
  async function go(x,y) {
    for(let n=0;n<80;n++) {
      const view=await game.observe(),p=view.position,dx=x-p.x,dy=y-p.y;
      if(Math.hypot(dx,dy)<0.4)return;
      const sx=dx*Math.cos(camera)+dy*Math.sin(camera),sy=-dx*Math.sin(camera)+dy*Math.cos(camera),keys=[];
      if(Math.abs(sx)>0.15&&Math.abs(sx)>=Math.abs(sy)*0.41421356)keys.push(sx>0?'d':'a');
      if(Math.abs(sy)>0.15&&Math.abs(sy)>=Math.abs(sx)*0.41421356)keys.push(sy>0?'s':'w');
      const distance=keys.length===2?Math.min(Math.abs(sx),Math.abs(sy))*Math.SQRT2:Math.max(Math.abs(sx),Math.abs(sy));
      const flags=view.character.conditions,speed=flags&(1<<3)?4:(4+5.6*view.character.stats.speed/75)*(flags&(1<<14)?1.5:1);
      const inputKeys=flags&(1<<10)?keys.map(k=>({d:'w',a:'s',s:'a',w:'d'})[k]):keys;
      await act({keys:inputKeys,duration_ms:Math.max(50,Math.min(400,Math.round(distance/speed*1000)))});
    }
    await save('failed-walk-'+x+'-'+y);
    throw Error('Native walk did not reach '+x+','+y);
  }
  async function defeat(kind,name) {
    for(let n=0;n<80;n++) {
      const e=(await entities()).find(e=>e.kind===kind);
      if(!e){game.check(name+' defeated through native shots',true);return;}
      const view=await game.observe();
      if(Math.hypot(e.pos.x-view.position.x,e.pos.y-view.position.y)>7)await go(e.pos.x-2,e.pos.y+2);
      const target={entity:e.id,camera_angle:camera,fire:true};
      if(kind===0x976) {
        if(e.condition&(1<<23)){await wait();continue;}
        const current=await game.observe();
        await prepare(game.actor,{kind:'vitals',hp:current.character.stats.max_hp,mp:current.character.stats.max_mp});
        if(e.hp>30000&&protections.get(game.actor).until-current.game_ms>10000) {
          await act(Array.from({length:3},()=>[
            {actor:game.actor,input:{keys:[' '],duration_ms:500,target}},
            {actor:game.actor,input:{keys:[],duration_ms:800,target}},
          ]).flat());
        } else await act({keys:[' '],duration_ms:1000,target});
      } else await act({keys:[],duration_ms:500,target});
    }
    await save('failed-fight-'+kind);
    throw Error(name+' survived bounded native fight');
  }
  const start=(await game.observe()).position;
  await act({keys:['w'],duration_ms:200},{name:'native-lab-before',when:()=>true});
  const moved=(await game.observe()).position;
  game.check('native movement supplies a nonzero camera calibration',Math.hypot(moved.x-start.x,moved.y-start.y)>0.5);
  camera=Math.atan2(moved.x-start.x,start.y-moved.y);
  game.check('selected client uses its default camera',Math.abs(camera+Math.PI/4)<0.0001);
  await writeFile(join(process.env.QA_RESULTS_DIR,'camera.json'),JSON.stringify({start,moved,camera}));
  for(const [x,y] of [[12.5,48.5],[32.5,48.5],[32.5,30.5],[56.5,30.5]]) {
    await go(x,y);
    if(x===56.5)await go(60.5,y);
    await defeat(0x188a,'Tesla Coil');
    await go(x,y);
  }
  await go(56.5,23.5);
  await alignEast();
  await go(57.5,20.5);
  await go(60.5,20.5);
  await go(64.0,12.5);
  await save('all-coils-before-boss');
  game.check('coils alone leave emergency walls sealed',(await entities()).some(e=>e.kind===0x18c2));
  await defeat(0x976,'Dr Terrible');
  await go(60.5,12.5);
  await act({keys:[],duration_ms:500});
  game.check('real coil and boss kills open emergency walls',!(await entities()).some(e=>e.kind===0x18c2));
  for(let n=0;n<20&&(await entities()).find(e=>e.kind===0x5e1c).stage!==4;n++)await wait();
  game.check('ordinary native distance triggers the center blast phase',(await entities()).find(e=>e.kind===0x5e1c).stage===4);
  await go(64.5,10.5);
  await act([
    {actor:game.actor,input:{keys:['d','s'],duration_ms:600}},
    {actor:game.actor,input:{keys:['a','s'],duration_ms:300}},
    {actor:game.actor,input:{keys:['d','s'],duration_ms:1200}},
  ]);
  await go(75.5,12.5);
  game.check('native movement crosses the opened emergency doorway',(await game.observe()).position.x>69);
  await save('creation-open');
  const creation=async()=> (await entities()).find(e=>e.kind===0x5e1c);
  const original=await creation(),seen=new Set(),shots=new Set(),trace=[];
  let captured=false,live=null,blastStart=null;
  function record(state) {
    const c=state.diagnostic.entities.find(e=>e.id===original.id);
    if(!c)throw Error('Creation disappeared during attack observation');
    if(c.stage===4&&live!==null&&live.stage!==4)blastStart=state.view.time_ms;
    live={time:state.view.time_ms,stage:c.stage,blast:state.view.projectiles.some(p=>p.owner===c.id&&p.speed===85&&p.lifetime_ms===1000)};
    seen.add(c.stage);trace.push({time:state.view.time_ms,stage:c.stage,pos:c.pos,hp:c.hp});
    for(const p of state.view.projectiles.filter(p=>p.owner===c.id))shots.add(p.speed+':'+p.lifetime_ms);
  }
  let sample=null,sampleError=null;
  const timer=setInterval(()=>{if(!sample)sample=game.inspect().then(record).catch(error=>{sampleError=error;}).finally(()=>{sample=null;});},50);
  try {
    for(let n=0;n<40;n++) {
      const state=await game.inspect(),c=state.diagnostic.entities.find(e=>e.id===original.id);
      record(state);
      if(c.stage===4&&!captured&&blastStart!==null&&state.view.time_ms-blastStart<8000) {
        await act({keys:[],duration_ms:2000},{name:'creation-blast',when:view=>live!==null&&live.stage===4&&live.blast&&Math.abs(view.game_ms-live.time)<=100});
        captured=true;
      }
      if(captured&&[4,6,7].every(s=>seen.has(s))&&['85:1000','50:400','55:3000'].every(s=>shots.has(s)))break;
      await wait();
    }
  } finally {clearInterval(timer);if(sample)await sample;}
  if(sampleError)throw sampleError;
  await writeFile(join(process.env.QA_RESULTS_DIR,'attack-cycle.json'),JSON.stringify({trace,seen:[...seen],shots:[...shots]}));
  game.check('native session observes blast, shockwave and energy projectiles',['85:1000','50:400','55:3000'].every(s=>shots.has(s)));
  game.check('native encounter enters blast, shockwave and energy phases',[4,6,7].every(s=>seen.has(s)));
  const partner=await game.join({...fixture,world:(await game.observe()).session.world,position:{x:70.5,y:13.5},equipment:[-1,-1,-1,-1]});
  protections.set(partner.actor,{until:0,armorUntil:0});
  await act({keys:[],duration_ms:100});
  game.check('second native client shares the secret room',(await partner.observe()).session.world===(await game.observe()).session.world);
  await go(70.5,12.5);
  for(let n=0;n<30&&(await creation()).stage!==2;n++)await wait();
  const pet=await creation();
  game.check('native Hexxer lure creates a vulnerable pet',pet.stage===2&&(pet.condition&(1<<23))===0);
  game.check('native player is Hexed in the green pool',((await game.observe()).character.conditions&(1<<27))!==0);
  const petTime=(await game.observe()).game_ms;
  await act([
    {actor:game.actor,input:{keys:['d','s'],duration_ms:400}},
    ...Array.from({length:4},(_,n)=>({actor:game.actor,input:{keys:n===0?[' ']:[],duration_ms:500,target:{entity:pet.id,camera_angle:camera,fire:true}}})),
  ],{name:'creation-pet',when:view=>(view.character.conditions&(1<<27))===0&&view.game_ms>=petTime+1200});
  game.check("native blue pool removes only that player's Hexed",((await game.observe()).character.conditions&(1<<27))===0&&((await partner.observe()).character.conditions&(1<<27))!==0);
  game.check('native player shots damage the same pet',(await creation()).hp<pet.hp);
  for(let n=0;n<20&&(await creation()).stage===2;n++)await wait();
  const restored=await creation();
  game.check('DeHexxer restores invincibility without replacing the boss',restored.id===original.id&&restored.stage!==2&&(restored.condition&(1<<23))!==0);
  await save('creation-restored');
  game.check('both prepared native players remain alive',(await game.observe()).lifecycle.alive&&(await partner.observe()).lifecycle.alive);
}
