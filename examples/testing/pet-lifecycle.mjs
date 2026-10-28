import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
import {setTimeout as delay} from 'node:timers/promises';
export const fixture={class:0x30e,level:20,seed:8931,world:'ocean-trench',position:'spawn',equipment:[-1,-1,-1,-1],backpack:[0x1539,0x1513,-1,-1,-1,-1,-1,-1],stats:[670,385,75,25,50,75,40,60]};
export async function run(game){
  const root=join(process.env.QA_RESULTS_DIR,'native'),records=[];
  const config=JSON.parse(await readFile(join(root,'native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  async function api(command){
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command})});
    const value=await response.json();assert(response.ok&&value.status==='succeeded',JSON.stringify(value));return value;
  }
  async function save(label,client=game){const record={label,...await client.inspect()};records.push(record);await writeFile(join(root,'lifecycle-observations.json'),JSON.stringify(records));return record;}
  await game.act({keys:['1'],duration_ms:500});
  const initial=await save('own-baby'),pet=initial.view.objects.find(o=>o.kind===0x1639);assert(pet);
  const position={x:initial.view.position.x,y:initial.view.position.y+2};
  assert(initial.diagnostic.entities.filter(e=>e.kind===0x731).every(e=>Math.hypot(e.pos.x-position.x,e.pos.y-position.y)>=1));
  const spec={...fixture,guid:'pet-mortal',world:initial.view.session.world,position,level:1,stats:[100,100,12,0,10,15,12,12],controller:'direct',profile:null};
  const victim=(await api({op:'spawn',spec})).result.actor;
  const actions=await Promise.allSettled([game.act({keys:[],duration_ms:1800}),(async()=>{
    const deadline=Date.now()+5000;
    while((await game.observe()).paused){assert(Date.now()<deadline,'Native mortality action did not resume');await delay(30);}
    await api({op:'act',actor:victim,action:{command:'use',slot:4,x:position.x,y:position.y},duration_ms:0,pauseAfter:false});
  })()]);
  for(const result of actions)if(result.status==='rejected')throw result.reason;
  const selected=await api({op:'inspect',actor:victim});
  game.check('direct mortality fixture commits its own Baby selection',selected.diagnostic.character.pet_kind===0x1639&&selected.view.character.items[4]===0x1539);
  for(let i=0;i<3;i++)await game.act({keys:[],duration_ms:9000});
  const death=await save('committed-peer-death');
  game.check('native witness sees one grave and only its own Baby after peer death',death.diagnostic.entities.filter(e=>e.kind===0x723&&e.name===selected.diagnostic.character.name).length===1&&death.diagnostic.entities.filter(e=>e.kind===0x1639).length===1&&death.view.objects.some(o=>o.id===pet.id));
  const replacement=(await api({op:'spawn',spec:{...spec,backpack:Array(8).fill(-1)}})).result.actor;
  const fresh=await api({op:'inspect',actor:replacement});
  game.check('replacement on dead account never inherits deceased selection or generator',replacement!==victim&&fresh.diagnostic.character.account===selected.diagnostic.character.account&&fresh.diagnostic.character.pet_kind===null&&fresh.view.character.items[4]===-1);
  await writeFile(join(root,'mortality-fixtures.json'),JSON.stringify({victim,selected,replacement,fresh}));
  await game.act({keys:['Insert'],duration_ms:800});
  const nexus=await save('nexus');assert.equal(nexus.view.map.name,'Nexus');
  const peer=await game.join({...fixture,world:-2,seed:8932,position:nexus.view.position,backpack:[0x1513,-1,-1,-1,-1,-1,-1,-1]});
  await peer.act({keys:['1'],duration_ms:500});
  const two=await save('two-owners'),fox=two.view.objects.find(o=>o.kind===0x1613);assert(fox);
  game.check('two living owners retain separate visible selections',two.view.objects.filter(o=>[0x1613,0x1639].includes(o.kind)).length===2);
  await game.act({keys:[],duration_ms:350,drag:{from:{x:634,y:428},to:{x:300,y:325}}});
  const dropped=await save('generator-dropped'),bag=dropped.diagnostic.entities.find(e=>e.items?.includes(0x1539));assert(bag);
  game.check('dropping reusable generator does not dismiss selection or transfer ownership',dropped.view.character.items[4]===-1&&dropped.view.objects.some(o=>o.kind===0x1639)&&bag.bag_owners.includes(dropped.view.session.account));
  const foreignBefore=await save('foreign-before',peer);
  await peer.act({keys:[],duration_ms:350,drag:{from:{x:634,y:528},to:{x:678,y:428}}});
  const foreignAfter=await save('foreign-after',peer);
  game.check('foreign native cannot see or collect soulbound generator',!foreignAfter.view.objects.some(o=>o.id===bag.id)&&JSON.stringify(foreignAfter.view.character.items)===JSON.stringify(foreignBefore.view.character.items)&&foreignAfter.view.character.inventory_revision===foreignBefore.view.character.inventory_revision);
  const portal=dropped.diagnostic.entities.find(e=>e.kind===0x720);assert(portal);
  for(let i=0;i<50;i++){
    const v=await game.observe(),dx=portal.pos.x-v.position.x,dy=portal.pos.y-v.position.y,distance=Math.hypot(dx,dy);
    if(distance<1)break;
    const x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2,keys=[];
    if(Math.abs(x)>.12)keys.push(x>0?'d':'a');
    if(Math.abs(y)>.12)keys.push(y>0?'s':'w');
    await game.act({keys,duration_ms:Math.min(300,Math.max(50,Math.round(distance*80)))});
  }
  await game.act({keys:[],mouse:{x:700,y:575},duration_ms:800});
  const vault=await save('vault');
  game.check('ordinary private Vault reconnect retains pet without carrying generator',vault.view.map.name==='Vault'&&vault.view.session.character===initial.view.session.character&&vault.view.character.items[4]===-1&&vault.view.objects.filter(o=>o.kind===0x1639).length===1);
  const stayed=await save('peer-stayed',peer);
  game.check('other owner and companion remain in Nexus',stayed.view.map.name==='Nexus'&&stayed.view.objects.some(o=>o.id===fox.id));
  await game.act({keys:['Insert'],duration_ms:800});
  await game.act({keys:[],duration_ms:700},{name:'pet-ownership',when:()=>true});
  const final=await save('final');
  game.check('second reconnect preserves same living identity without duplicating selection',final.view.map.name==='Nexus'&&final.view.lifecycle.alive&&final.view.session.character===initial.view.session.character&&final.view.objects.filter(o=>o.kind===0x1639).length===1);
}
