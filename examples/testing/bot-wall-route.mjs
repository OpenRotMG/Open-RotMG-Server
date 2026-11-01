import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';

const gear={class:0x30e,level:20,seed:1239,equipment:[0xa97,-1,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,10,25,25,30,40,60]};
export const fixture={...gear,world:'kitchen',position:{x:12.6,y:4.5}};
export async function run(game){
  const root=process.env.QA_RESULTS_DIR,config=JSON.parse(await readFile(join(root,'native/native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim();let serial=0;
  const api=async command=>{
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:'wall-'+(++serial),command}),signal:AbortSignal.timeout(10000)});
    const value=await response.json();assert(response.ok&&value.status!=='failed',JSON.stringify(value));return value;
  };
  const observer=await game.observe(),initial=await game.inspect();
  game.check('selected Grey Wall blocks the prepared crossing',initial.view.objects.some(o=>o.kind===0x120&&o.position.x===12.5&&o.position.y===10.5));
  const created=await api({op:'spawn',spec:{...gear,world:observer.session.world,guid:'wall-bot',position:{x:12.4,y:12.2},controller:'game_bot',profile:'DrFlux'}});
  const before=await api({op:'observe',actor:created.result.actor});
  await game.act({keys:[],duration_ms:500,chat:'DrFlux follow me'});
  const samples=[];
  for(let i=0;i<6;i++){
    await game.act({keys:[],duration_ms:1000},i===2?{name:'bot-wall-route',when:v=>v.lifecycle.alive}:undefined);
    const view=await api({op:'observe',actor:created.result.actor});
    const feeds=await (await fetch(config.origin+'/api/bots')).json();
    samples.push({view,feed:feeds.bots.find(b=>b.name==='DrFlux')});
    await writeFile(join(root,'bot-wall-observations.json'),JSON.stringify({prepared:true,limits:'Unprotected prepared Kitchen crossing of selected Grey Wall; not the original Realm incident or earned progression.',observer,before,samples},null,2));
  }
  const after=samples.at(-1).view;
  game.check('ordinary bot accepted the native follow invitation',samples.some(s=>s.feed?.frame?.goal==='Follow('+observer.session.player+')'));
  game.check('bot actually progressed around the wall to its north side',samples.some(s=>s.view.position.y<9.4));
  game.check('same living unprotected bot remains under normal collision',after.lifecycle.alive&&after.session.character===before.session.character&&after.character.conditions===0);
  game.check('native observer remains alive',(await game.observe()).lifecycle.alive);
}
