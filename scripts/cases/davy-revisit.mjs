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
  async function nearest(kind,near=()=>true){const m=await scene(),v=await game.observe(),choices=m.entities.filter(e=>e.kind===kind&&near(e,m)).flatMap(e=>{let p;try{p=route(m,v.position,e.pos,mask).points;}catch(error){if(error.code!=='ERR_ASSERTION')throw error;return [];}return [{e,cost:p.reduce((sum,q,i)=>sum+(i?distance(q,p[i-1]):0),0)}];}).sort((a,b)=>a.cost-b.cost);assert(choices.length,'no reachable generated object of kind '+kind);return choices[0].e;}
  // Use a barrel in a room the generator crewed with a Lil' Ghost Pirate.
  const barrel=await nearest(0x182e,(e,m)=>m.entities.some(g=>g.kind===0xe31&&distance(g.home,e.home)<8));assert(barrel.hp===2200);
  // The seed can put the southern side in a wall; goal the barrel so the planner picks its reachable side.
  await walk(barrel.pos,mask);
  for(let n=0;n<20&&(await scene()).entities.some(e=>e.id===barrel.id);n++)await game.act({keys:[],duration_ms:2000,target:{entity:barrel.id,camera_angle:-Math.PI/4,fire:true}});
  game.check('ordinary journey destroys a natural ship barrel',!(await scene()).entities.some(e=>e.id===barrel.id));
  let ghost;
  for(let n=0;n<12;n++){ghost=(await scene()).entities.find(e=>e.kind===0xe31&&distance(e.home,barrel.home)<8);if(ghost)break;await game.act({keys:[],duration_ms:500});}assert(ghost);
  // Recovered Lil' Ghost Pirate (actors.xml:11098) stays invincible in Ini until a nearby help call wakes it.
  await walk(ghost.pos,mask);
  // HelpCall is probabilistic even within 4.5 tiles (ghostShip.xml:2397); wait for the same ghost's Fight state.
  let waking;
  for(let n=0;n<=15;n++){
    waking=await game.inspect();
    if(waking.diagnostic.xml[ghost.id]?.states.some(s=>s.name==='Fight')||n===15)break;
    const waiting=waking.diagnostic.entities.find(e=>e.id===ghost.id);
    assert(waiting&&distance(waking.view.position,waiting.pos)<4.5,'Natural HelpCall requires a nearby player');
    await game.act({keys:[],duration_ms:4000});
  }
  await save('room-ghost-wake');
  assert(waking.diagnostic.xml[ghost.id]?.states.some(s=>s.name==='Fight'),'Nearby room ghost did not naturally wake within 60 game seconds');
  let last=ghost.pos;
  for(let n=0;n<20;n++){const g=(await scene()).entities.find(e=>e.id===ghost.id);if(!g)break;last=g.pos;await game.act({keys:[],duration_ms:1000,target:{entity:ghost.id,camera_angle:-Math.PI/4,fire:true}});}
  game.check('ordinary journey defeats renewable room ghost',!(await scene()).entities.some(e=>e.id===ghost.id));
  // Decor varies by generated room; take two nonblocking prop kinds (Bones, Stool, Small Candle: no OccupySquare) the Locker holds.
  const decor=(await scene()).entities,kinds=[0x1834,0x1832,0x1831].filter(k=>decor.some(e=>e.kind===k)).slice(0,2);assert.equal(kinds.length,2,'Generated Locker lacks two nonblocking prop kinds');
  // Props stand beside walls, so finish with one centred step onto the tile the two-tile-clearance route stops short of.
  async function step(p){const v=await game.observe(),speed=4+5.6*v.character.stats.speed/75,legs=[];let at=v.position;for(const q of [{x:at.x,y:p.y},p]){const dx=q.x-at.x,dy=q.y-at.y;if(Math.hypot(dx,dy)>=.1)legs.push({actor:game.actor,input:{keys:Math.abs(dx)>Math.abs(dy)?(dx>0?['d','s']:['a','w']):(dy>0?['a','s']:['d','w']),duration_ms:Math.max(30,Math.round(Math.hypot(dx,dy)/speed*1000))}});at=q;}if(legs.length)await game.act(legs);}
  for(const kind of kinds){const prop=await nearest(kind);await walk(prop.pos,mask);await step({x:Math.floor(prop.pos.x)+.5,y:Math.floor(prop.pos.y)+.5});const v=await save('crossed-prop-'+kind);game.check('native walk enters selected nonblocking prop tile '+kind,Math.floor(v.view.position.x)===Math.floor(prop.pos.x)&&Math.floor(v.view.position.y)===Math.floor(prop.pos.y)&&v.diagnostic.entities.some(e=>e.id===prop.id));}
  await walk({x:captain.x-2,y:captain.y},mask);
  for(const player of [game,peer])game.check('room revisit leaves native participant outside respawn range '+player.actor,distance((await player.observe()).position,ghost.home)>12);
  for(let n=0;n<4;n++)await game.act({keys:[],duration_ms:1800});await save('far-from-cleared-room');
  await walk(barrel.home,mask);await step({x:Math.floor(barrel.home.x)+.5,y:Math.floor(barrel.home.y)+.5});const revisited=await save('ordinary-room-revisited');
  game.check('far room revisit retains destroyed barrel and releases its actual tile',!revisited.diagnostic.entities.some(e=>e.kind===0x182e&&distance(e.home,barrel.home)<.01)&&Math.floor(revisited.view.position.x)===Math.floor(barrel.home.x)&&Math.floor(revisited.view.position.y)===Math.floor(barrel.home.y));
  // MetamorphoseAtDeath (posJitter 2, actors.xml:11143) replaced it at once; the replacement, not the old identity, remains.
  game.check('far room revisit observes the renewed ghost beside the old one',revisited.diagnostic.entities.some(e=>e.kind===0xe31&&e.id!==ghost.id&&distance(e.home,last)<=2.9)&&!revisited.diagnostic.entities.some(e=>e.id===ghost.id));
  await game.act({keys:[],duration_ms:900},{name:'ordinary-room-revisited',when:()=>true});
}
