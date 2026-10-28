import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';

export const fixture={class:0x30e,level:20,seed:860,world:'tomb-of-the-ancients',worldSeed:12,position:'spawn',equipment:[0xb08,-1,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,75,25,50,75,40,60]};
export async function run(observer){
  const out=process.env.QA_RESULTS_DIR,config=JSON.parse(await readFile(join(out,'native/native.json'),'utf8'));
  const headers={Authorization:'Bearer '+(await readFile(join(config.state,'control.token'),'utf8')).trim(),'Content-Type':'application/json'};
  let serial=0;
  const api=async command=>{const r=await fetch(config.origin+'/dev/testing',{method:'POST',headers,body:JSON.stringify({id:'tomb-treasure-'+(++serial),command}),signal:AbortSignal.timeout(10000)});const s=await r.json();assert(r.ok&&s.status!=='failed',JSON.stringify(s));return s;};
  const start=await observer.observe();
  const map=async()=>{const r=await fetch(config.origin+'/dev/map?player='+start.session.player,{headers,signal:AbortSignal.timeout(10000)});assert(r.ok);return r.json();};
  const layout=await map(),treasure=layout.entities.find(e=>e.kind===0xd2c);assert(treasure,'Generated optional treasure required');
  const {worldSeed,...spec}=fixture;
  const game=await observer.join({...spec,world:start.session.world,position:{x:treasure.pos.x-3,y:treasure.pos.y}});
  await api({op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}});
  await api({op:'cheat',actor:game.actor,change:{kind:'stat_modifier',index:2,amount:100,duration_ms:60000}});
  const records=[];
  const save=async label=>{const s={label,state:await game.inspect(),arena:await observer.inspect(),map:await map()};records.push(s);await writeFile(join(out,'treasure.json'),JSON.stringify(records));return s;};
  const initial=await save('initial');
  game.check('optional treasure begins at selected full 50000 HP',initial.map.entities.find(e=>e.id===treasure.id).hp===50000);
  await game.act({keys:[],duration_ms:500,target:{entity:treasure.id,camera_angle:-Math.PI/4,fire:true}});
  await game.act({keys:[],duration_ms:8500},{name:'tomb-canopic-jars',when:()=>true});
  const spawned=await save('jars');
  const jars=spawned.map.entities.filter(e=>e.kind>=0xd2d&&e.kind<=0xd30);
  game.check('ordinary damage produces eight jars of four selected kinds',jars.length===8&&new Set(jars.map(e=>e.kind)).size===4&&spawned.map.entities.some(e=>e.id===treasure.id&&e.hp<49500));
  for(let n=0;n<15&&(await map()).entities.some(e=>e.id===treasure.id);n++)await game.act({keys:[],duration_ms:1500,target:{entity:treasure.id,camera_angle:-Math.PI/4,fire:true}});
  const defeated=await save('treasure-defeated');
  game.check('native staff defeats the optional treasure and earns XP',!defeated.map.entities.some(e=>e.id===treasure.id)&&defeated.state.view.character.experience>initial.state.view.character.experience);
  const target=jars.reduce((a,b)=>Math.hypot(a.pos.x-initial.state.view.position.x,a.pos.y-initial.state.view.position.y)<Math.hypot(b.pos.x-initial.state.view.position.x,b.pos.y-initial.state.view.position.y)?a:b);
  await game.act({keys:[],duration_ms:700,target:{entity:target.id,camera_angle:-Math.PI/4,fire:true}});
  const broken=await save('jar-broken');
  game.check('native fire breaks a real jar and produces its Scarab',!broken.map.entities.some(e=>e.id===target.id)&&broken.map.entities.some(e=>e.kind===0xd1b&&e.tomb?.producer===target.id));
  game.check('treasure and jar rewards do not awaken bosses or count completion',broken.map.entities.filter(e=>[0xd3d,0xd3e,0xd3f].includes(e.kind)).length===3&&broken.map.entities.filter(e=>e.kind===0xd29).length===layout.entities.filter(e=>e.kind===0xd29).length&&broken.state.diagnostic.character.tombs===0&&broken.arena.diagnostic.character.tombs===0);
  await game.act({keys:['Insert'],duration_ms:300});
  const returned=await game.observe();game.check('ordinary escape keeps the same living character and earned XP without clear credit',returned.world==='Nexus'&&returned.lifecycle.alive&&returned.session.character===initial.state.view.session.character&&returned.character.experience===broken.state.view.character.experience&&(await game.inspect()).diagnostic.character.tombs===0);
}
