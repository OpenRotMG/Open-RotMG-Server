import assert from 'node:assert/strict';
import {readFile, writeFile} from 'node:fs/promises';
import {join} from 'node:path';
const out = process.env.QA_RESULTS_DIR;
export const realmSeed = 1296;
export async function prepare(api) {
  const world = await api({op:'world_info', world:1});
  const events = (await readFile(join(out,'native/server.log'),'utf8')).split('\n')
    .filter(line=>line.startsWith('{')).map(line=>JSON.parse(line).fields);
  const end = events.findLastIndex(e=>e.message==='Realm simulation populated'&&e.layout===world.realm_layout);
  assert(end>=0, 'Ordinary Realm initialization must complete');
  const start = events.slice(0,end).findLastIndex(e=>e.message==='Realm simulation populated');
  const current = events.slice(start+1,end);
  const piece = current.find(e=>e.message==='named Realm boss setpiece placed'&&e.kind===0x928);
  assert(piece, 'Ordinary population must create a generated Ghost King tower');
  const root = current.find(e=>e.message==='finite named Realm quest placed'&&e.kind===0x928&&e.x>=piece.x&&e.x<piece.x+piece.width&&e.y>=piece.y&&e.y<piece.y+piece.height);
  assert(root, 'Generated tower must have an ordinary quest root');
  await writeFile(join(out,'generated-tower.json'),JSON.stringify({world,piece,root},null,2));
  return {class:0x31e,level:20,seed:1296,world:1,position:{x:root.x,y:root.y+2},
    equipment:[0xb0b,0xc0f,0xafc,0xacd],backpack:Array(8).fill(-1),stats:[770,252,50,40,50,50,75,50]};
}
export async function run(game) {
  const {root,piece} = JSON.parse(await readFile(join(out,'generated-tower.json'),'utf8'));
  game.check('generated King home is centered without changing the tower bounds',root.x===piece.x+13&&root.y===piece.y+13);
  const records = [];
  const entity = (state,id)=>state.diagnostic.entities.find(e=>e.id===id);
  async function save(label) {
    const state = await game.inspect();
    records.push({label,state});
    await writeFile(join(out,'encounter.json'),JSON.stringify(records));
    return state;
  }
  async function approach(point) {
    for(let n=0;n<50;n++) {
      const view = await game.observe(), dx = point.x-view.position.x, dy = point.y-view.position.y;
      if(Math.hypot(dx,dy)<1.4)return;
      const x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2,keys=[];
      if(Math.abs(x)>=Math.abs(y)*.4142)keys.push(x>0?'d':'a');
      if(Math.abs(y)>=Math.abs(x)*.4142)keys.push(y>0?'s':'w');
      await game.act({keys,duration_ms:100});
    }
    throw Error('Ordinary walking did not reach the observed ghost');
  }
  await game.act({keys:[],duration_ms:500},{name:'generated-tower-idle',when:()=>true});
  let state;
  for(let n=0;n<50;n++) {
    state = await game.inspect();
    const king = entity(state,root.entity);
    assert(king?.stage===0&&king.hp===300000, 'Untouched ordinary King required');
    if(king.pos.x>=root.x+1.5)break;
    await game.act({keys:[],duration_ms:250});
  }
  state = await save('eastern-idle');
  game.check('ordinary King paces east inside its generated tower',entity(state,root.entity).pos.x>=root.x+1.5);
  await approach(entity(state,root.entity).pos);
  for(let n=0;n<12&&entity(await game.inspect(),root.entity).stage===0;n++)
    await game.act({keys:[],duration_ms:100,target:{entity:root.entity,camera_angle:-Math.PI/4,fire:true}});
  state = await save('evaluation-start');
  game.check('ordinary sword fire starts King evaluation',entity(state,root.entity).stage===1);
  await game.act({keys:[],duration_ms:10000},{name:'generated-tower-wave',when:()=>true});
  state = await save('wave-landed');
  const king = entity(state,root.entity);
  const masters = state.diagnostic.entities.filter(e=>e.kind===0x929&&e.quest===king.quest);
  game.check('generated tower admits the required owned Ghost Master',masters.length===1);
  const master = masters[0];
  await approach(master.pos);
  for(let n=0;n<40;n++) {
    const current = await game.inspect();
    if(!entity(current,root.entity))break;
    const target = entity(current,master.id);
    assert(target, 'Required Master must remain present until release');
    await approach(target.pos);
    await game.act({keys:[],duration_ms:1000,target:{entity:master.id,camera_angle:-Math.PI/4,fire:true}});
  }
  state = await save('master-release');
  game.check('ordinary Master damage releases the same encounter before timeout',
    !entity(state,root.entity)&&state.diagnostic.entities.some(e=>e.kind===0x92d&&e.quest===king.quest)&&state.diagnostic.time_ms-king.activated<140000);
  game.check('same Knight survives ordinary combat without health or phase overrides',state.view.lifecycle.alive);
  await game.act({keys:[],duration_ms:500},{name:'master-released-king',when:()=>true});
}
