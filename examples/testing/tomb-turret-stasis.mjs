import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {setTimeout as delay} from 'node:timers/promises';
import {connect} from '../../scripts/record-demo.mjs';

const turret={x:21.5,y:47.5},offset=3.8/Math.SQRT2;
export const fixture={class:0x323,level:20,seed:12351,world:'tomb-of-the-ancients',worldSeed:12,position:{x:turret.x+offset,y:turret.y+offset},equipment:[-1,0xb2d,0xb05,0xac8],backpack:Array(8).fill(-1),stats:[670,385,60,25,60,50,40,75]};
export async function run(game){
  const out=process.env.QA_RESULTS_DIR,config=JSON.parse(await readFile(join(out,'native/native.json'),'utf8')),before=await game.inspect();
  assert(before.paused&&before.view.time_ms===0&&before.view.map.name==='Tomb of the Ancients','Fresh paused Tomb required');
  const target=before.view.objects.find(o=>o.kind===0xd33&&o.position.x===turret.x&&o.position.y===turret.y);
  assert(target&&target.hp===0,'Generated zero-HP ordinary Frost Turret must be client-visible');
  assert(Math.hypot(before.view.position.x-fixture.position.x,before.view.position.y-fixture.position.y)<0.0001,'Prepared caster position changed');
  const records={scope:'Supplied legal maxed Mystic, Planefetter Orb, robe and defense ring at generated ordinary Frost Turret. Read-only map/aim planning; no primary weapon, consumables, protection, NPC/HP/RNG changes or relocation. Not earned traversal or a complete Tomb.',before,target,samples:[]};
  const browser=config.native.find(b=>b.actor===game.actor);assert(browser,'Exact native browser missing');
  const connection=await connect({...browser,url:config.origin+'/',helper:config.browserTools});
  try{
    const page=await connection.eval('(()=>{const p=document.querySelector("ruffle-player"),r=p.getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height,vw:innerWidth,vh:innerHeight,playing:p.ruffle().isPlaying};})()');
    assert(page.playing===false&&page.width===800&&page.height===600&&page.x>=0&&page.y>=0&&page.x+800<=page.vw&&page.y+600<=page.vh,'Paused native geometry changed');
    const a=-Math.PI/4,dx=turret.x-before.view.position.x,dy=turret.y-before.view.position.y;
    const pointer={type:'mouseMoved',buttons:0,x:page.x+300+50*(dx*Math.cos(a)+dy*Math.sin(a)),y:page.y+325+50*(-dx*Math.sin(a)+dy*Math.cos(a))};
    assert(pointer.x>=page.x&&pointer.x<page.x+600&&pointer.y>=page.y&&pointer.y<page.y+600,'Aim leaves gameplay canvas');
    const started=performance.now();await connection.call('Input.dispatchMouseEvent',pointer);
    const after=await game.inspect(),playing=await connection.eval('document.querySelector("ruffle-player").ruffle().isPlaying');
    records.aim={page,pointer,elapsed_ms:performance.now()-started,after,playing};
    assert(playing===false&&after.paused&&after.view.time_ms===before.view.time_ms&&after.view.character.hp===before.view.character.hp&&after.view.character.mp===before.view.character.mp&&after.diagnostic.character.shots===0&&JSON.stringify(after.view.position)===JSON.stringify(before.view.position),'Paused hover changed gameplay');
  }finally{connection.close();await writeFile(join(out,'turret-stasis.json'),JSON.stringify(records,null,2));}
  let complete=false;
  const action=game.act([{actor:game.actor,input:{keys:[' '],duration_ms:350}},{actor:game.actor,input:{keys:['Insert'],duration_ms:300}}],{name:'tomb-ordinary-turret-stasis',when:v=>v.world==='Tomb of the Ancients'&&v.lifecycle.alive&&v.character.mp<before.view.character.mp}).finally(()=>{complete=true;});
  const monitor=(async()=>{
    for(let n=0;n<160&&!complete;n++){
      const s=await game.inspect();
      if(s.view.map.name!=='Tomb of the Ancients')break;
      const npc=s.view.objects.find(e=>e.id===target.id);assert(npc,'Observed turret disappeared');
      records.samples.push({game_ms:s.view.time_ms,paused:s.paused,hp:s.view.character.hp,mp:s.view.character.mp,position:s.view.position,nativeShots:s.diagnostic.character.shots,target:npc});
      await writeFile(join(out,'turret-stasis.json'),JSON.stringify(records,null,2));await delay(100);
    }
  })();
  const settled=await Promise.allSettled([action,monitor]),errors=settled.filter(r=>r.status==='rejected').map(r=>r.reason);
  records.outcomes=settled.map(r=>r.status==='fulfilled'?{status:r.status}:{status:r.status,error:String(r.reason)});
  try{records.after=await game.inspect();}catch(error){records.inspectError=String(error);errors.push(error);}
  await writeFile(join(out,'turret-stasis.json'),JSON.stringify(records,null,2));
  if(errors.length)throw new AggregateError(errors,'Native turret observation or ordinary escape failed');
  const after=records.after;
  game.check('the orb activates without firing a primary weapon',records.samples.some(s=>s.mp<before.view.character.mp)&&records.samples.every(s=>s.nativeShots===0));
  game.check('the same living Mystic escapes without Tomb credit',after.view.map.name==='Nexus'&&after.view.lifecycle.alive&&after.view.session.character===before.view.session.character&&after.diagnostic.character.tombs===0);
  game.check('ordinary zero-HP Tomb Frost Turret actually receives Stasis',records.samples.some(s=>(s.target.condition_mask&(1<<21))!==0));
}
