import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';

const spec={class:0x310,level:20,seed:901,world:-2,position:'spawn',equipment:[0xaf6,0xb25,0xb05,0xac5],backpack:Array(8).fill(-1),stats:[670,385,50,25,50,50,40,75]};
export async function prepare(){
  const root=join(process.env.QA_RESULTS_DIR,'native'),{origin}=JSON.parse(await readFile(join(root,'result.json'),'utf8'));
  const token=(await readFile(join(root,'state','control.token'),'utf8')).trim();
  const response=await fetch(origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:'portal-capacity',command:{op:'prepare_realm_capacity',limit:1}}),signal:AbortSignal.timeout(10000)});
  const receipt=await response.json();assert(response.ok&&receipt.status==='succeeded',JSON.stringify(receipt));
  await writeFile(join(process.env.QA_RESULTS_DIR,'capacity.json'),JSON.stringify(receipt));
  return spec;
}
export async function run(game){
  const observations=[];
  async function save(name,client=game){
    const view=await client.observe();
    view.entrances=(await client.inspect()).view.objects.filter(o=>o.kind===0x712);
    observations.push({name,view});
    await writeFile(join(process.env.QA_RESULTS_DIR,'portals.json'),JSON.stringify(observations,null,2));return view;
  }
  async function capture(name){
    await game.act({keys:[],duration_ms:350});
    await game.act({keys:[],duration_ms:700},{name,when:()=>true});
  }
  async function walk(pos){
    for(let i=0;i<80;i++){
      const v=await game.observe(),dx=pos.x-v.position.x,dy=pos.y-v.position.y,distance=Math.hypot(dx,dy);
      if(distance<.6)return;
      const x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2,keys=[];
      if(Math.abs(x)>.12)keys.push(x>0?'d':'a');
      if(Math.abs(y)>.12)keys.push(y>0?'s':'w');
      await game.act({keys,duration_ms:Math.min(300,Math.max(50,Math.round(distance*80)))});
    }
    throw Error('Native walk did not reach Realm entrance');
  }
  async function escape(client){
    await client.act({keys:['Insert'],duration_ms:700});
    for(let i=0;i<8&&(await client.observe()).world!=='Nexus';i++)await client.act({keys:[],duration_ms:500});
    assert.equal((await client.observe()).world,'Nexus');
  }
  async function enter(){
    await game.act({keys:[],mouse:{x:700,y:575},duration_ms:700});
    for(let i=0;i<8&&(await game.observe()).world==='Nexus';i++)await game.act({keys:[],duration_ms:500});
    const view=await game.observe();assert.equal(view.world,'Realm');return view;
  }
  const initial=await save('initial');
  let portals=[];
  for(let i=0;i<60&&portals.length<2;i++){
    await game.act({keys:['d','w'],duration_ms:300});
    portals=(await game.inspect()).diagnostic.entities.filter(e=>e.kind===0x712);
  }
  assert.equal(portals.length,2,'Walking north did not reveal both Realm entrances');
  await walk(portals[0].pos);
  await capture('realm-portals-open');
  const open=await save('open');
  const entrances=open.entrances;
  game.check('two differently named available entrances are visible',entrances.length===2&&new Set(entrances.map(o=>o.name.split(' (')[0])).size===2&&entrances.every(o=>o.usable));
  const occupant=await game.join({...spec,seed:902,world:-3});
  const filled=await save('occupant',occupant);
  await capture('realm-portals-full');
  const full=await save('full');
  const unavailable=full.entrances.find(o=>o.id===portals[0].id),available=full.entrances.find(o=>o.id===portals[1].id);
  game.check('live population binds Full only to its occupied Realm',unavailable?.name.endsWith('(1/1)')&&!unavailable.usable&&available?.name.endsWith('(0/85)')&&available.usable);
  await game.act({keys:[],mouse:{x:700,y:575},duration_ms:700});
  const denied=await save('denied');
  game.check('full entrance click retains the exact living Nexus session and inventory',denied.lifecycle.alive&&JSON.stringify(denied.session)===JSON.stringify(full.session)&&JSON.stringify(denied.character.items)===JSON.stringify(initial.character.items));
  await escape(occupant);
  await capture('realm-portals-reopened');
  const reopened=await save('reopened'),entrance=reopened.entrances.find(o=>o.id===portals[0].id);
  game.check('departure restores the same empty available entrance',entrance?.name.endsWith('(0/1)')&&entrance.usable);
  const first=await enter();await save('entered-first');
  game.check('reopened entrance reaches its exact Realm',first.session.world===filled.session.world);
  await escape(game);
  await walk(portals[1].pos);
  const second=await enter();await save('entered-second');
  game.check('second named entrance reaches a different Realm',second.session.world!==first.session.world);
  await escape(game);
  const final=await save('final'),other=await save('other-final',occupant);
  game.check('both accounts remain alive and distinct',final.lifecycle.alive&&other.lifecycle.alive&&final.session.account!==other.session.account);
  game.check('portal round trips preserve character identity, equipment, XP and inventory revision',final.session.character===initial.session.character&&final.session.account===initial.session.account&&JSON.stringify(final.character.items)===JSON.stringify(initial.character.items)&&final.character.experience===initial.character.experience&&final.character.inventory_revision===initial.character.inventory_revision);
}
