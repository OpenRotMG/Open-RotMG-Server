import {writeFile,readFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
const gear={class:0x30e,level:20,seed:12351,equipment:[0xa9e,0xa2e,0xa80,-1],backpack:Array(8).fill(-1),stats:[500,200,35,10,25,30,30,25]};
let spawn;
export const fixture={...gear,world:'pirate-cave',worldSeed:12351,position(info) {
  spawn={x:info.spawn_points[0][0],y:info.spawn_points[0][1]};
  const boss=info.encounter_markers.find(e=>e.kind===0x927);
  if(!boss)throw Error('Pirate King marker missing');
  return {x:boss.position.x-6,y:boss.position.y};
}};
export async function run(game) {
  let serial=0;
  async function command(command) {
    const path=join(process.env.QA_RESULTS_DIR,'regroup-'+serial+'.json');
    await writeFile(path,JSON.stringify({id:'regroup-'+serial++,command}));
    return JSON.parse(execFileSync(process.execPath,[resolve('scripts/game.mjs'),join(process.env.QA_RESULTS_DIR,'native/native.json'),path],{encoding:'utf8'}));
  }
  const human=await game.observe();
  const {result:bot}=await command({op:'spawn',spec:{...gear,guid:'regroup-bot',world:human.session.world,position:spawn,controller:'game_bot',profile:'Bugmaster'}});
  const before=await command({op:'observe',actor:bot.actor});
  const distance=p=>Math.hypot(p.x-human.position.x,p.y-human.position.y);
  game.check('prepared rusher is more than 24 tiles ahead',distance(before.position)>24);
  await game.act({keys:[],duration_ms:3000},{name:'dungeon-rusher',when:v=>v.game_ms>=human.game_ms+600});
  const after=await command({op:'observe',actor:bot.actor});
  await writeFile(join(process.env.QA_RESULTS_DIR,'regroup-observed.json'),JSON.stringify({before,after,human},null,2));
  const log=await readFile(join(process.env.QA_RESULTS_DIR,'native/server.log'),'utf8');
  const teleport=log.split('\n').filter(line=>line.includes('player teleported')).map(line=>JSON.parse(line).fields).find(event=>event.player===before.session.player);
  game.check('same living bot character',after.lifecycle.alive&&after.session.character===before.session.character);
  game.check(process.env.REGROUP_BASELINE==='1'?'baseline walks instead of teleporting':'bot actually lands on the native rusher',process.env.REGROUP_BASELINE==='1'?!teleport:teleport?.target===human.session.player&&Math.hypot(teleport.x-human.position.x,teleport.y-human.position.y)<0.2);
}
