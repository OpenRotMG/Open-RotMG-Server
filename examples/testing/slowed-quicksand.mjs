import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
const out=process.argv[process.argv.indexOf('--run-dir')+1];
export const fixture={class:0x31d,level:20,seed:937,world:'tomb-of-the-ancients',worldSeed:12,position:'spawn',equipment:[0xa00,-1,-1,-1],backpack:Array(8).fill(-1),stats:[770,252,75,25,50,50,75,50]};
export async function run(observer){
  const config=JSON.parse(await readFile(join(out,'native.json'),'utf8'));
  const headers={Authorization:'Bearer '+(await readFile(join(config.state,'control.token'),'utf8')).trim(),'Content-Type':'application/json'},records=[];
  const initial=await observer.observe();
  const response=await fetch(config.origin+'/dev/map?player='+initial.session.player,{headers,signal:AbortSignal.timeout(10000)});assert(response.ok);
  const layout=await response.json(),blocked=new Set(layout.blocked.map(p=>p.join(','))),candidates=[];
  for(let y=16;y<145;y+=32)for(let x=32;x<145;x+=32){
    if([-1,0,1].every(dx=>[-1,0,1].every(dy=>!blocked.has([x+dx,y+dy].join(',')))))candidates.push({x:x+.5,y:y+.5});
  }
  assert(candidates.length,'Generated Tomb lacks a horizontal corridor midpoint');
  const {worldSeed,...spec}=fixture,game=await observer.join({...spec,world:initial.session.world,position:candidates[0]});
  const r=await fetch(config.origin+'/dev/testing',{method:'POST',headers,body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invincible',duration_ms:60000}}}),signal:AbortSignal.timeout(10000)});
  const protectedResult=await r.json();assert(r.ok&&protectedResult.status==='succeeded',JSON.stringify(protectedResult));
  async function save(label){const s=await game.inspect();records.push({label,state:s});await writeFile(join(out,'quicksand.json'),JSON.stringify(records,null,2));return s.view;}
  const tile=v=>{const t=v.tiles.find(t=>t.x===Math.floor(v.position.x)&&t.y===Math.floor(v.position.y));assert(t,'Current native tile is unknown');return t;};
  const start=await save('prepared-on-sand');
  game.check('fresh native starting context is actual selected Quicksand',tile(start).ground===0xbf);
  await game.act({keys:[],duration_ms:2500});
  const sunk=await game.act({keys:['d','s'],duration_ms:1000},{name:'quicksand-sunk',when:()=>true});
  const sand=await save('sunk-movement');
  game.check('fully sunk native movement advances slowly while remaining on Quicksand',tile(sand).ground===0xbf&&sunk.effects.distance>.1&&sunk.effects.distance<1);
  for(let n=0;n<12&&tile((await game.inspect()).view).ground===0xbf;n++)await game.act({keys:['d','s'],duration_ms:1000});
  const floor=await save('walked-off-sand');
  game.check('ordinary native walking reaches non-Sinking floor',tile(floor).ground!==0xbf&&tile(floor).ground_speed>=.85);
  const recovered=await game.act({keys:['d','s'],duration_ms:1000},{name:'quicksand-recovered',when:()=>true});
  const end=await save('recovered');
  game.check('leaving Quicksand resets sinking and restores native movement',recovered.effects.distance>sunk.effects.distance*3&&end.lifecycle.alive&&end.session.character===start.session.character);
  game.check('terrain slowing is not a Slowed condition bit',!(sand.character.conditions&(1<<3))&&!(end.character.conditions&(1<<3)));
  records.push({sunk:sunk.effects,recovered:recovered.effects});await writeFile(join(out,'quicksand.json'),JSON.stringify(records,null,2));
}
