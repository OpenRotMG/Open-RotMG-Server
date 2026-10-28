import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';

export const fixture={class:0x30e,level:20,seed:863,world:'davy-jones-locker',worldSeed:863,position:{x:66.5,y:10.8},equipment:[0xb08,-1,0xb05,0xac5],backpack:Array(8).fill(-1),stats:[670,385,75,25,50,75,40,60]};
const distance=(a,b)=>Math.hypot(a.x-b.x,a.y-b.y);
export async function run(game){
  const out=process.env.QA_RESULTS_DIR,config=JSON.parse(await readFile(join(out,'native/native.json'),'utf8')),token=(await readFile(join(config.state,'control.token'),'utf8')).trim(),samples=[];
  const api=async command=>{const r=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command}),signal:AbortSignal.timeout(10000)}),v=await r.json();assert(r.ok&&v.status!=='failed',JSON.stringify(v));return v;};
  async function save(name){const s=await game.inspect();samples.push({name,state:s});await writeFile(join(out,'crew-observations.json'),JSON.stringify(samples));return s;}
  await api({op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invincible',duration_ms:60000}});
  const initial=await save('prepared-room'),barrel=initial.diagnostic.entities.find(e=>e.kind===0x182e&&distance(e.pos,{x:66.5,y:9.5})<.1);assert(barrel&&barrel.hp===2200);
  for(let n=0;n<10&&(await game.observe()).position.y>10.05;n++)await game.act({keys:['d','w'],duration_ms:400});
  await game.act({keys:['d','w'],duration_ms:900},{name:'ship-barrel-collision',when:()=>true});const blocked=await save('barrel-collision');
  game.check('selected ship barrel stops ordinary northward movement',Math.abs(blocked.view.position.y-10)<.05&&Math.abs(blocked.view.position.x-66.5)<.3);
  for(let n=0;n<25;n++){if(!(await game.inspect()).diagnostic.entities.some(e=>e.id===barrel.id))break;await game.act({keys:[],duration_ms:400,target:{entity:barrel.id,camera_angle:-Math.PI/4,fire:true}});}
  const destroyed=await save('barrel-destroyed');
  game.check('ordinary staff destroys the ship barrel without substituting Wine Barrel',!destroyed.diagnostic.entities.some(e=>e.id===barrel.id||e.kind===0xde5));
  await game.act({keys:['d','w'],duration_ms:250});const crossed=await save('barrel-crossed');
  game.check('destroyed barrel releases collision tile',crossed.view.position.y<9.2);
  await game.act({keys:['a','s'],duration_ms:750});
  const room=await save('room-population'),ghost=room.diagnostic.entities.filter(e=>e.kind===0xe31).sort((a,b)=>distance(a.pos,room.view.position)-distance(b.pos,room.view.position))[0];assert(ghost&&distance(ghost.pos,room.view.position)<8);
  for(let n=0;n<25;n++){if(!(await game.inspect()).diagnostic.entities.some(e=>e.id===ghost.id))break;await game.act({keys:[],duration_ms:250,target:{entity:ghost.id,camera_angle:-Math.PI/4,fire:true}});}
  const dead=await save('ghost-killed');game.check('ordinary staff defeats a renewable room ghost',!dead.diagnostic.entities.some(e=>e.id===ghost.id));
  await game.act({keys:[],duration_ms:6500},{name:'ship-ghost-regrowth',when:()=>true});const regrown=await save('ghost-regrown');
  game.check('nearby revisit regenerates a new ghost rather than resurrecting old identity',regrown.diagnostic.entities.some(e=>e.kind===0xe31&&e.id!==ghost.id&&distance(e.home,ghost.home)<.01));
  const roger=regrown.diagnostic.entities.find(e=>e.kind===0xe45&&distance(e.pos,regrown.view.position)<13);assert(roger);
  let souls=[];
  for(let n=0;n<16;n++){const s=await save('roger-offspring');souls=s.diagnostic.entities.filter(e=>e.kind===0xe48&&e.parent===roger.id);game.check('Roger offspring remain bounded at sample '+n,souls.length<=4);if(souls.some(e=>e.stage===1))break;await game.act({keys:[],duration_ms:500});}
  game.check('natural Roger produces actual owned charging Lost Soul offspring',souls.some(e=>e.stage===1));
  await game.act({keys:[],duration_ms:3000},{name:'ship-room-crew',when:()=>true});const final=await save('room-revisit');
  game.check('observed charging Souls expire without attacking them',souls.filter(e=>e.stage===1).every(e=>!final.diagnostic.entities.some(other=>other.id===e.id)));
  game.check('barrel remains destroyed during room revisit',!final.diagnostic.entities.some(e=>e.id===barrel.id));
  game.check('prepared room has no invented Maggot or Sack spawns',!final.diagnostic.entities.some(e=>e.kind===0xe46||e.kind===0xe47));
}
