import assert from 'node:assert/strict';
import {writeFile} from 'node:fs/promises';
import {join} from 'node:path';

export const fixture={class:0x30e,level:20,seed:1308,world:'spider-den',worldSeed:1308,position:{x:61.5,y:5.5},equipment:[0xb08,0xb24,0xb05,0xac5],backpack:Array(8).fill(-1),stats:null};
export async function run(game){
  const samples=[];
  async function save(name){
    const state=await game.inspect();samples.push({name,state});
    await writeFile(join(process.env.QA_RESULTS_DIR,'spider-exit.json'),JSON.stringify(samples,null,2));return state;
  }
  async function walk(point,enemy){
    for(let step=0;step<60;step++){
      const s=await game.inspect(),dx=point.x-s.view.position.x,dy=point.y-s.view.position.y;
      if(Math.hypot(dx,dy)<.8)return;
      const keys=Math.abs(dx)>Math.abs(dy)?(dx>0?['d','s']:['a','w']):(dy>0?['a','s']:['d','w']);
      const target=s.view.objects.some(e=>e.id===enemy&&e.hp>0)?{entity:enemy,camera_angle:-Math.PI/4,fire:true}:null;
      await game.act({keys,duration_ms:Math.min(500,Math.max(100,Math.round(Math.max(Math.abs(dx),Math.abs(dy))*160))),...(target?{target}:{})});
    }
    throw Error('Could not walk to Arachna completion exit');
  }
  await game.act({keys:[],duration_ms:4000});
  const before=await save('prepared-arena');
  const boss=before.diagnostic.entities.find(e=>e.kind===0x936);assert(boss);
  await game.act({keys:[],duration_ms:700},{name:'arachna-before',when:()=>true});
  await walk({x:72.5,y:7.5},boss.id);
  for(let shot=0;shot<40;shot++){
    const s=await game.inspect(),enemy=s.diagnostic.entities.find(e=>e.id===boss.id);
    if(!enemy)break;
    const dx=s.view.position.x-enemy.pos.x,dy=s.view.position.y-enemy.pos.y,d=Math.hypot(dx,dy);
    if(d>6)await walk({x:enemy.pos.x+dx/d*5,y:enemy.pos.y+dy/d*5},boss.id);
    if(!(await game.inspect()).diagnostic.entities.some(e=>e.id===boss.id))break;
    await game.act({keys:enemy.stage===2&&s.view.character.mp>=80?[' ']:[],duration_ms:900,target:{entity:boss.id,camera_angle:-Math.PI/4,fire:true}});
  }
  const cleared=await save('defeated');
  game.check('ordinary unprotected Wizard attacks defeat Arachna and commit one clear',!cleared.diagnostic.entities.some(e=>e.id===boss.id)&&cleared.diagnostic.character.spider_dens===before.diagnostic.character.spider_dens+1);
  const portals=cleared.diagnostic.entities.filter(e=>e.kind===0x704||e.kind===0x71d);
  assert.equal(portals.length,1,'Expected exactly one completion exit');
  await walk(portals[0].pos);
  await game.act({keys:[],duration_ms:700},{name:'arachna-exit',when:()=>true});
  await game.act({keys:[],mouse:{x:700,y:575},duration_ms:700});
  for(let step=0;step<8&&(await game.observe()).session.world===before.view.session.world;step++)await game.act({keys:[],duration_ms:500});
  const returned=await save('returned');
  await game.act({keys:[],duration_ms:700},{name:'arachna-return',when:()=>true});
  game.check('prepared dungeon completion uses a Realm Portal and reaches a Realm',portals[0].kind===0x704&&returned.view.map.name==='Realm');
  game.check('exit preserves the same living character, inventory and earned clear',returned.view.lifecycle.alive&&returned.view.session.account===before.view.session.account&&returned.view.session.character===before.view.session.character&&JSON.stringify(returned.view.character.items)===JSON.stringify(cleared.view.character.items)&&returned.diagnostic.character.spider_dens===cleared.diagnostic.character.spider_dens);
}
