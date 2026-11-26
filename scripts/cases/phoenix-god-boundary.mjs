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
  root=current.find(f=>f.message==='linked Realm site placed'&&f.kind===0x675);
  assert(root,'Seed requires a naturally placed Phoenix Lord');
  area=current.find(f=>f.message==='named Realm boss setpiece placed'&&f.kind===0x675&&root.x>=f.x&&root.x<f.x+f.width&&root.y>=f.y&&root.y<f.y+f.height);
  assert(area,'Phoenix bounds must come from actual placement');
  await writeFile(join(out,'site.json'),JSON.stringify({info,root,area}));
  return {class:0x31d,level:20,seed:1058,world:1,position:{x:root.x,y:root.y},equipment:[-1,-1,0xafc,-1],backpack:Array(8).fill(-1),stats:[770,252,75,25,50,50,75,0]};
}
export async function run(game){
  const before=await game.inspect();
  // The Pyre's scenery can wall off any one side of the Lord's spawn point.
  const moved=async()=>{const v=await game.observe();return Math.hypot(v.position.x-before.view.position.x,v.position.y-before.view.position.y)>.3;};
  for(const key of ['s','d','w','a'])if(!await moved())await game.act({keys:[key],duration_ms:150});
  game.check('native walking at the naturally placed Pyre',await moved());
  await game.act({keys:[],duration_ms:1000},{name:'phoenix-god-boundary',when:()=>true});
  const after=await game.inspect();
  await writeFile(join(out,'boundary.json'),JSON.stringify({before,after,area}));
  // XML Pyre birds are the Lord's MakeMinions children (actors.xml:1903).
  game.check('Lord and its native followers remain active inside the Pyre',after.diagnostic.entities.some(e=>e.id===root.entity&&e.activated!==null)&&after.diagnostic.entities.some(e=>(e.quest===root.entity||e.parent===root.entity)&&[0x676,0x677].includes(e.kind)));
  game.check('same prepared living character',after.view.lifecycle.alive&&after.view.session.character===before.view.session.character);
}
