import assert from 'node:assert/strict';
import {writeFile} from 'node:fs/promises';
import {join} from 'node:path';

export const fixture={class:0x31e,level:20,seed:12351,world:'tomb-of-the-ancients',worldSeed:12,position:{x:16.5,y:31.5},equipment:[0xb0b,0xb22,0xafc,0xacd],backpack:Array(8).fill(-1),stats:[770,252,50,40,50,50,75,50]};
export async function run(game){
  const records={scope:'Supplied legal maxed Knight/gear and initial corridor position. Genuine approach and bounded Fire Turret attack observation; no player attacks, consumables, protection, relocation or NPC/HP/RNG changes. Not earned full traversal.',phases:[]};
  const save=async label=>{const state=await game.inspect();records.phases.push({label,state});await writeFile(join(process.env.QA_RESULTS_DIR,'turret-approach.json'),JSON.stringify(records,null,2));return state;};
  const before=await save('prepared-corridor');
  assert(before.paused&&before.view.time_ms===0&&before.view.map.name==='Tomb of the Ancients');
  assert.deepEqual(before.view.position,fixture.position);
  // A wall-clock input hold does not establish how far the live client actually moved.
  async function walk(point){
    for(let n=0;n<25;n++){
      const v=await game.observe(),dx=point.x-v.position.x,dy=point.y-v.position.y;
      assert(v.world==='Tomb of the Ancients'&&v.lifecycle.alive,'Approach left its living Tomb scope');
      if(v.character.hp<300){await game.act({keys:['Insert'],duration_ms:300});throw Error('Approach aborted before observation: HP '+v.character.hp);}
      if(Math.hypot(dx,dy)<0.7)return;
      const x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2,keys=[];
      if(Math.abs(x)>=Math.abs(y)*0.4142)keys.push(x>0?'d':'a');
      if(Math.abs(y)>=Math.abs(x)*0.4142)keys.push(y>0?'s':'w');
      await game.act({keys,duration_ms:Math.max(50,Math.min(400,Math.round(Math.hypot(dx,dy)*100)))});
      await save('native-walk-'+point.x+'-'+point.y+'-'+n);
    }
    throw Error('Observed native approach did not reach '+JSON.stringify(point));
  }
  await walk({x:16.5,y:40.5});await walk({x:14.5,y:43.5});
  const arrived=await save('fire-observation-start'),start=arrived.view.time_ms;
  const turret=arrived.view.objects.find(o=>o.kind===0xd31&&o.hp===0);
  assert(turret&&Math.hypot(arrived.view.position.x-turret.position.x,arrived.view.position.y-turret.position.y)<4.9,'Fire Turret not in native range');
  await game.act({keys:[],duration_ms:700},{name:'tomb-fire-armor-break',when:v=>v.world==='Tomb of the Ancients'&&v.lifecycle.alive&&v.game_ms>=start+300&&(v.character.conditions&(1<<26))!==0});
  const attacked=await save('ordinary-fire-attack');
  await game.act({keys:['Insert'],duration_ms:300});const after=await save('ordinary-nexus-escape');
  game.check('native movement reaches the generated Fire Turret',Math.hypot(arrived.view.position.x-14.5,arrived.view.position.y-43.5)<0.7);
  game.check('ordinary incoming fire actually inflicts Armor Broken and damage',(attacked.view.character.conditions&(1<<26))!==0&&attacked.view.character.hp<arrived.view.character.hp);
  game.check('no primary shots or ability use replaced ordinary movement',records.phases.every(p=>p.state.diagnostic.character.shots===0&&p.state.view.character.mp===before.view.character.mp));
  game.check('the same living Knight returns without kills or Tomb credit',after.view.map.name==='Nexus'&&after.view.lifecycle.alive&&after.view.session.character===before.view.session.character&&after.diagnostic.character.kills===0&&after.diagnostic.character.tombs===0);
}
