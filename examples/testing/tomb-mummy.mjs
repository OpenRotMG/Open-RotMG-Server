import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {setTimeout as delay} from 'node:timers/promises';
import {join} from 'node:path';
import {connect} from '../../scripts/record-demo.mjs';

// The ordinary shield counters live Scarabs while native rendering and pause acknowledgements complete.
export const fixture={class:0x31e,level:20,seed:12351,world:'tomb-of-the-ancients',worldSeed:12,position(info){assert.equal(String(info.prepared_seed),'12');return{x:147.5,y:18.5};},equipment:[0xb0b,0xb22,0xafc,0xacd],backpack:Array(8).fill(-1),stats:[770,252,50,40,50,50,75,50]};
export async function run(game){
  const out=process.env.QA_RESULTS_DIR,config=JSON.parse(await readFile(join(out,'native/native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim(),initial=await game.inspect();
  const map=async()=>{const r=await fetch(config.origin+'/dev/map?player='+initial.diagnostic.player,{headers:{Authorization:'Bearer '+token},signal:AbortSignal.timeout(10000)});assert(r.ok,'Read-only Tomb map failed');return r.json();};
  const layout=await map();await writeFile(join(out,'mummy-initial-map.json'),JSON.stringify(layout));
  const distance=e=>Math.hypot(e.pos.x-initial.view.position.x,e.pos.y-initial.view.position.y);
  const roots=layout.entities.filter(e=>e.kind===0xd1a&&e.hp===3000).sort((a,b)=>distance(a)-distance(b));
  const mummy=roots[0],aim=roots.find(e=>distance(e)>=3&&distance(e)<=5&&initial.view.objects.some(o=>o.id===e.id&&o.hp>0));
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
  const browser=config.native.find(b=>b.actor===game.actor);assert(browser,'Owned native browser missing');
  const connection=await connect({...browser,url:config.origin+'/',helper:config.browserTools});
  try{
    const page=await connection.eval('(()=>{const p=document.querySelector("ruffle-player"),r=p.getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height,viewportWidth:innerWidth,viewportHeight:innerHeight,playing:p.ruffle().isPlaying};})()');
    assert(page.playing===false&&initial.paused===true&&page.width===800&&page.height===600&&page.x>=0&&page.y>=0&&page.x+800<=page.viewportWidth&&page.y+600<=page.viewportHeight,'Paused native aim geometry changed');
    const target=initial.view.objects.find(o=>o.id===aim.id);assert.deepEqual(target.position,aim.pos,'Native shield aim differs from the paused map');
    const dx=target.position.x-initial.view.position.x,dy=target.position.y-initial.view.position.y,a=-Math.PI/4,x=50*(dx*Math.cos(a)+dy*Math.sin(a)),y=50*(-dx*Math.sin(a)+dy*Math.cos(a)),scale=Math.max(1,Math.hypot(x,y)/270);
    const pointer={type:'mouseMoved',buttons:0,x:page.x+300+x/scale,y:page.y+325+y/scale},started=performance.now();
    await connection.call('Input.dispatchMouseEvent',pointer);
    const elapsedMs=performance.now()-started,after=await game.inspect(),playing=await connection.eval('document.querySelector("ruffle-player").ruffle().isPlaying');
    await writeFile(join(out,'mummy-paused-aim.json'),JSON.stringify({page,pointer,elapsedMs,before:initial,after,playing},null,2));
    assert(playing===false&&after.paused===true&&after.view.time_ms===initial.view.time_ms&&after.view.character.hp===initial.view.character.hp&&after.view.character.mp===initial.view.character.mp&&after.diagnostic.character.shots===0&&JSON.stringify(after.view.position)===JSON.stringify(initial.view.position),'Paused hover advanced or changed gameplay');
  }finally{connection.close();}
  let complete=false;
  const action=game.act({keys:[' '],duration_ms:700},{name:'tomb-mummy-proximity-burst',when:v=>v.world==='Tomb of the Ancients'&&v.lifecycle.alive&&v.game_ms>=initial.view.time_ms+150}).finally(()=>{complete=true;});
  const monitor=(async()=>{for(let n=0;n<20&&!complete;n++){await sample('during-proximity-and-ordinary-shield');await delay(100);}})();
  const settled=await Promise.allSettled([action,monitor]),errors=settled.filter(r=>r.status==='rejected').map(r=>r.reason);
  if(errors.length)throw new AggregateError(errors,'Mummy native action or bounded observation failed');
  const final=await sample('after-ordinary-shield');
  const owned=row=>row.entities.filter(e=>e.kind===0xd1b&&e.tomb?.producer===mummy.id);
  const natural=records.find(row=>row.state.diagnostic.character.shots===0&&row.after.verification.shots===0&&!row.entities.some(e=>e.id===mummy.id)&&owned(row).length>=10&&owned(row).length<=13);
  game.check('natural Mummy suicide precedes native damage and leaves both owned Scarab waves',Boolean(natural));
  game.check('the native Colossus Shield emits its five shots without a primary attack',final.state.diagnostic.character.shots===5&&final.state.view.character.mp<initial.view.character.mp);
  game.check('the prepared Knight survives the live room without protection or a Tomb clear',final.state.view.lifecycle.alive&&final.state.diagnostic.character.tombs===0);
  await game.act({keys:['Insert'],duration_ms:300});
  const returned=await game.inspect();
  game.check('the same living character escapes with no Tomb completion',returned.view.map.name==='Nexus'&&returned.view.session.character===initial.view.session.character&&returned.view.session.account===initial.view.session.account&&returned.view.lifecycle.alive&&returned.diagnostic.character.tombs===0);
  await writeFile(join(out,'mummy-scope.json'),JSON.stringify({suppliedGearAndPosition:true,readOnlyPlanning:true,nativeShield:true,npcEdits:false,protection:false,reinforcements:false,proximityRoot:mummy.id,aimRoot:aim.id,ordinaryWave:5,additionalScarabs:owned(natural).length-5,limits:'Prepared Mummy proximity component, not earned traversal, all phases, historical cadence authentication, or a full Tomb clear.'},null,2));
}
