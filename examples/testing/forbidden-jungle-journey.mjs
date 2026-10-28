import assert from 'node:assert/strict';
import {readFile,writeFile,appendFile} from 'node:fs/promises';
import {join} from 'node:path';
export const fixture={class:0x310,level:20,seed:12351,world:1,position:'spawn',equipment:[0xaf6,0xb25,0xb05,0xacd],backpack:[0xc19,0xa22,0xa22,0xa22,0xa23,0xa23,0xa23,0xa23],stats:[670,385,50,25,50,50,40,75]};
export async function run(game){
 const out=process.argv[process.argv.indexOf('--run-dir')+1];
 const log=async label=>{const s=await game.inspect();await appendFile(join(out,'journey.jsonl'),JSON.stringify({label,...s})+'\n');return s;};
 const capture=async name=>{await game.act({keys:[],duration_ms:700},{name,when:()=>true});};
 const keys=(a,b)=>{const dx=b.x-a.x,dy=b.y-a.y,x=dx-dy,y=dx+dy;return [Math.abs(x)>.1&&Math.abs(x)>=Math.abs(y)*.4142?(x>0?'d':'a'):null,Math.abs(y)>.1&&Math.abs(y)>=Math.abs(x)*.4142?(y>0?'s':'w'):null].filter(Boolean);};
 async function move(point){
  for(let n=0;n<90;n++){
   const v=await game.observe(),d=Math.hypot(point.x-v.position.x,point.y-v.position.y);if(d<.4)return;
   await game.act({keys:keys(v.position,point),duration_ms:Math.max(50,Math.min(500,Math.round(d*100)))});
  }
  assert.fail('Native route blocked '+JSON.stringify(point));
 }
 async function heal(){const v=await game.observe();if(v.character.hp<650&&!(v.character.conditions&2)){
  if(v.character.mp>=90)await game.act({keys:[' '],duration_ms:350});
  else{const slot=v.character.items.findIndex((k,i)=>i>=8&&k===0xa23);if(slot>=8)await game.act({keys:[String(slot-3)],duration_ms:350});}
 }}
 const initial=await log('realm');
 await game.act({keys:['1'],duration_ms:900});const opened=await log('key-consumed'),portal=opened.diagnostic.entities.find(e=>e.kind===0x733);assert(portal);
 game.check('one supplied Totem Key commits one usable entrance',opened.view.character.items[4]===-1&&portal.published);
 await move(portal.pos);await capture('key-portal');await game.act({keys:[],mouse:{x:700,y:580},duration_ms:1000});
 game.check('normal portal interaction enters Jungle',(await game.observe()).world==='Forbidden Jungle');
 await game.act({keys:[],duration_ms:2500});await capture('jungle-arrival');
 const entries=(await readFile(join(out,'server.log'),'utf8')).split('\n').filter(l=>l.startsWith('{')).map(l=>JSON.parse(l));
 const generated=entries.findLast(e=>e.fields?.message==='generated branching Forbidden Jungle').fields;
 const rooms=JSON.parse(generated.layout.replaceAll('(','[').replaceAll(')',']')),path=[];
 for(let i=generated.boss_room;i!==0;i=rooms[i][2])path.unshift(i);
 await writeFile(join(out,'route.json'),JSON.stringify({generated,path}));
 for(const i of path){const [x,y]=rooms[i];await move({x:20+x*20+.5-(i===generated.boss_room?9:0),y:20+y*20+.5});await heal();await log('room-'+i);}
 await capture('boss-arrival');const stages=new Set();
 for(let n=0;n<90;n++){
  const s=await log('boss-'+n),boss=s.diagnostic.entities.find(e=>e.kind===0xdc3);if(!boss)break;stages.add(boss.stage);
  const d=Math.hypot(boss.pos.x-s.view.position.x,boss.pos.y-s.view.position.y);
  if(d>9.7){await move({x:boss.pos.x-9,y:boss.pos.y});continue;}
  if(d<7.5&&boss.stage!==6){await move({x:boss.pos.x-9,y:boss.pos.y});continue;}
  await heal();await game.act({keys:[],duration_ms:1000,target:{entity:boss.id,camera_angle:-Math.PI/4,fire:true}});
 }
 const clear=await log('defeat');
 game.check('ordinary native shots defeat Mixcoatl after actual phases',!clear.diagnostic.entities.some(e=>e.kind===0xdc3)&&stages.has(1)&&stages.has(6));
 game.check('one Jungle clear is committed',clear.diagnostic.character.jungles===1);
 const bag=clear.diagnostic.entities.find(e=>e.items?.includes(0xc1a));assert(bag);await heal();await move(bag.pos);
 await capture('earned-rewards');const before=await game.observe(),slot=bag.items.indexOf(0xc1a);
 const drag={from:{x:634+(slot%4)*44,y:528+Math.floor(slot/4)*44},to:{x:634,y:428}};
 await game.act({keys:[],duration_ms:350,drag});const picked=await log('pollen-picked');
 game.check('native drag picks up earned Pollen exactly once',picked.view.character.items[4]===0xc1a&&picked.view.character.inventory_revision===before.character.inventory_revision+1);
 await game.act({keys:[],duration_ms:350,drag:{from:drag.from,to:{x:678,y:428}}});
 game.check('empty earned bag slot cannot duplicate Pollen',(await game.observe()).character.inventory_revision===picked.view.character.inventory_revision);
 const exit=picked.diagnostic.entities.find(e=>e.kind===0x704&&e.published);assert(exit);await heal();await move(exit.pos);await capture('completion-exit');
 await game.act({keys:[],duration_ms:1000,mouse:{x:700,y:580}});await game.act({keys:[],duration_ms:2000});const final=await log('returned');
 game.check('same living character returns through completion exit with loot and one clear',final.view.world==='Realm'&&final.view.session.world===initial.view.session.world&&final.view.session.character===initial.view.session.character&&final.view.lifecycle.alive&&final.diagnostic.character.jungles===1&&final.view.character.items[4]===0xc1a);
 await capture('realm-return');
}
