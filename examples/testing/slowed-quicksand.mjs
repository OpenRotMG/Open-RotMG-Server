import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
const out=process.argv[process.argv.indexOf('--run-dir')+1];
export const fixture={class:0x31d,level:20,seed:937,world:'tomb-of-the-ancients',worldSeed:12,position:'spawn',equipment:[0xa00,-1,-1,-1],backpack:Array(8).fill(-1),stats:[770,252,75,25,50,50,75,50]};
export async function run(game){
  const config=JSON.parse(await readFile(join(out,'native.json'),'utf8'));
  const headers={Authorization:'Bearer '+(await readFile(join(config.state,'control.token'),'utf8')).trim(),'Content-Type':'application/json'},records=[];
  async function prepare(change){
    const r=await fetch(config.origin+'/dev/testing',{method:'POST',headers,body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:game.actor,change}}),signal:AbortSignal.timeout(10000)});
    const result=await r.json();assert(r.ok&&result.status==='succeeded',JSON.stringify(result));
  }
  async function save(label){const s=await game.inspect();records.push({label,state:s});await writeFile(join(out,'quicksand.json'),JSON.stringify(records,null,2));return s.view;}
  async function place(position){await prepare({kind:'teleport',position});await game.act({keys:[],duration_ms:150});}
  const initial=await save('prepared');
  const response=await fetch(config.origin+'/dev/map?player='+initial.session.player,{headers,signal:AbortSignal.timeout(10000)});assert(response.ok);
  const layout=await response.json(),blocked=new Set(layout.blocked.map(p=>p.join(','))),turret=layout.entities.find(e=>e.kind===0xd36);assert(turret,'Generated Tomb lacks Center Frost Turret');
  const candidates=[];
  for(let y=16;y<145;y+=16)for(let x=16;x<145;x+=16){
    if((x%32===16)===(y%32===16))continue;
    if([-1,0,1].every(dx=>[-1,0,1].every(dy=>!blocked.has([x+dx,y+dy].join(',')))))candidates.push({x:x+.5,y:y+.5});
  }
  assert(candidates.length,'Generated Tomb lacks a walkable corridor midpoint');
  await prepare({kind:'protection',condition:'invulnerable',duration_ms:60000});
  await place(candidates[0]);
  const sand=await save('sand');
  game.check('observed corridor midpoint is actual selected Quicksand',sand.tiles.some(t=>t.x===Math.floor(sand.position.x)&&t.y===Math.floor(sand.position.y)&&t.ground===0xbf));
  await place({x:turret.pos.x,y:turret.pos.y+3});
  for(let n=0;n<20&&!((await game.observe()).character.conditions&(1<<3));n++)await game.act({keys:[],duration_ms:250});
  const hit=await save('turret-hit');
  game.check('natural Center Frost Turret applies Slowed',hit.character.conditions&(1<<3));
  await place(candidates[0]);
  await game.act({keys:[],duration_ms:2100});
  const sunk=await save('sunk');
  await game.act({keys:['d','s'],duration_ms:1000},{name:'slowed-quicksand',when:()=>true});
  const after=await save('slowed-sunk-movement'),distance=Math.hypot(after.position.x-sunk.position.x,after.position.y-sunk.position.y);
  game.check('Slowed remains active throughout Quicksand measurement',(sunk.character.conditions&(1<<3))&&(after.character.conditions&(1<<3)));
  game.check('fully sunk Slowed movement is 0.4 tiles per second',distance>.2&&distance<.65);
  await place(initial.position);
  await game.act({keys:[],duration_ms:4500});
  const floor=await save('dry-floor');
  game.check('ordinary floor clears sinking and timed Slowed expires',!(floor.character.conditions&(1<<3))&&floor.tiles.some(t=>t.x===Math.floor(floor.position.x)&&t.y===Math.floor(floor.position.y)&&t.ground!==0xbf&&t.ground_speed===1));
  await game.act({keys:['d','s'],duration_ms:500},{name:'quicksand-recovered',when:()=>true});
  const recovered=await save('recovered'),moved=Math.hypot(recovered.position.x-floor.position.x,recovered.position.y-floor.position.y);
  game.check('actual movement recovers after leaving Quicksand',moved>3&&recovered.lifecycle.alive&&recovered.session.character===initial.session.character);
}
