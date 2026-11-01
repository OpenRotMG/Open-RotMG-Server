import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {setTimeout as delay} from 'node:timers/promises';
import {join} from 'node:path';

// The ordinary shield counters live Scarabs while native rendering and pause acknowledgements complete.
export const fixture={class:0x31e,level:20,seed:12351,world:'tomb-of-the-ancients',worldSeed:12,position(info){assert.equal(String(info.prepared_seed),'12');return{x:147.5,y:18.5};},equipment:[0xb0b,0xb22,0xafc,0xacd],backpack:Array(8).fill(-1),stats:[770,252,50,40,50,50,75,50]};
export async function run(game){
  const out=process.env.QA_RESULTS_DIR,config=JSON.parse(await readFile(join(out,'native/native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim(),initial=await game.inspect();
  const map=async()=>{const r=await fetch(config.origin+'/dev/map?player='+initial.diagnostic.player,{headers:{Authorization:'Bearer '+token},signal:AbortSignal.timeout(10000)});assert(r.ok,'Read-only Tomb map failed');return r.json();};
  const layout=await map();await writeFile(join(out,'mummy-initial-map.json'),JSON.stringify(layout));
  const distance=e=>Math.hypot(e.pos.x-initial.view.position.x,e.pos.y-initial.view.position.y);
  const roots=layout.entities.filter(e=>e.kind===0xd1a&&e.hp===3000).sort((a,b)=>distance(a)-distance(b));
  const mummy=roots[0],aim=roots.find(e=>distance(e)>=3&&distance(e)<=5&&initial.view.objects.some(o=>o.id===e.id));
  assert(mummy&&distance(mummy)<2,'Prepared position must actually trigger a generated Mummy');
  assert(aim,'A different living Mummy must provide the ordinary shield aim');
  assert.equal(initial.view.character.conditions,0);assert.equal(initial.diagnostic.character.shots,0);
  const records=[];
  async function sample(label){
    const state=await game.inspect(),snapshot=await map(),after=await game.observe();
    const row={label,state,after,entities:snapshot.entities.filter(e=>e.kind===0xd1a||e.kind===0xd1b)};
    records.push(row);await writeFile(join(out,'mummy-lifecycle.json'),JSON.stringify(records));return row;
  }
  await sample('prepared-before-proximity');
  let complete=false;
  const action=game.act({keys:[' '],duration_ms:700,target:{entity:aim.id,camera_angle:-Math.PI/4,fire:false}},{name:'tomb-mummy-proximity-burst',when:v=>v.world==='Tomb of the Ancients'&&v.lifecycle.alive&&v.game_ms>=initial.view.time_ms+150}).finally(()=>{complete=true;});
  const monitor=(async()=>{for(let n=0;n<20&&!complete;n++){await sample('during-proximity-and-ordinary-shield');await delay(100);}})();
  const settled=await Promise.allSettled([action,monitor]),errors=settled.filter(r=>r.status==='rejected').map(r=>r.reason);
  if(errors.length)throw new AggregateError(errors,'Mummy native action or bounded observation failed');
  const final=await sample('after-ordinary-shield');
  const owned=row=>row.entities.filter(e=>e.kind===0xd1b&&e.tomb?.producer===mummy.id);
  const natural=records.find(row=>row.state.diagnostic.character.shots===0&&row.after.verification.shots===0&&!row.entities.some(e=>e.id===mummy.id)&&owned(row).length>=10&&owned(row).length<=13&&row.state.view.projectiles.some(p=>p.owner===mummy.id));
  game.check('natural Mummy suicide precedes native damage and leaves both owned Scarab waves plus live burst projectiles',Boolean(natural));
  game.check('the native Colossus Shield emits its five shots without a primary attack',final.state.diagnostic.character.shots===5&&final.state.view.character.mp<initial.view.character.mp);
  game.check('the prepared Knight survives the live room without protection or a Tomb clear',final.state.view.lifecycle.alive&&final.state.diagnostic.character.tombs===0);
  await game.act({keys:['Insert'],duration_ms:300});
  const returned=await game.inspect();
  game.check('the same living character escapes with no Tomb completion',returned.view.map.name==='Nexus'&&returned.view.session.character===initial.view.session.character&&returned.view.session.account===initial.view.session.account&&returned.view.lifecycle.alive&&returned.diagnostic.character.tombs===0);
  await writeFile(join(out,'mummy-scope.json'),JSON.stringify({suppliedGearAndPosition:true,readOnlyPlanning:true,nativeShield:true,npcEdits:false,protection:false,reinforcements:false,proximityRoot:mummy.id,aimRoot:aim.id,ordinaryWave:5,additionalScarabs:owned(natural).length-5,limits:'Prepared Mummy proximity component, not earned traversal, all phases, historical cadence authentication, or a full Tomb clear.'},null,2));
}
