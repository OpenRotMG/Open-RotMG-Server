import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
const out=process.env.QA_RESULTS_DIR;
export const realmSeed=1058;
let root,area;
export async function prepare(api){
  const info=await api({op:'world_info',world:1});
  const events=(await readFile(join(out,'native/server.log'),'utf8')).split('\n').filter(l=>l.startsWith('{')).map(l=>JSON.parse(l).fields);
  const end=events.findLastIndex(f=>f.message==='Realm simulation populated'&&f.layout===info.realm_layout);
  const start=events.slice(0,end).findLastIndex(f=>f.message==='Realm simulation populated');
  const current=events.slice(start+1,end);
  root=current.find(f=>f.message==='finite named Realm quest placed'&&f.kind===0x675);
  assert(root,'Seed requires a naturally placed Phoenix Lord');
  area=current.find(f=>f.message==='named Realm boss setpiece placed'&&f.kind===0x675&&root.x>=f.x&&root.x<f.x+f.width&&root.y>=f.y&&root.y<f.y+f.height);
  assert(area,'Phoenix bounds must come from actual placement');
  await writeFile(join(out,'site.json'),JSON.stringify({info,root,area}));
  return {class:0x31d,level:20,seed:1058,world:1,position:{x:root.x,y:root.y},equipment:[-1,-1,0xafc,-1],backpack:Array(8).fill(-1),stats:[770,252,75,25,50,50,75,0]};
}
export async function run(game){
  const before=await game.inspect();
  await game.act({keys:['s'],duration_ms:150});
  const moved=await game.observe();
  game.check('native walking at the naturally placed Pyre',Math.hypot(moved.position.x-before.view.position.x,moved.position.y-before.view.position.y)>.3);
  await game.act({keys:[],duration_ms:1000},{name:'phoenix-god-boundary',when:()=>true});
  const after=await game.inspect();
  await writeFile(join(out,'boundary.json'),JSON.stringify({before,after,area}));
  game.check('Lord and its native followers remain active inside the Pyre',after.diagnostic.entities.some(e=>e.id===root.entity&&e.activated!==null)&&after.diagnostic.entities.some(e=>e.quest===root.entity&&[0x676,0x677].includes(e.kind)));
  game.check('same prepared living character',after.view.lifecycle.alive&&after.view.session.character===before.view.session.character);
}
