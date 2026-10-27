import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';

const root=process.env.QA_RESULTS_DIR;
export const fixture={class:0x310,level:20,seed:791,world:-3,position:'spawn',equipment:[0xaf6,0xb25,0xb05,0xac5],backpack:[0x70b,0x70b,-1,-1,-1,-1,-1,-1],stats:[670,385,50,25,50,50,40,75]};
export async function prepare(){
  const native=join(root,'native'),{origin}=JSON.parse(await readFile(join(native,'result.json'),'utf8'));
  const token=(await readFile(join(native,'state','control.token'),'utf8')).trim();
  const response=await fetch(origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:'travel-capacity',command:{op:'prepare_realm_capacity',limit:1}}),signal:AbortSignal.timeout(10000)});
  const receipt=await response.json();assert(response.ok&&receipt.status!=='failed',JSON.stringify(receipt));
  assert.equal(receipt.result.world,fixture.world);
  await writeFile(join(root,'capacity.json'),JSON.stringify(receipt));
  return fixture;
}
export async function run(game){
  const observations=[];
  async function save(name,actor=game){const view=await actor.observe();observations.push({name,view});await writeFile(join(root,'travel.json'),JSON.stringify(observations,null,2));return view;}
  async function enter(actor,from,name){
    await actor.act({keys:[],mouse:{x:700,y:575},duration_ms:700});
    for(let i=0;i<8&&(await actor.observe()).session.world===from;i++)await actor.act({keys:[],duration_ms:500});
    const view=await save(name,actor);assert.notEqual(view.session.world,from,'Portal failed to transfer: '+name);return view;
  }
  const initial=await save('realm'),home=initial.session.world;
  await game.act({keys:['1'],duration_ms:700});
  const keyed=await save('first-key');
  assert.equal(keyed.character.items[4],-1);
  const first=await enter(game,home,'first-dungeon');assert.equal(first.world,'Snake Pit');
  await game.act({keys:['2'],duration_ms:700});
  const nested=await enter(game,first.session.world,'nested-dungeon');assert.equal(nested.world,'Snake Pit');
  assert.notEqual(nested.session.world,first.session.world);
  // Origin inheritance and full-Realm refusal are project policy, not recovered 2012 semantics: https://github.com/OpenRotMG/Open-RotMG-Server/pull/438
  const returned=await enter(game,nested.session.world,'nested-return');assert.equal(returned.session.world,home);
  const again=await enter(game,home,'reused-entrance');assert.equal(again.session.world,first.session.world);
  const other=await game.join({...fixture,seed:792,backpack:Array(8).fill(-1)});
  const occupant=await save('other-occupant',other);assert.equal(occupant.session.world,home);
  await game.act({keys:[],mouse:{x:700,y:575},duration_ms:1000});
  const denied=await save('full-return-denied');
  game.check('full origin refuses entry without replacing the character or inventory',denied.session.world===first.session.world&&denied.session.character===initial.session.character&&denied.lifecycle.alive&&JSON.stringify(denied.character.items)===JSON.stringify(again.character.items)&&denied.character.inventory_revision===again.character.inventory_revision);
  await game.act({keys:['w'],duration_ms:300});
  const moved=await save('denied-source-usable');
  game.check('denied transfer leaves usable movement in the source',Math.hypot(moved.position.x-denied.position.x,moved.position.y-denied.position.y)>0.1);
  await game.act({keys:['s'],duration_ms:300});
  await other.act({keys:['r'],duration_ms:1000});
  const escaped=await save('other-nexus',other);assert.equal(escaped.world,'Nexus');
  const admitted=await enter(game,first.session.world,'return-after-slot-freed');assert.equal(admitted.session.world,home);
  await game.act({keys:['r'],duration_ms:1000});
  const nexus=await save('nexus');assert.equal(nexus.world,'Nexus');
  await game.act({keys:['r'],duration_ms:700},{name:'nexus-arrival',when:v=>v.world==='Nexus'&&v.lifecycle.alive});
  const redundant=await save('redundant-escape');
  game.check('redundant Nexus escape does not reconnect',redundant.session.player===nexus.session.player&&redundant.session.world===nexus.session.world);
  game.check('two distinct key instances return to the exact opening Realm',nested.session.world!==first.session.world&&returned.session.world===home&&admitted.session.world===home);
  game.check('keys are consumed once and never duplicated across transfers',redundant.character.items.slice(4).every(x=>x===-1)&&redundant.character.inventory_revision===initial.character.inventory_revision+2);
  game.check('Nexus escape retains the same living account character equipment and XP',redundant.lifecycle.alive&&redundant.session.account===initial.session.account&&redundant.session.character===initial.session.character&&JSON.stringify(redundant.character.items.slice(0,4))===JSON.stringify(initial.character.items.slice(0,4))&&redundant.character.experience===initial.character.experience);
  game.check('other account remains independent in Nexus',escaped.session.account!==nexus.session.account&&escaped.session.character!==nexus.session.character&&escaped.lifecycle.alive);
}
