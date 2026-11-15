import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
export const realmSeed=7;
export async function prepare(api){
  const world=await api({op:'world_info',world:1});
  if(world.realm_layout!=='classic-world-03-reconstruction')throw Error('Expected seeded World3');
  const registry=JSON.parse(await readFile(resolve('data/realm-sites.json'),'utf8'));
  const site=registry.find(r=>r.layout===world.realm_layout).sites.find(s=>s.kind===0x668);
  await writeFile(join(process.env.QA_RESULTS_DIR,'site.json'),JSON.stringify(site));
  return {class:0x30e,level:20,seed:1232,world:1,position:{x:site.chest[0]+.5,y:site.chest[1]+.5},equipment:[0xa97,0xa2e,-1,-1],backpack:Array(8).fill(-1),stats:null};
}
export async function run(game){
  const request=join(process.env.QA_RESULTS_DIR,'protect.json');
  await writeFile(request,JSON.stringify({id:'fissure-protect',command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}}));
  execFileSync(process.execPath,[resolve('scripts/game.mjs'),join(process.env.QA_RESULTS_DIR,'native/native.json'),request],{stdio:'pipe'});
  const before=await game.inspect(),start=(await game.observe()).game_ms;
  await game.act({keys:[],duration_ms:1800},{name:'fissure',when:v=>v.game_ms>=start+1000});
  const after=await game.inspect();
  await writeFile(join(process.env.QA_RESULTS_DIR,'inspection.json'),JSON.stringify({before,after}));
  const floor=after.view.objects.find(e=>e.kind===0x153&&Math.floor(e.position.x)===Math.floor(after.view.position.x)&&Math.floor(e.position.y)===Math.floor(after.view.position.y));
  game.check('chest tile has a visible Partial Red Floor',!!floor);
  game.check('prepared invulnerable observer remains alive',after.view.lifecycle.alive);
  const chest=after.diagnostic.entities.find(e=>e.kind===0x501&&e.items[0]>=0&&Math.hypot(e.pos.x-after.view.position.x,e.pos.y-after.view.position.y)<1.5);
  if(!chest)throw Error('No ordinary chest item within native interaction range');
  await game.act({keys:[],duration_ms:700,drag:{from:{x:634,y:528},to:{x:634,y:428}}});
  const looted=await game.observe();
  game.check('native chest pickup commits the real item once',looted.character.items[4]===chest.items[0]&&looted.character.inventory_revision===after.view.character.inventory_revision+1);
}
