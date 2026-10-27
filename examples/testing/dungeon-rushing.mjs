import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';

export const fixture={class:0x310,level:20,seed:751,world:'snake-pit',worldSeed:751,position:'spawn',equipment:[0xaf6,-1,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,50,25,50,50,40,75]};
export async function run(game){
  const root=process.env.QA_RESULTS_DIR,config=JSON.parse(await readFile(join(root,'native','native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  let serial=0;
  const api=async command=>{
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:'rush-'+(++serial),command}),signal:AbortSignal.timeout(10000)});
    const result=await response.json();assert(response.ok&&result.status!=='failed',JSON.stringify(result));return result;
  };
  const initial=await game.inspect();
  const response=await fetch(config.origin+'/dev/map?player='+initial.view.session.player,{headers:{Authorization:'Bearer '+token}});
  assert(response.ok);const map=await response.json();
  const grate=map.entities.filter(e=>e.kind===0x173).sort((a,b)=>Math.hypot(a.pos.x-initial.view.position.x,a.pos.y-initial.view.position.y)-Math.hypot(b.pos.x-initial.view.position.x,b.pos.y-initial.view.position.y))[0];assert(grate);
  const position={x:grate.pos.x,y:grate.pos.y+2};
  const {worldSeed,...observer}=fixture;
  game=await game.join({...observer,world:initial.view.session.world,position});
  await api({op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invincible',duration_ms:60000}});
  const spawned=await api({op:'spawn',spec:{guid:'rushing-751',class:0x30e,level:20,seed:751,world:initial.view.session.world,position,equipment:[0xa9e,-1,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,50,25,50,50,40,60],controller:'game_bot',profile:'DrFlux'}});
  const actor=spawned.result.actor,observations=[];
  for(let i=0;i<6;i++){
    await game.act({keys:[],duration_ms:2000},i===2?{name:'rushing',when:v=>v.lifecycle.alive}:undefined);
    const bot=await api({op:'inspect',actor});
    const feeds=await (await fetch(config.origin+'/api/bots')).json();
    observations.push({bot,feeds});
    await writeFile(join(root,'rushing-observations.json'),JSON.stringify({initial,position,observerProtection:'invincible; bot unprotected',observations},null,2));
  }
  const last=observations.at(-1).bot;
  game.check('native observer remains alive', (await game.observe()).lifecycle.alive);
  game.check('prepared rusher remains alive',last.view.lifecycle.alive);
  game.check('rusher has no prepared protection',(last.view.character.conditions&((1<<24)|(1<<25)|(1<<22)))===0);
  game.check('rusher fires ordinary shots',last.diagnostic.character.shots>0);
  game.check('rusher makes progress away from entry',Math.hypot(last.view.position.x-position.x,last.view.position.y-position.y)>8);
}
