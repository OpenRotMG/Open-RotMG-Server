import {writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
export const fixture = {
  class:0x30e,level:20,seed:12351,world:'mad-lab',position:{x:12.5,y:48.5},
  equipment:[0xb08,0xa2e,-1,-1],backpack:Array(8).fill(-1),
  stats:[670,385,75,25,50,75,40,60],
};
export async function run(game) {
  let expires=0,serial=0,camera=0;
  async function act(input,capture) {
    const now=(await game.observe()).game_ms;
    if(now>=expires) {
      const request=join(process.env.QA_RESULTS_DIR,'protection-request-'+serial+'.json');
      await writeFile(request,JSON.stringify({id:'explicit-native-protection-'+serial++,command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invincible',duration_ms:60000}}}));
      execFileSync(process.execPath,[resolve('scripts/game.mjs'),join(process.env.QA_RESULTS_DIR,'native/native.json'),request]);
      expires=now+60000;
    }
    return game.act(input,capture);
  }
  const save=async name=>writeFile(join(process.env.QA_RESULTS_DIR,name+'.json'),JSON.stringify(await game.inspect(),null,2));
  const entities=async()=> (await game.inspect()).diagnostic.entities;
  async function go(x,y) {
    for(let n=0;n<80;n++) {
      const p=(await game.observe()).position,dx=x-p.x,dy=y-p.y;
      if(Math.hypot(dx,dy)<0.4)return;
      const sx=dx*Math.cos(camera)+dy*Math.sin(camera),sy=-dx*Math.sin(camera)+dy*Math.cos(camera);
      const keys=[];
      if(Math.abs(sx)>0.15)keys.push(sx>0?'d':'a');
      if(Math.abs(sy)>0.15)keys.push(sy>0?'s':'w');
      const distance=keys.length===2?Math.min(Math.abs(sx),Math.abs(sy))*Math.SQRT2:Math.max(Math.abs(sx),Math.abs(sy));
      await act({keys,duration_ms:Math.max(50,Math.min(400,Math.round(distance/7.733*1000)))});
    }
    throw Error('Native walk did not reach '+x+','+y);
  }
  async function defeat(kind,name) {
    for(let n=0;n<300;n++) {
      const e=(await entities()).find(e=>e.kind===kind);
      if(!e){game.check(name+' defeated through native shots',true);return;}
      const p=(await game.observe()).position;
      if(Math.hypot(e.pos.x-p.x,e.pos.y-p.y)>7)await go(e.pos.x-2,e.pos.y+2);
      await act({keys:[],duration_ms:500,target:{entity:e.id,camera_angle:camera,fire:true}});
    }
    throw Error(name+' survived bounded native fight');
  }
  const start=(await game.observe()).position;
  await act({keys:['w'],duration_ms:200},{name:'native-lab-before',when:()=>true});
  const moved=(await game.observe()).position;
  game.check('native movement supplies a nonzero camera calibration',Math.hypot(moved.x-start.x,moved.y-start.y)>0.5);
  camera=Math.atan2(moved.x-start.x,start.y-moved.y);
  await writeFile(join(process.env.QA_RESULTS_DIR,'camera.json'),JSON.stringify({start,moved,camera}));
  for(const [x,y] of [[12.5,48.5],[32.5,48.5],[32.5,30.5],[56.5,30.5]]) {
    await go(x,y);
    if(x===56.5)await go(60.5,y);
    await defeat(0x188a,'Tesla Coil');
    await go(x,y);
  }
  await go(56.5,22.5);
  await go(60.5,22.5);
  await go(60.5,20.5);
  await go(64.5,12.5);
  await save('all-coils-before-boss');
  game.check('coils alone leave emergency walls sealed',(await entities()).some(e=>e.kind===0x18c2));
  await defeat(0x976,'Dr Terrible');
  await act({keys:[],duration_ms:500});
  game.check('real coil and boss kills open emergency walls',!(await entities()).some(e=>e.kind===0x18c2));
  await go(60.5,12.5);
  for(let n=0;n<200&&(await entities()).find(e=>e.kind===0x5e1c).stage!==4;n++)await act({keys:[],duration_ms:100});
  game.check('ordinary native distance triggers the center blast phase',(await entities()).find(e=>e.kind===0x5e1c).stage===4);
  await go(64.5,11.5);
  await go(69.5,11.5);
  await go(75.5,12.5);
  await save('creation-open');
  const creation=async()=> (await entities()).find(e=>e.kind===0x5e1c);
  const original=await creation(),seen=new Set(),shots=new Set(),captured=new Set(),trace=[];
  for(let n=0;n<1000;n++) {
    const state=await game.inspect(),c=state.diagnostic.entities.find(e=>e.id===original.id);
    seen.add(c.stage);trace.push({time:state.view.time_ms,stage:c.stage,pos:c.pos,hp:c.hp});
    for(const p of state.view.projectiles.filter(p=>p.owner===c.id))shots.add(p.speed+':'+p.lifetime_ms);
    if([4,6,7].includes(c.stage)&&!captured.has(c.stage)) {
      await act({keys:[],duration_ms:500},{name:'creation-phase-'+c.stage,when:()=>true});
      captured.add(c.stage);
    }
    if([4,6,7].every(s=>seen.has(s))&&['85:1000','50:400','55:3000'].every(s=>shots.has(s)))break;
    await act({keys:[],duration_ms:100});
  }
  await writeFile(join(process.env.QA_RESULTS_DIR,'attack-cycle.json'),JSON.stringify({trace,seen:[...seen],shots:[...shots]}));
  game.check('native client observes blast, shockwave and energy projectiles',['85:1000','50:400','55:3000'].every(s=>shots.has(s)));
  game.check('native encounter enters blast, shockwave and energy phases',[4,6,7].every(s=>seen.has(s)));
  await go(70.5,12.5);
  for(let n=0;n<500&&(await creation()).stage!==2;n++)await act({keys:[],duration_ms:100});
  const pet=await creation();
  game.check('native Hexxer lure creates a vulnerable pet',pet.stage===2&&(pet.condition&(1<<23))===0);
  game.check('native player is Hexed in the green pool',((await game.observe()).character.conditions&(1<<27))!==0);
  await act({keys:[],duration_ms:500},{name:'creation-pet',when:()=>true});
  await go(75.5,12.5);
  game.check('native blue pool removes player Hexed',((await game.observe()).character.conditions&(1<<27))===0);
  await act({keys:[],duration_ms:500,target:{entity:pet.id,camera_angle:camera,fire:true}});
  game.check('ordinary native shots damage the same pet',(await creation()).hp<pet.hp);
  for(let n=0;n<250&&(await creation()).stage===2;n++)await act({keys:[],duration_ms:100});
  const restored=await creation();
  game.check('DeHexxer restores invincibility without replacing the boss',restored.id===original.id&&restored.stage!==2&&(restored.condition&(1<<23))!==0);
  await save('creation-restored');
  game.check('native player survives the encounter',(await game.observe()).lifecycle.alive);
}
