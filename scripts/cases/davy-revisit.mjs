import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {run as traverse} from './davy-locker.mjs';
export {fixture} from './davy-locker.mjs';
const distance=(a,b)=>Math.hypot(a.x-b.x,a.y-b.y);

export async function run(native){
  const out=process.env.QA_RESULTS_DIR,samples=[];
  await writeFile(join(out,'traversal-case.mjs'),await readFile(new URL('./davy-locker.mjs',import.meta.url)));
  const {game,peer,walk,scene,mask,route}=await traverse(native);
  async function save(name){const state=await game.inspect();samples.push({name,state});await writeFile(join(out,'revisit-observations.json'),JSON.stringify(samples));return state;}
  let m=await scene();const boss=m.entities.find(e=>e.kind===0xe32);assert(boss);const captain={...boss.home};
  await walk({x:captain.x+2,y:captain.y+1},mask,peer);
  async function nearest(kind){const m=await scene(),v=await game.observe(),choices=m.entities.filter(e=>e.kind===kind).map(e=>{const p=route(m,v.position,e.pos,mask);return {e,cost:p.reduce((sum,q,i)=>sum+(i?distance(q,p[i-1]):0),0)};}).sort((a,b)=>a.cost-b.cost);assert(choices.length,'no generated object of kind '+kind);return choices[0].e;}
  const barrel=await nearest(0x182e);assert(barrel.hp===2200);await walk({x:barrel.pos.x,y:barrel.pos.y+2},mask);
  for(let n=0;n<20&&(await scene()).entities.some(e=>e.id===barrel.id);n++)await game.act({keys:[],duration_ms:2000,target:{entity:barrel.id,camera_angle:-Math.PI/4,fire:true}});
  game.check('ordinary journey destroys a natural ship barrel',!(await scene()).entities.some(e=>e.id===barrel.id));
  let ghost;
  for(let n=0;n<12;n++){ghost=(await scene()).entities.find(e=>e.kind===0xe31&&distance(e.home,barrel.home)<8);if(ghost)break;await game.act({keys:[],duration_ms:500});}assert(ghost);
  for(let n=0;n<20&&(await scene()).entities.some(e=>e.id===ghost.id);n++)await game.act({keys:[],duration_ms:1000,target:{entity:ghost.id,camera_angle:-Math.PI/4,fire:true}});
  game.check('ordinary journey defeats renewable room ghost',!(await scene()).entities.some(e=>e.id===ghost.id));
  for(const kind of [0x1834,0x1832]){const prop=await nearest(kind);await walk(prop.pos,mask);const v=await save('crossed-prop-'+kind);game.check('native walk enters selected nonblocking prop tile '+kind,Math.floor(v.view.position.x)===Math.floor(prop.pos.x)&&Math.floor(v.view.position.y)===Math.floor(prop.pos.y)&&v.diagnostic.entities.some(e=>e.id===prop.id));}
  await walk({x:captain.x-2,y:captain.y},mask);
  for(const player of [game,peer])game.check('room revisit leaves native participant outside respawn range '+player.actor,distance((await player.observe()).position,ghost.home)>12);
  for(let n=0;n<4;n++)await game.act({keys:[],duration_ms:1800});await save('far-from-cleared-room');
  await walk(barrel.home,mask);const revisited=await save('ordinary-room-revisited');
  game.check('far room revisit retains destroyed barrel and releases its actual tile',!revisited.diagnostic.entities.some(e=>e.kind===0x182e&&distance(e.home,barrel.home)<.01)&&Math.floor(revisited.view.position.x)===Math.floor(barrel.home.x)&&Math.floor(revisited.view.position.y)===Math.floor(barrel.home.y));
  game.check('far room revisit observes a renewed ghost at the same home',revisited.diagnostic.entities.some(e=>e.kind===0xe31&&e.id!==ghost.id&&distance(e.home,ghost.home)<.01));
  await game.act({keys:[],duration_ms:900},{name:'ordinary-room-revisited',when:()=>true});
}
