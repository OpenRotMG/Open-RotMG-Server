import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {setTimeout as delay} from 'node:timers/promises';
import {randomUUID} from 'node:crypto';
import {connect} from '../../scripts/record-demo.mjs';
const spec={class:0x324,level:20,seed:879,world:-2,equipment:[-1,0xae3,-1,-1],backpack:Array(8).fill(-1),stats:[720,252,65,25,50,75,40,0]};
const root=()=>join(process.env.QA_RESULTS_DIR,'native');
let blocked;
export async function prepare(api){
  const info=await api({op:'world_info',world:-2}),[x,y]=info.spawn_points[0];
  const scout=await api({op:'spawn',spec:{...spec,guid:'ability-wall-scout',position:{x,y},controller:'direct',profile:null}});
  const state=await api({op:'inspect',actor:scout.result.actor});
  const report=JSON.parse(await readFile(join(root(),'result.json'),'utf8'));
  const token=(await readFile(join(root(),'state/control.token'),'utf8')).trim();
  const response=await fetch(report.origin+'/dev/map?player='+state.diagnostic.player,{headers:{Authorization:'Bearer '+token},signal:AbortSignal.timeout(10000)});assert(response.ok);
  const map=await response.json(),walls=new Set(map.blocked.map(p=>p.join(',')));
  for(const [bx,by] of map.blocked)if(Math.hypot(bx+.5-x,by+.5-y)<20)for(const [dx,dy] of [[-1,0],[1,0],[0,-1],[0,1]]){
    const px=bx+dx,py=by+dy;
    if(px>0&&py>0&&px<map.width-1&&py<map.height-1&&!walls.has([px,py].join(','))){blocked={x:bx+.5,y:by+.5};return {...spec,position:{x:px+.5,y:py+.5}};}
  }
  throw Error('No real Nexus wall with a walkable neighboring starting context');
}
export async function run(game){
  const config=JSON.parse(await readFile(join(root(),'native.json'),'utf8')),records=[];
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const api=async command=>{const r=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command}),signal:AbortSignal.timeout(10000)});const s=await r.json();assert(r.ok&&s.status==='succeeded',JSON.stringify(s));return s;};
  const record=async(label,client=game)=>{const s=await client.inspect();records.push({label,...s});await writeFile(join(root(),'refusal-observations.json'),JSON.stringify(records));return s;};
  const browser=config.native.find(n=>n.actor===game.actor),cdp=await connect({port:browser.port,target:browser.target,url:config.origin+'/',helper:config.browserTools});
  async function pauseChat(){
    const action=game.act({keys:[],duration_ms:1600});
    const input=(async()=>{const deadline=Date.now()+5000;while((await game.observe()).paused){assert(Date.now()<deadline);await delay(20);}for(const type of ['keyDown','keyUp'])await cdp.call('Input.dispatchKeyEvent',{type,key:'Enter',code:'Enter',windowsVirtualKeyCode:13});await cdp.call('Input.insertText',{text:'/pause'});for(const type of ['keyDown','keyUp'])await cdp.call('Input.dispatchKeyEvent',{type,key:'Enter',code:'Enter',windowsVirtualKeyCode:13});})();
    const results=await Promise.allSettled([action,input]);for(const r of results)if(r.status==='rejected')throw r.reason;
  }
  const initial=await record('initial');
  try{
    await pauseChat();const paused=await record('native-pause');
    game.check('ordinary native chat enters player Paused state',!!(paused.view.character.conditions&(1<<20)));
    await game.act({keys:[' '],duration_ms:200});const denied=await record('paused-refusal');
    game.check('native Paused ability refuses without spend movement or deployment',denied.view.character.mp>=paused.view.character.mp&&JSON.stringify(denied.view.position)===JSON.stringify(paused.view.position)&&denied.diagnostic.entities.every(e=>e.kind!==0x715)&&denied.view.character.ability_ready_ms<=denied.view.game_ms);
    await pauseChat();game.check('ordinary native chat exits player Paused state',!((await game.observe()).character.conditions&(1<<20)));
  }finally{cdp.close();}
  const before=await record('blocked-before'),dx=blocked.x-before.view.position.x,dy=blocked.y-before.view.position.y,a=-Math.PI/4;
  await game.act({keys:[' '],duration_ms:200,mouse:{x:300+50*(dx*Math.cos(a)+dy*Math.sin(a)),y:325+50*(-dx*Math.sin(a)+dy*Math.cos(a))}});
  const denied=await record('blocked-refusal');
  game.check('native blocked teleport refuses before mana cooldown or decoy creation',denied.view.character.mp>=before.view.character.mp&&JSON.stringify(denied.view.position)===JSON.stringify(before.view.position)&&denied.view.character.ability_ready_ms<=denied.view.game_ms&&denied.diagnostic.entities.every(e=>e.kind!==0x715));
  const quiet=await game.join({class:0x30e,level:20,seed:879,world:'oryx-castle',position:{x:24.5,y:139.5},equipment:[-1,0xa2e,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,75,25,50,75,40,0]});
  await api({op:'cheat',actor:quiet.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}});
  await quiet.act({keys:[],duration_ms:1000});const hit=await record('natural-quiet',quiet);
  game.check('natural Living Floor projectile applies Quiet and drains MP',!!(hit.view.character.conditions&2)&&hit.view.character.mp===0);
  await api({op:'cheat',actor:quiet.actor,change:{kind:'vitals',hp:hit.view.character.hp,mp:200}});
  const funded=await record('explicit-funded-quiet',quiet);
  game.check('explicit mana preparation isolates Quiet from insufficient MP',!!(funded.view.character.conditions&2)&&funded.view.character.mp===200);
  await quiet.act({keys:[' '],duration_ms:100});
  const refused=await record('quiet-refusal',quiet);
  game.check('native Quiet with enough MP rejects without spending starting cooldown or shooting',refused.view.character.mp===200&&refused.view.character.ability_ready_ms<=refused.view.game_ms&&refused.diagnostic.character.shots===funded.diagnostic.character.shots&&JSON.stringify(refused.view.character.items)===JSON.stringify(funded.view.character.items));
  await quiet.act({keys:[],duration_ms:700},{name:'quiet-refusal',when:()=>true});
  game.check('both prepared characters remain alive',refused.view.lifecycle.alive&&(await game.observe()).lifecycle.alive&&initial.view.session.character===(await game.observe()).session.character);
}
