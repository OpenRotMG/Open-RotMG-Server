import assert from 'node:assert/strict';
import {writeFile} from 'node:fs/promises';
import {join} from 'node:path';

export const fixture={class:0x323,level:20,seed:12351,world:'tomb-of-the-ancients',worldSeed:12,position:{x:16.5,y:31.5},equipment:[0xb08,0xb2d,0xb05,0xac8],backpack:Array(8).fill(-1),stats:[670,385,60,25,20,50,40,75]};
export async function run(game){
  const out=process.env.QA_RESULTS_DIR,records={scope:'Supplied Mystic/gear and initial corridor position; genuine ordinary walk toward generated Fire Turret. No ability/primary fire, consumables, protection, relocation, NPC/HP/RNG changes or empty warm-up. Not earned full traversal.',phases:[]};
  const save=async label=>{const state=await game.inspect();records.phases.push({label,state});await writeFile(join(out,'turret-approach.json'),JSON.stringify(records,null,2));return state;};
  const before=await save('prepared-corridor');
  assert(before.paused&&before.view.time_ms===0&&before.view.map.name==='Tomb of the Ancients');
  assert.deepEqual(before.view.position,fixture.position);
  await game.act({keys:['a','s'],duration_ms:1500});
  const first=await save('ordinary-south-approach');
  assert(first.view.lifecycle.alive&&first.view.position.x>=15.5&&first.view.position.x<=17.5&&first.view.position.y>=34&&first.view.position.y<42,'Southward corridor approach left planned region');
  await game.act({keys:['a'],duration_ms:700});
  const second=await save('ordinary-side-approach');
  assert(second.view.lifecycle.alive&&second.view.position.x>=13&&second.view.position.x<=16.5&&second.view.position.y>=38&&second.view.position.y<43.5,'Side approach left planned region');
  await game.act([{actor:game.actor,input:{keys:['a','s'],duration_ms:300}},{actor:game.actor,input:{keys:['Insert'],duration_ms:300}}],{name:'tomb-edge-fire-turret',when:v=>v.world==='Tomb of the Ancients'&&v.lifecycle.alive&&v.game_ms>=3000&&v.position.x>=13&&v.position.x<=16.5&&v.position.y>=42.5&&v.position.y<=45});
  const after=await save('ordinary-nexus-escape');
  game.check('native movement approaches the generated turret room',first.view.position.y>before.view.position.y+2&&second.view.position.x<first.view.position.x);
  game.check('no primary shots or ability use were substituted for movement',records.phases.every(p=>p.state.diagnostic.character.shots===0&&p.state.view.character.mp===before.view.character.mp));
  game.check('the same living Mystic returns without kills or Tomb credit',after.view.map.name==='Nexus'&&after.view.lifecycle.alive&&after.view.session.character===before.view.session.character&&after.diagnostic.character.kills===0&&after.diagnostic.character.tombs===0);
}
