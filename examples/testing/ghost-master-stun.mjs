import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
export const realmSeed=1296;
const out=process.env.QA_RESULTS_DIR;
let root;
export async function prepare(api){
  const world=await api({op:'world_info',world:1});
  const events=(await readFile(join(out,'native/server.log'),'utf8')).split('\n').filter(s=>s.startsWith('{')).map(s=>JSON.parse(s).fields);
  const end=events.findLastIndex(e=>e.message==='Realm simulation populated'&&e.layout===world.realm_layout);
  assert(end>=0,'Completed ordinary Realm initialization required');
  const start=events.slice(0,end).findLastIndex(e=>e.message==='Realm simulation populated');
  const current=events.slice(start+1,end),piece=current.find(e=>e.message==='named Realm boss setpiece placed'&&e.kind===0x928);
  assert(piece,'Generated Ghost King tower required');
  root=current.find(e=>e.message==='finite named Realm quest placed'&&e.kind===0x928&&e.x>=piece.x&&e.x<piece.x+piece.width&&e.y>=piece.y&&e.y<piece.y+piece.height);
  assert(root,'Ordinarily placed Ghost King required');
  await writeFile(join(out,'tower.json'),JSON.stringify({world,piece,root},null,2));
  return {class:0x31e,level:20,seed:1296,world:1,position:{x:root.x,y:root.y+2},equipment:[0xb0b,0xa08,0xafc,0xacd],backpack:Array(8).fill(-1),stats:[770,252,50,40,50,50,75,50]};
}
export async function run(game){
  const first=await game.observe(),records=[];
  const king=s=>s.diagnostic.entities.find(e=>e.id===root.entity);
  async function inspect(label){
    const s=await game.inspect();
    assert(s.view.lifecycle.alive&&s.view.session.character===first.session.character&&s.view.session.account===first.session.account&&s.view.session.world===1,'Same living original Knight in same Realm required');
    records.push({label,state:s});await writeFile(join(out,'stun-observations.json'),JSON.stringify(records,null,2));return s;
  }
  async function approach(point){
    for(let n=0;n<50;n++){
      const s=await game.observe(),dx=point.x-s.position.x,dy=point.y-s.position.y;
      if(Math.hypot(dx,dy)<1.4)return;
      const x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2,keys=[];
      if(Math.abs(x)>=Math.abs(y)*.4142)keys.push(x>0?'d':'a');
      if(Math.abs(y)>=Math.abs(x)*.4142)keys.push(y>0?'s':'w');
      await game.act({keys,duration_ms:100});
    }
    throw Error('Ordinary walking did not reach the observed King');
  }
  let state=await inspect('original');assert(king(state)?.hp===300000&&king(state).stage===0,'Untouched ordinary King required');
  await approach(king(state).pos);
  for(let n=0;n<12;n++){
    state=await game.inspect();if(king(state)?.stage===1)break;
    assert(king(state)?.stage===0,'Unexpected evaluation state');
    await game.act({keys:[],duration_ms:100,target:{entity:root.entity,camera_angle:-Math.PI/4,fire:true}});
  }
  state=await inspect('evaluation-start');assert.equal(king(state)?.stage,1,'Native sword fire must begin evaluation');
  for(let n=0;n<32;n++){
    state=await game.inspect();const e=king(state);assert(e,'Original King disappeared');
    if(e.stage===2&&state.diagnostic.time_ms-e.activated>=4500)break;
    assert(e.stage===1||e.stage===2,'Master cast passed before stun');
    await game.act({keys:[],duration_ms:250});
  }
  state=await inspect('before-shield');assert.equal(king(state)?.stage,2);
  await approach(king(state).pos);
  state=await inspect('shield-aim');const e=king(state);
  assert(e.stage===2&&state.diagnostic.time_ms-e.activated>=4500&&state.diagnostic.time_ms-e.activated<6000,'Actual evaluation window required');
  assert(state.view.character.items[1]===0xa08&&state.view.character.mp>=80,'Original Wooden Shield and actual mana required');
  const due=e.activated+6000;
  await game.act({keys:[' '],duration_ms:300,target:{entity:root.entity,camera_angle:-Math.PI/4,fire:false}});
  state=await inspect('actual-native-stun');const stunned=king(state);
  assert(stunned&&Object.hasOwn(stunned.statuses,'7')&&stunned.statuses['7'][1]>due,'Accepted native stun must span the actual Master cast');
  assert(state.view.character.mp<records.find(r=>r.label==='shield-aim').state.view.character.mp-65,'Shield must spend actual mana');
  await game.act({keys:[],duration_ms:7000},{name:'stunned-master-wave',when:s=>s.session.character===first.session.character&&s.session.world===1&&s.lifecycle.alive&&s.game_ms>=due+2500});
  state=await inspect('wave-after-stun');const original=king(state);assert(original,'Original King must remain pending release');
  const masters=state.diagnostic.entities.filter(e=>e.kind===0x929&&e.quest===original.quest);
  await writeFile(join(out,'master-result.json'),JSON.stringify({preparedFixture:true,realmSeed,root,original:first.session,actualStun:stunned.statuses['7'],castDue:due,masters:masters.map(e=>({id:e.id,kind:e.kind,quest:e.quest,hp:e.hp,pos:e.pos})),sameLivingCharacter:true,scope:'Native one-player stun/Master birth only; not earned progression, all waves, whole Realm closure, historical fidelity or original reporter causation'},null,2));
  game.check('stunning the King does not discard the required Ghost Master',masters.length===1);
}
