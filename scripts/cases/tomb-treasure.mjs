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
  await game.act({keys:[],duration_ms:2500});
  const initial=await save('initial');
  game.check('optional treasure begins at selected full 50000 HP',initial.map.entities.find(e=>e.id===treasure.id).hp===50000);
  await game.act({keys:[],duration_ms:500,target:{entity:treasure.id,camera_angle:-Math.PI/4,fire:true}});
  const jarsAt=(await game.observe()).game_ms;
  await game.act({keys:[],duration_ms:8500},{name:'tomb-canopic-jars',when:v=>v.game_ms>=jarsAt+8000});
  const spawned=await save('jars');
  const jars=spawned.map.entities.filter(e=>e.kind>=0xd2d&&e.kind<=0xd30);
  game.check('ordinary damage produces eight jars of four selected kinds',jars.length===8&&new Set(jars.map(e=>e.kind)).size===4&&spawned.map.entities.some(e=>e.id===treasure.id&&e.hp<49500));
  for(let n=0;n<15&&(await map()).entities.some(e=>e.id===treasure.id);n++)await game.act({keys:[],duration_ms:1500,target:{entity:treasure.id,camera_angle:-Math.PI/4,fire:true}});
  const defeated=await save('treasure-defeated');
  game.check('native staff defeats the optional treasure without inventing XP',!defeated.map.entities.some(e=>e.id===treasure.id)&&defeated.state.diagnostic.character.kills>initial.state.diagnostic.character.kills&&defeated.state.view.character.experience===initial.state.view.character.experience);
  const remaining=defeated.map.entities.filter(e=>e.kind>=0xd2d&&e.kind<=0xd30);assert(remaining.length,'A surviving jar is required');
  const target=remaining.reduce((a,b)=>Math.hypot(a.pos.x-initial.state.view.position.x,a.pos.y-initial.state.view.position.y)<Math.hypot(b.pos.x-initial.state.view.position.x,b.pos.y-initial.state.view.position.y)?a:b);
  await game.act({keys:[],duration_ms:700,target:{entity:target.id,camera_angle:-Math.PI/4,fire:true}});
  const broken=await save('jar-broken');
  game.check('native fire breaks a real jar and records its kill',!broken.map.entities.some(e=>e.id===target.id)&&broken.state.diagnostic.character.kills>defeated.state.diagnostic.character.kills);
  const bag=broken.state.diagnostic.entities.find(e=>e.items?.some(i=>i!==-1)&&e.extra_stats.some(([stat,owner])=>stat===54&&owner===broken.state.view.session.account));assert(bag,'This seeded treasure must produce an eligible private reward');
  for(let n=0;n<25;n++){
    const v=await game.observe(),dx=bag.pos.x-v.position.x,dy=bag.pos.y-v.position.y;if(Math.hypot(dx,dy)<.8)break;
    const x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2,keys=[];
    if(Math.abs(x)>=Math.abs(y)*.4142)keys.push(x>0?'d':'a');if(Math.abs(y)>=Math.abs(x)*.4142)keys.push(y>0?'s':'w');
    await game.act({keys,duration_ms:100});
  }
  const atBag=await game.observe();assert(Math.hypot(atBag.position.x-bag.pos.x,atBag.position.y-bag.pos.y)<.8,'Native reward approach failed');
  await game.act({keys:[],duration_ms:800},{name:'tomb-optional-reward',when:()=>true});
  const slot=bag.items.findIndex(i=>i!==-1),item=bag.items[slot],revision=(await game.observe()).character.inventory_revision;
  const from={x:634+(slot%4)*44,y:528+Math.floor(slot/4)*44};
  await game.act({keys:[],duration_ms:350,drag:{from,to:{x:634,y:428}}});
  const picked=await save('picked');game.check('native drag commits the optional reward once',picked.state.view.character.items[4]===item&&picked.state.view.character.inventory_revision===revision+1);
  await game.act({keys:[],duration_ms:350,drag:{from,to:{x:678,y:428}}});
  game.check('empty optional reward slot cannot duplicate its item',(await game.observe()).character.inventory_revision===revision+1);
  game.check('treasure and jar rewards do not awaken bosses or count completion',broken.map.entities.filter(e=>[0xd3d,0xd3e,0xd3f].includes(e.kind)).length===3&&broken.map.entities.filter(e=>e.kind===0xd29).length===layout.entities.filter(e=>e.kind===0xd29).length&&broken.state.diagnostic.character.tombs===0&&broken.arena.diagnostic.character.tombs===0);
  await game.act({keys:['Insert'],duration_ms:300});
  const returned=await game.observe();game.check('ordinary escape keeps the same living character and optional item without clear credit',returned.world==='Nexus'&&returned.lifecycle.alive&&returned.session.character===initial.state.view.session.character&&returned.character.items[4]===item&&returned.character.inventory_revision===revision+1&&(await game.inspect()).diagnostic.character.tombs===0);
}
