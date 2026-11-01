import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {setTimeout as delay} from 'node:timers/promises';
import {connect} from '../../scripts/record-demo.mjs';

export const fixture={class:0x323,level:20,seed:12351,world:'tomb-of-the-ancients',worldSeed:12,position:{x:15.5,y:39},equipment:[0xb08,0xb2d,0xb05,0xac8],backpack:Array(8).fill(-1),stats:[670,385,60,25,60,50,40,75]};
export async function run(game){
  const out=process.env.QA_RESULTS_DIR,records={scope:'Supplied legal maxed Mystic/gear and initial corridor position. Genuine approach and ordinary orb use, no primary attacks, consumables, protection, NPC/HP/RNG changes or relocation. Not earned entry/full traversal.',phases:[],samples:[]};
  const persist=()=>writeFile(join(out,'turret-stasis.json'),JSON.stringify(records,null,2));
  const save=async label=>{const state=await game.inspect();records.phases.push({label,state});await persist();return state;};
  const before=await save('prepared-corridor');assert(before.paused&&before.view.time_ms===0&&before.view.map.name==='Tomb of the Ancients');
  for(let n=0;n<25;n++){
    const v=await game.observe(),dx=14.5-v.position.x,dy=42.2-v.position.y;
    assert(v.world==='Tomb of the Ancients'&&v.lifecycle.alive,'Approach left living Tomb scope');
    if(v.character.hp<300){await game.act({keys:['Insert'],duration_ms:300});throw Error('Stasis approach aborted: HP '+v.character.hp);}
    if(Math.hypot(dx,dy)<0.2)break;
    assert(n<24,'Observed native Stasis approach did not arrive');
    const x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2,keys=[];
    if(Math.abs(x)>=Math.abs(y)*0.4142)keys.push(x>0?'d':'a');if(Math.abs(y)>=Math.abs(x)*0.4142)keys.push(y>0?'s':'w');
    await game.act({keys,duration_ms:Math.max(50,Math.min(400,Math.round(Math.hypot(dx,dy)*100)))});await save('native-walk-'+n);
  }
  const ready=await save('before-orb'),fire=ready.view.objects.find(o=>o.kind===0xd31),thunder=ready.view.objects.find(o=>o.kind===0xd32);
  assert(fire&&thunder&&fire.hp===0&&thunder.hp===0,'Both generated indestructible edge turrets must be visible');
  assert(Math.hypot(ready.view.position.x-fire.position.x,ready.view.position.y-fire.position.y)>5.05,'Caster entered Fire range before Stasis');
  const aim={x:fire.position.x+1,y:fire.position.y};assert(Math.hypot(aim.x-thunder.position.x,aim.y-thunder.position.y)<3);
  const config=JSON.parse(await readFile(join(out,'native/native.json'),'utf8')),browser=config.native.find(b=>b.actor===game.actor);assert(browser,'Exact native browser missing');
  const connection=await connect({...browser,url:config.origin+'/',helper:config.browserTools});
  try{
    const page=await connection.eval('(()=>{const p=document.querySelector("ruffle-player"),r=p.getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height,vw:innerWidth,vh:innerHeight,playing:p.ruffle().isPlaying};})()');
    assert(page.playing===false&&page.width===800&&page.height===600&&page.x>=0&&page.y>=0&&page.x+800<=page.vw&&page.y+600<=page.vh,'Paused native geometry changed');
    const a=-Math.PI/4,dx=aim.x-ready.view.position.x,dy=aim.y-ready.view.position.y,pointer={type:'mouseMoved',buttons:0,x:page.x+300+50*(dx*Math.cos(a)+dy*Math.sin(a)),y:page.y+325+50*(-dx*Math.sin(a)+dy*Math.cos(a))};
    assert(pointer.x>=page.x&&pointer.x<page.x+600&&pointer.y>=page.y&&pointer.y<page.y+600,'Stasis aim leaves gameplay canvas');
    await connection.call('Input.dispatchMouseEvent',pointer);const after=await game.inspect(),playing=await connection.eval('document.querySelector("ruffle-player").ruffle().isPlaying');records.aim={aim,page,pointer,after,playing};
    assert(after.paused&&playing===false&&after.view.time_ms===ready.view.time_ms&&after.view.character.hp===ready.view.character.hp&&after.view.character.mp===ready.view.character.mp&&JSON.stringify(after.view.position)===JSON.stringify(ready.view.position)&&after.diagnostic.character.shots===0,'Paused hover changed gameplay');
  }finally{connection.close();await persist();}
  let complete=false;
  const action=game.act({keys:[' '],duration_ms:350},{name:'tomb-edge-turrets-stasis',when:v=>v.world==='Tomb of the Ancients'&&v.lifecycle.alive&&v.character.mp<ready.view.character.mp}).finally(()=>{complete=true;});
  const monitor=(async()=>{for(let n=0;n<160&&!complete;n++){
    const s=await game.inspect();assert(s.view.map.name==='Tomb of the Ancients','Orb observation left Tomb');
    const targets=[fire.id,thunder.id].map(id=>s.view.objects.find(o=>o.id===id));assert(targets.every(Boolean),'Observed turret disappeared');
    records.samples.push({game_ms:s.view.time_ms,hp:s.view.character.hp,mp:s.view.character.mp,shots:s.diagnostic.character.shots,targets});await persist();await delay(100);
  }})();
  const settled=await Promise.allSettled([action,monitor]),errors=settled.filter(r=>r.status==='rejected').map(r=>r.reason);records.outcomes=settled.map(r=>r.status==='fulfilled'?{status:r.status}:{status:r.status,error:String(r.reason)});await persist();
  if(errors.length)throw new AggregateError(errors,'Native orb action or observation failed');
  await save('after-orb');await game.act({keys:['Insert'],duration_ms:300});const after=await save('ordinary-nexus-escape');
  game.check('ordinary orb really spends mana without primary shots',records.samples.some(s=>s.mp<ready.view.character.mp)&&records.samples.every(s=>s.shots===0));
  game.check('both original zero-HP Fire and Thunder turrets receive Stasis',records.samples.some(s=>s.targets.every(t=>t.hp===0&&(t.condition_mask&(1<<21))!==0)));
  game.check('same living Mystic escapes without kills or Tomb credit',after.view.map.name==='Nexus'&&after.view.lifecycle.alive&&after.view.session.character===before.view.session.character&&after.diagnostic.character.kills===0&&after.diagnostic.character.tombs===0);
}
