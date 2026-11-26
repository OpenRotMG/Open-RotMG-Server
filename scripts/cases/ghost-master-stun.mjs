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
  root=current.find(e=>e.message==='linked Realm site placed'&&e.kind===0x928&&e.x>=piece.x&&e.x<piece.x+piece.width&&e.y>=piece.y&&e.y<piece.y+piece.height);
  assert(root,'Ordinarily placed Ghost King required');
  await writeFile(join(out,'tower.json'),JSON.stringify({world,piece,root},null,2));
  return {class:0x31e,level:20,seed:1296,world:1,position:{x:root.x,y:root.y+2},equipment:[0xb0b,0xa08,0xafc,0xacd],backpack:Array(8).fill(-1),stats:[770,252,50,40,50,50,75,50]};
}
export async function run(game){
  const first=await game.observe(),records=[];
  const king=s=>s.diagnostic.entities.find(e=>e.id===root.entity);
  // Recovered Ghost King states (actors.xml:3800): Idle until HP<99.999%, invulnerable EvaluationStart1 2.5s, EvaluationStart2 up to 6s, then Solo tosses the Ghost Master.
  const phase=s=>(s.diagnostic.xml[root.entity]?.states??[]).map(x=>x.name).at(-1);
  async function inspect(label){
    const s=await game.inspect();
    assert(s.view.lifecycle.alive&&s.view.session.character===first.session.character&&s.view.session.account===first.session.account&&s.view.session.world===1,'Same living original Knight in same Realm required');
    records.push({label,state:s});await writeFile(join(out,'stun-observations.json'),JSON.stringify(records,null,2));return s;
  }
  async function approach(point,range=1.4){
    for(let n=0;n<50;n++){
      const s=await game.observe(),dx=point.x-s.position.x,dy=point.y-s.position.y;
      if(Math.hypot(dx,dy)<range)return;
      const x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2,keys=[];
      if(Math.abs(x)>=Math.abs(y)*.4142)keys.push(x>0?'d':'a');
      if(Math.abs(y)>=Math.abs(x)*.4142)keys.push(y>0?'s':'w');
      await game.act({keys,duration_ms:100});
    }
    throw Error('Ordinary walking did not reach the observed King');
  }
  let state=await inspect('original');assert(king(state)?.hp===300000&&[undefined,'Idle'].includes(phase(state)),'Untouched ordinary King required');
  await approach(king(state).pos);
  for(let n=0;n<12;n++){
    state=await game.inspect();if(phase(state)==='EvaluationStart1')break;
    assert(phase(state)==='Idle','Unexpected evaluation state');
    await game.act({keys:[],duration_ms:100,target:{entity:root.entity,camera_angle:-Math.PI/4,fire:true}});
  }
  state=await inspect('evaluation-start');assert.equal(phase(state),'EvaluationStart1','Native sword fire must begin evaluation');
  let second;
  for(let n=0;n<40;n++){
    state=await game.inspect();assert(king(state),'Original King disappeared');
    if(phase(state)==='EvaluationStart2')second??=state.diagnostic.time_ms;
    // The 3s Wooden Shield stun must span the 6s mark; first observation of EvaluationStart2 can lag under load.
    if(second!==undefined&&state.diagnostic.time_ms-second>=3500)break;
    assert(['EvaluationStart1','EvaluationStart2'].includes(phase(state)),'Master cast passed before stun');
    // Stay within the 3.2-tile Wooden Shield reach while the King wanders, so the stun can be cast at once.
    if(Math.hypot(king(state).pos.x-state.view.position.x,king(state).pos.y-state.view.position.y)>2.5)await approach(king(state).pos);
    else await game.act({keys:[],duration_ms:250});
  }
  state=await inspect('before-shield');assert.equal(phase(state),'EvaluationStart2');
  await approach(king(state).pos,3);
  state=await inspect('shield-aim');
  assert(phase(state)==='EvaluationStart2'&&state.diagnostic.time_ms-second<6000,'Actual evaluation window required');
  assert(state.view.character.items[1]===0xa08&&state.view.character.mp>=80,'Original Wooden Shield and actual mana required');
  const due=second+6000;
  await game.act({keys:[' '],duration_ms:300,target:{entity:root.entity,camera_angle:-Math.PI/4,fire:false}});
  state=await inspect('actual-native-stun');const stunned=king(state);
  assert(stunned&&Object.hasOwn(stunned.statuses,'7')&&stunned.statuses['7'][1]>due,'Accepted native stun must span the actual Master cast');
  assert(state.view.character.mp<records.find(r=>r.label==='shield-aim').state.view.character.mp-65,'Shield must spend actual mana');
  await game.act({keys:[],duration_ms:7000},{name:'stunned-master-wave',when:s=>s.session.character===first.session.character&&s.session.world===1&&s.lifecycle.alive&&s.game_ms>=due+2500});
  state=await inspect('wave-after-stun');const original=king(state);assert(original,'Original King must remain pending release');
  const masters=state.diagnostic.entities.filter(e=>e.kind===0x929&&(e.parent===original.id||e.quest===original.quest));
  await writeFile(join(out,'master-result.json'),JSON.stringify({preparedFixture:true,realmSeed,root,original:first.session,actualStun:stunned.statuses['7'],castDue:due,masters:masters.map(e=>({id:e.id,kind:e.kind,quest:e.quest,hp:e.hp,pos:e.pos})),sameLivingCharacter:true,scope:'Native one-player stun/Master birth only; not earned progression, all waves, whole Realm closure, historical fidelity or original reporter causation'},null,2));
  // Auto ObjectToss never checks STUNNED in recovered behavior.cc:2352-2400, so the Solo toss lands 1.5s later even while stunned.
  game.check('stunning the King does not discard the required Ghost Master',masters.length===1&&masters[0].activated-1500>=stunned.statuses['7'][0]&&masters[0].activated-1500<=stunned.statuses['7'][1]+100);
}
