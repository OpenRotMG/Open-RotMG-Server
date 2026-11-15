import assert from 'node:assert/strict';
import {writeFile} from 'node:fs/promises';
import {join} from 'node:path';

export const fixture={class:0x310,level:20,seed:791,world:-3,position:'spawn',equipment:[0xaf6,0xb25,0xb05,0xac5],backpack:[0x70b,0x70b,-1,-1,-1,-1,-1,-1],stats:[670,385,50,25,50,50,40,75]};
export async function run(game){
  const observations=[];
  async function save(name,actor=game){const view=await actor.observe();observations.push({name,view});await writeFile(join(process.env.QA_RESULTS_DIR,'travel.json'),JSON.stringify(observations,null,2));return view;}
  async function enter(from,name){
    await game.act({keys:[],mouse:{x:700,y:575},duration_ms:700});
    for(let i=0;i<8&&(await game.observe()).session.world===from;i++)await game.act({keys:[],duration_ms:500});
    const view=await save(name);assert.notEqual(view.session.world,from,'Portal failed to transfer: '+name);return view;
  }
  async function escape(name){
    await game.act({keys:['Insert'],duration_ms:700});
    for(let i=0;i<8&&(await game.observe()).world!=='Nexus';i++)await game.act({keys:[],duration_ms:500});
    const view=await save(name);assert.equal(view.world,'Nexus');return view;
  }
  const initial=await save('realm'),home=initial.session.world;
  await game.act({keys:['1'],duration_ms:700});
  const first=await enter(home,'first-dungeon');assert.equal(first.world,'Snake Pit');
  await game.act({keys:['d','s'],duration_ms:600});
  const inside=await save('away-from-cowardice');assert(Math.hypot(inside.position.x-first.position.x,inside.position.y-first.position.y)>2,'Nested key needs an unambiguous entrance panel');
  await game.act({keys:['2'],duration_ms:700});
  const nested=await enter(first.session.world,'nested-dungeon');assert.equal(nested.world,'Snake Pit');
  // Origin inheritance is project policy, not recovered 2012 semantics: https://github.com/OpenRotMG/Open-RotMG-Server/pull/438
  const admitted=await enter(nested.session.world,'nested-return');
  game.check('nested key instance returns to the exact opening Realm',nested.session.world!==first.session.world&&admitted.session.world===home);
  const nexus=await escape('nexus');
  await game.act({keys:[],duration_ms:700},{name:'nexus-arrival',when:v=>v.world==='Nexus'&&v.lifecycle.alive});
  game.check('keys are consumed once and never duplicated across transfers',nexus.character.items.slice(4).every(x=>x===-1)&&nexus.character.inventory_revision===initial.character.inventory_revision+2);
  game.check('Nexus escape retains the same living account character equipment and XP',nexus.lifecycle.alive&&nexus.session.account===initial.session.account&&nexus.session.character===initial.session.character&&JSON.stringify(nexus.character.items.slice(0,4))===JSON.stringify(initial.character.items.slice(0,4))&&nexus.character.experience===initial.character.experience);
  const other=await game.join({...fixture,world:-2,seed:792,backpack:Array(8).fill(-1)}),neighbor=await save('other-nexus',other);
  const portal=(await game.inspect()).diagnostic.entities.find(e=>e.kind===0x720);assert(portal,'Nexus Vault portal missing');
  for(let i=0;i<50;i++){
    const v=await game.observe(),dx=portal.pos.x-v.position.x,dy=portal.pos.y-v.position.y,distance=Math.hypot(dx,dy);
    if(distance<1)break;
    const x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2,keys=[];
    if(Math.abs(x)>.12)keys.push(x>0?'d':'a');
    if(Math.abs(y)>.12)keys.push(y>0?'s':'w');
    await game.act({keys,duration_ms:Math.min(300,Math.max(50,Math.round(distance*80)))});
  }
  const vault=await enter(nexus.session.world,'private-vault');assert.equal(vault.world,'Vault');
  const stayed=await other.observe();
  game.check('private Vault arrival does not move the other account',stayed.world==='Nexus'&&stayed.session.player===neighbor.session.player&&stayed.session.account!==vault.session.account);
  const final=await escape('vault-nexus');
  game.check('Vault escape preserves the same living character and exact inventory',final.lifecycle.alive&&final.session.character===initial.session.character&&JSON.stringify(final.character.items)===JSON.stringify(nexus.character.items)&&final.character.inventory_revision===nexus.character.inventory_revision);
}
