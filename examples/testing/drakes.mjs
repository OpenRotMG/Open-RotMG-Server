import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
const root=process.env.QA_RESULTS_DIR;
const eggs=[0xaf0,0xaf2,0xaf3,0xaf1,0xaa9,0xaef,0xaa9,0xaef];
const base={class:0x30e,level:20,seed:894,world:1,position:'spawn',equipment:[-1,-1,-1,-1],backpack:eggs,stats:[670,385,50,25,50,50,0,0]};
let target;
export async function prepare(api){
  const world=await api({op:'world_info',world:1}),[x,y]=world.spawn_points[0];
  const scout=await api({op:'spawn',spec:{...base,guid:'drake-scout',position:{x,y},controller:'direct',profile:null}});
  const view=await api({op:'inspect',actor:scout.result.actor});
  const report=JSON.parse(await readFile(join(root,'native/result.json'),'utf8'));
  const token=(await readFile(join(root,'native/state/control.token'),'utf8')).trim();
  const response=await fetch(report.origin+'/dev/map?player='+view.diagnostic.player,{headers:{Authorization:'Bearer '+token}});assert(response.ok);
  const map=await response.json(),blocked=new Set(map.blocked.map(p=>p.join(',')));
  target=map.entities.find(e=>e.kind===0x945&&[-3,-2,-1,0].every(dx=>!blocked.has([Math.floor(e.pos.x)+dx,Math.floor(e.pos.y)].join(','))));
  assert(target,'Natural Horned Drake with a clear approach required');
  return {...base,position:{x:target.pos.x-2.5,y:target.pos.y}};
}
export async function run(game){
  const config=JSON.parse(await readFile(join(root,'native/native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim(),samples=[];
  async function cheat(client,change){
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:client.actor,change}})});
    const receipt=await response.json();assert(response.ok&&receipt.status==='succeeded',JSON.stringify(receipt));
  }
  const pets=view=>view.diagnostic.entities.filter(e=>e.kind>=0x1400&&e.kind<=0x1405);
  async function use(client,slot,kind,name){
    const before=await client.observe();
    await client.act({keys:[String(slot-3)],duration_ms:300},{name,when:v=>v.character.items[slot]===-1});
    const after=await client.inspect();samples.push({name,before,after});
    game.check(name+' consumes exactly one egg',after.view.character.items[slot]===-1&&after.view.character.inventory_revision===before.character.inventory_revision+1&&after.view.character.items.every((item,i)=>i===slot||item===before.character.items[i]));
    game.check(name+' publishes its selected drake',pets(after).some(e=>e.kind===kind)&&after.view.objects.some(e=>e.kind===kind));
    return after;
  }
  await cheat(game,{kind:'protection',condition:'invulnerable',duration_ms:60000});
  await game.act({keys:[],duration_ms:400},{name:'drakes-before',when:()=>true});
  for(const [slot,kind,bit,name] of [[4,0x1402,1<<13,'blue'],[5,0x1404,1<<3,'green'],[6,0x1405,1<<15,'yellow']]){
    let view=await use(game,slot,kind,name);
    if(name==='yellow'){await game.act({keys:[],duration_ms:1200});view=await game.inspect();}
    game.check(name+' applies its enemy condition',view.view.objects.some(e=>e.kind<0x1400&&(e.condition_mask&bit)!==0));
    if(name==='yellow')game.check('yellow causes observed enemy damage',view.diagnostic.entities.some(e=>[0x945,0x946].includes(e.kind)&&e.hp<(e.kind===0x945?1270:700)));
  }
  const beforePurple=await game.inspect();
  const purple=await use(game,8,0x1400,'purple');
  game.check('purple damages an enemy without native weapon fire',purple.diagnostic.entities.some(e=>beforePurple.diagnostic.entities.some(b=>b.id===e.id&&e.hp<b.hp&&(b.hp-e.hp)%30!==0)));
  const orange=await use(game,7,0x1403,'orange');
  game.check('orange applies enemy Stasis',orange.view.objects.some(e=>(e.condition_mask&(1<<21))!==0));
  await cheat(game,{kind:'vitals',hp:100,mp:100});
  const white=await use(game,9,0x1401,'white');
  game.check('white restores injured owner HP',white.view.character.hp>=125);
  const beforeTravel=await game.inspect(),ids=pets(beforeTravel).map(e=>e.id);
  await game.act({keys:['Insert'],duration_ms:700});
  const escaped=await game.inspect();
  game.check('Nexus reconnect preserves identity and consumed eggs',escaped.view.world==='Nexus'&&escaped.view.session.character===beforeTravel.view.session.character&&JSON.stringify(escaped.view.character.items)===JSON.stringify(beforeTravel.view.character.items));
  game.check('temporary drakes do not persist into Nexus',pets(escaped).length===0);
  const observer=await game.join({...base,backpack:Array(8).fill(-1),position:beforeTravel.view.position});
  game.check('old-world observer sees no abandoned drakes',!(await observer.inspect()).diagnostic.entities.some(e=>ids.includes(e.id)));
  const nexus=await game.join({...base,world:-2,position:'spawn'});
  for(const [i,kind] of [0x1402,0x1404,0x1405,0x1403,0x1400,0x1401,0x1400,0x1401].entries())await use(nexus,i+4,kind,'nexus-'+i);
  game.check('eight companions coexist rather than replacing each other',pets(await nexus.inspect()).length===8);
  await cheat(nexus,{kind:'item',slot:4,item:0xaa9});
  const refused=await nexus.observe();await nexus.act({keys:['1'],duration_ms:400},{name:'drakes-limit',when:()=>true});
  const afterRefusal=await nexus.inspect();
  game.check('ninth companion refusal preserves egg and revision',afterRefusal.view.character.items[4]===0xaa9&&afterRefusal.view.character.inventory_revision===refused.character.inventory_revision&&pets(afterRefusal).length===8);
  await nexus.act({keys:['d'],duration_ms:700});
  await nexus.act({keys:[],duration_ms:1000},{name:'drakes-follow',when:()=>true});
  const followed=await nexus.inspect();
  game.check('companions follow native movement',pets(followed).every(e=>Math.hypot(e.pos.x-followed.view.position.x,e.pos.y-followed.view.position.y)<2));
  for(let i=0;i<3;i++)await nexus.act({keys:[],duration_ms:10000});
  const thirty=await nexus.inspect();samples.push({name:'after-thirty-seconds',view:thirty});
  game.check('30-second variants expire while both White drakes remain',pets(thirty).length===2&&pets(thirty).every(e=>e.kind===0x1401));
  for(let i=0;i<3;i++)await nexus.act({keys:[],duration_ms:10000});
  const sixty=await nexus.inspect();samples.push({name:'after-sixty-seconds',view:sixty});
  game.check('White drakes expire without consuming the refused egg',pets(sixty).length===0&&sixty.view.character.items[4]===0xaa9&&sixty.view.character.inventory_revision===refused.character.inventory_revision);
  await nexus.act({keys:[],duration_ms:700},{name:'drakes-expired',when:()=>true});
  await writeFile(join(root,'drake-observations.json'),JSON.stringify({preparedInventoryStatsProtectionAndVitals:true,samples},null,2));
}
