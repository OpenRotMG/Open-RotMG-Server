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
    const victim=(await entities()).find(e=>e.kind===kind);
    if(!victim)throw Error('Missing native fight target: '+name);
    for(let n=0;n<80;n++) {
      const e=(await entities()).find(e=>e.id===victim.id);
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
  const original=await creation();
  const beforeFinal=await game.inspect(),trace=[];
  for(let n=0;n<70;n++) {
    const c=await creation();if(!c)break;
    const now=(await game.observe()).game_ms,remaining=protections.get(game.actor).until-now;
    trace.push({game_ms:now,id:c.id,hp:c.hp,stage:c.stage});
    await writeFile(join(process.env.QA_RESULTS_DIR,'creation-completion.json'),JSON.stringify(trace));
    if(c.stage===2) {
      if((await game.observe()).character.conditions&(1<<27))await go(75.5,12.5);
      const view=await game.observe();
      await prepare(game.actor,{kind:'vitals',hp:view.character.stats.max_hp,mp:view.character.stats.max_mp});
      const input={keys:[' '],duration_ms:1000,target:{entity:c.id,camera_angle:camera,fire:true}};
      if(c.hp>15000&&remaining>5000)await act(Array.from({length:3},()=>({actor:game.actor,input})));
      else await act(input);
    } else if((await game.observe()).position.x>72)await go(70.5,12.5);
    else if(c.stage===4&&remaining>7000) {
      await act(Array.from({length:3},()=>({actor:game.actor,input:{keys:[],duration_ms:2000}})));
    } else await wait();
  }
  const final=await game.inspect();
  game.check('native weapon fire defeats the original Creation and earns kill and XP credit',!final.diagnostic.entities.some(e=>e.id===original.id)&&final.diagnostic.character.kills>beforeFinal.diagnostic.character.kills&&final.view.character.experience>beforeFinal.view.character.experience);
  game.check('optional defeat does not duplicate Mad Lab clear credit',final.diagnostic.character.mad_labs===beforeFinal.diagnostic.character.mad_labs);
  const bags=final.diagnostic.entities.filter(e=>Math.hypot(e.pos.x-original.pos.x,e.pos.y-original.pos.y)<10&&e.items?.some(i=>i>=0));
  game.check('Horrific Creation guarantees one defense potion and no other equipment',bags.length===1&&bags[0].items.filter(i=>i>=0).every(i=>i===0xa20));
  await act({keys:[],duration_ms:1000},{name:'creation-defeated',when:()=>true});
  await save('creation-defeated');
  await go(64.5,12.5);
  const exit=(await entities()).find(e=>e.kind===0x71d);
  if(!exit)throw Error('Previously committed Dr Terrible exit is missing');
  await go(exit.pos.x,exit.pos.y);await act({keys:[],duration_ms:900});
  await act({keys:[],mouse:{x:700,y:575},duration_ms:1700});
  const returned=await game.inspect();
  game.check('same living character can use the retained first-boss exit after Creation',returned.view.map.name==='Nexus'&&returned.view.session.character===beforeFinal.view.session.character&&returned.view.lifecycle.alive&&returned.diagnostic.character.mad_labs===beforeFinal.diagnostic.character.mad_labs);
  await act({keys:[],duration_ms:1000},{name:'creation-returned',when:()=>true});
  await save('creation-returned');
}
