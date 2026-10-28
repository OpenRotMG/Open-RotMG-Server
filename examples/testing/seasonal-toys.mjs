import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
const out=process.env.QA_RESULTS_DIR;
const base={class:0x30e,level:20,seed:892,world:'pirate-cave',position:'spawn',equipment:[-1,-1,-1,-1],backpack:[0xc34,0xc2b,0xc2c,-1,-1,-1,-1,-1],stats:[670,385,50,25,50,50,0,0]};
let target,grave,scoutPlayer;
export async function prepare(api){
  const world=await api({op:'world_info',world:1}),[x,y]=world.spawn_points[0];
  const scout=await api({op:'spawn',spec:{...base,world:1,position:{x,y},guid:'seasonal-scout',controller:'direct',profile:null}});
  const state=await api({op:'inspect',actor:scout.result.actor}),root=join(out,'native');
  const report=JSON.parse(await readFile(join(root,'result.json'),'utf8'));
  const headers={Authorization:'Bearer '+(await readFile(join(root,'state/control.token'),'utf8')).trim()};
  const response=await fetch(report.origin+'/dev/map?player='+state.diagnostic.player,{headers});assert(response.ok);
  const map=await response.json(),blocked=new Set(map.blocked.map(p=>p.join(',')));
  scoutPlayer=state.diagnostic.player;
  target=map.entities.find(e=>e.kind===0x668&&[0,1,2].every(dx=>[0,1,2].every(dy=>!blocked.has([Math.floor(e.pos.x)+dx,Math.floor(e.pos.y)+dy].join(',')))));
  grave=map.entities.find(e=>e.kind===0x6db&&[-3,-2,-1,0].every(dx=>!blocked.has([Math.floor(e.pos.x)+dx,Math.floor(e.pos.y)].join(','))));
  assert(target&&grave,'Expected natural Red Demon and approachable Kage grave');
  await writeFile(join(out,'natural-targets.json'),JSON.stringify({target,grave}));
  return base;
}
export async function run(game){
  const config=JSON.parse(await readFile(join(out,'native/native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim(),samples=[];
  const change=async(actor,change)=>{
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor,change}})});
    const receipt=await response.json();assert(response.ok&&receipt.status==='succeeded',JSON.stringify(receipt));
  };
  const castles=async()=> (await game.inspect()).diagnostic.entities.filter(e=>e.kind===0x140a);
  const initial=await game.observe();
  const peer=await game.join({...base,world:initial.session.world,position:initial.position,backpack:Array(8).fill(-1)});
  for(const actor of [game.actor,peer.actor])await change(actor,{kind:'protection',condition:'invulnerable',duration_ms:60000});
  await game.act({keys:[],duration_ms:600},{name:'seasonal-before',when:()=>true});
  await game.act({keys:[],duration_ms:350,drag:{from:{x:634,y:428},to:{x:634,y:385}}});
  game.check('Sand Pail refuses equipment slot without consuming',JSON.stringify((await game.observe()).character.items)===JSON.stringify(initial.character.items));
  // Selected doses and advertised effects, not recovered server constants: https://openrotmg.org/restoration/reference-data/client-123.5.1.swf
  for(let dose=5;dose>0;dose--){
    const before=await game.observe(),next=dose===1?-1:0xc39-dose+1;
    await game.act({keys:['1'],duration_ms:300},dose===5?{name:'castle-summoned',when:v=>v.character.items[4]===next}:undefined);
    const after=await game.observe();samples.push({dose,before,after});
    game.check('Sand Pail dose '+dose+' commits only the next dose once',after.character.items[4]===next&&after.character.inventory_revision===before.character.inventory_revision+1&&after.character.items.slice(5).every((v,i)=>v===before.character.items[i+5]));
    game.check('Sand Pail dose '+dose+' adds one stationary Castle',(await castles()).length===6-dose);
  }
  const spent=await game.observe();await game.act({keys:['1'],duration_ms:250});
  game.check('empty pail cannot consume or summon again',(await game.observe()).character.inventory_revision===spent.character.inventory_revision&&(await castles()).length===5);
  await change(game.actor,{kind:'item',slot:4,item:0xc34});
  for(let i=0;i<3;i++)await game.act({keys:['1'],duration_ms:250});
  const capped=await game.observe();await game.act({keys:['1'],duration_ms:250});
  const denied=await game.observe();
  game.check('ninth temporary companion refuses without spending a pail dose',(await castles()).length===8&&denied.character.items[4]===capped.character.items[4]&&denied.character.inventory_revision===capped.character.inventory_revision);
  await change(game.actor,{kind:'vitals',hp:100,mp:100});await change(peer.actor,{kind:'vitals',hp:100,mp:100});
  await game.act({keys:[],duration_ms:1200},{name:'castles-heal-nearby',when:v=>v.character.hp>100});
  game.check('stacked Castles heal both injured native players',(await game.observe()).character.hp>=180&&(await peer.observe()).character.hp>=180);
  const fixed=(await castles()).map(e=>({id:e.id,pos:e.pos}));
  await peer.act({keys:['d'],duration_ms:1500});
  const outside=await peer.observe();
  game.check('recipient walked outside reconstructed three-tile range',Math.hypot(outside.position.x-initial.position.x,outside.position.y-initial.position.y)>3);
  await change(peer.actor,{kind:'vitals',hp:100,mp:100});
  await game.act({keys:[],duration_ms:1500});
  game.check('out-of-range player receives no Castle healing',(await peer.observe()).character.hp===100);
  const current=await castles();
  game.check('Castles remain at their summon positions',fixed.every(e=>current.some(c=>c.id===e.id&&c.pos.x===e.pos.x&&c.pos.y===e.pos.y)));
  await change(game.actor,{kind:'vitals',hp:669,mp:100});
  await game.act({keys:[],duration_ms:1200});
  game.check('Castle healing caps at effective maximum without restoring MP',(await game.observe()).character.hp===670&&(await game.observe()).character.mp===100);
  await game.act({keys:[],duration_ms:10000});await game.act({keys:[],duration_ms:10000});await game.act({keys:[],duration_ms:10000});
  game.check('all Castles expire after reconstructed lifetime',(await castles()).length===0);
  await change(game.actor,{kind:'vitals',hp:100,mp:100});await game.act({keys:[],duration_ms:1200});
  game.check('expired Castles stop healing',(await game.observe()).character.hp===100);
  const fire=await game.join({...base,world:1,position:{x:target.pos.x+1,y:target.pos.y+1},backpack:[0xc2b,0xc2c,0xc34,-1,-1,-1,-1,-1]});
  await change(fire.actor,{kind:'protection',condition:'invulnerable',duration_ms:60000});
  for(const [key,slot,kind] of [['1',4,0x1407],['2',5,0x1408]]){
    const start=(await fire.inspect()).diagnostic.entities.find(e=>e.id===target.id);assert(start,'Natural damage target must survive');
    await change(fire.actor,{kind:'teleport',position:{x:start.pos.x+1,y:start.pos.y+1}});
    const before=await fire.observe();
    await fire.act({keys:[key],duration_ms:1400},{name:'firecracker-'+kind.toString(16),when:v=>v.character.items[slot]===-1});
    const after=await fire.observe(),end=(await fire.inspect()).diagnostic.entities.find(e=>e.id===target.id);samples.push({kind,before,after,start,end});
    game.check(kind.toString(16)+' consumes exactly once',after.character.items[slot]===-1&&after.character.inventory_revision===before.character.inventory_revision+1);
    game.check(kind.toString(16)+' damages a natural monster without a weapon',start&&end&&end.hp<start.hp);
    game.check(kind.toString(16)+' expires after its pulse sequence',!(await fire.inspect()).diagnostic.entities.some(e=>e.kind===kind));
  }
  await change(fire.actor,{kind:'teleport',position:{x:grave.pos.x-2.5,y:grave.pos.y}});
  await change(fire.actor,{kind:'item',slot:0,item:0xb08});
  for(let n=0;n<25;n++){
    await fire.act({keys:[],duration_ms:200,target:{entity:grave.id,camera_angle:-Math.PI/4,fire:true}});
    if((await fire.inspect()).diagnostic.entities.find(e=>e.id===grave.id)?.stage>0)break;
  }
  for(let n=0;n<50&&!((await fire.observe()).character.conditions&16);n++)await fire.act({keys:[],duration_ms:200});
  game.check('ordinary Kage encounter inflicts Sick',Boolean((await fire.observe()).character.conditions&16));
  await change(fire.actor,{kind:'vitals',hp:100,mp:100});
  const sick=await fire.observe();await fire.act({keys:['3'],duration_ms:1500},{name:'sick-castle',when:v=>v.character.items[6]===0xc35});
  const unhealed=await fire.observe();
  game.check('Sick permits a pail dose but refuses Castle healing',Boolean(unhealed.character.conditions&16)&&unhealed.character.items[6]===0xc35&&unhealed.character.inventory_revision===sick.character.inventory_revision+1&&unhealed.character.hp===100);
  await fire.act({keys:['Insert'],duration_ms:700});
  const response=await fetch(config.origin+'/dev/map?player='+scoutPlayer,{headers:{Authorization:'Bearer '+token}});assert(response.ok);
  game.check('owner departure removes its still-living Castle',!(await response.json()).entities.some(e=>e.kind===0x140a));
  const beforeExit=await game.observe();await game.act({keys:['Insert'],duration_ms:700});
  game.check('normal Nexus reconnect preserves spent inventory',JSON.stringify((await game.observe()).character.items)===JSON.stringify(beforeExit.character.items));
  await writeFile(join(out,'seasonal-observations.json'),JSON.stringify({preparedInventoryVitalsProtectionAndTeleport:true,samples},null,2));
}
