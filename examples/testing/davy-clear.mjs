import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {run as traverse,fixture as kit} from './davy-locker.mjs';
export {fixture} from './davy-locker.mjs';

export async function fight({game,players,scene,reserve,save,walk,mask}){
  assert.equal(players.length,3);
  const posts=[{x:-4.5,y:1.5},{x:3.5,y:1.5},{x:-.5,y:-1}];
  async function shot(player,lamp,duration_ms){const v=await player.observe(),a=-Math.PI/4,dx=lamp.pos.x-v.position.x,dy=lamp.pos.y-v.position.y,x=50*(dx*Math.cos(a)+dy*Math.sin(a)),y=50*(-dx*Math.sin(a)+dy*Math.cos(a));assert(Math.hypot(x,y)<=270,'Lamp aim exceeds native reticle');return {actor:player.actor,input:{keys:[' '],duration_ms,mouse:{x:300+x,y:325+y}}};}
  for(let cycle=0;cycle<6;cycle++){
    let m=await scene(),boss=m.entities.find(e=>e.kind===0xe32);if(!boss)return;
    for(let n=0;n<15;n++){m=await scene();boss=m.entities.find(e=>e.kind===0xe32);if(!boss||boss.stage===1)break;await reserve(5000);await game.act({keys:[],duration_ms:2000});}
    if(!boss)return;
    for(let i=0;i<players.length;i++)await walk({x:boss.home.x+posts[i].x,y:boss.home.y+posts[i].y},mask,players[i]);
    if(cycle>0)for(const player of players){const v=await player.observe(),slot=v.character.items.findIndex((item,i)=>i>=4&&item===0xa23);assert(slot>=4,'Expected finite prepared Magic Potion');await player.act({keys:[String(slot-3)],duration_ms:150});}
    for(let n=0;;n++){if((await Promise.all(players.map(p=>p.observe()))).every(v=>v.character.mp===v.character.stats.max_mp))break;assert(n<20,'Normal MP regeneration did not fill the next cycle');await reserve(7000);await game.act({keys:[],duration_ms:5000});}
    await reserve(22000);m=await scene();boss=m.entities.find(e=>e.kind===0xe32);assert(boss);
    const hp=boss.hp,lamps=m.entities.filter(e=>e.kind===0xe35).sort((a,b)=>a.pos.y-b.pos.y||a.pos.x-b.pos.x);assert.equal(lamps.length,8);
    const first=[];for(const indices of [[2,3,0],[4,5,1]])first.push(await Promise.all(players.map((p,i)=>shot(p,lamps[indices[i]],900))));
    await game.act(first);await save('first-lamps-'+cycle);
    const off=new Set((await scene()).entities.filter(e=>e.kind===0xe35).map(e=>e.id)),pending=[[2,4],[3,5],[0,1]].map(indices=>indices.map(i=>lamps[i]).filter(e=>off.has(e.id)));
    pending[0].push(lamps[6],lamps[6]);pending[1].push(lamps[7],lamps[7]);
    // Fixed aim keeps a second cast valid after a lamp transforms; damage rolls are never forced.
    const sequence=[];for(let n=0;n<Math.max(...pending.map(p=>p.length));n++){const group=[];for(let i=0;i<players.length;i++)if(n<pending[i].length)group.push(await shot(players[i],pending[i][n],600));sequence.push(group);}
    sequence.push(players.map((p,i)=>({actor:p.actor,input:{keys:[['d','s'],['a','w'],['a','s']][i],duration_ms:250}})));
    sequence.push(players.map(p=>({actor:p.actor,input:{keys:[' '],duration_ms:1080,target:{entity:boss.id,camera_angle:-Math.PI/4,fire:true}}})));
    await game.act(sequence);const volley=await save('native-lamps-and-captain-'+cycle),remaining=volley.diagnostic.entities.find(e=>e.id===boss.id);
    game.check('bounded native lamp cycle actually damages captain '+cycle,!remaining||remaining.hp<hp);
  }
}
export async function run(native){
  const out=process.env.QA_RESULTS_DIR,samples=[];
  await writeFile(join(out,'traversal-case.mjs'),await readFile(new URL('./davy-locker.mjs',import.meta.url)));
  const context=await traverse(native),{game,peer,walk,scene,mask}=context;
  async function save(name){const state=await game.inspect();samples.push({name,state});await writeFile(join(out,'clear-observations.json'),JSON.stringify(samples));return state;}
  let m=await scene();const boss=m.entities.find(e=>e.kind===0xe32);assert(boss);
  const world=(await game.observe()).session.world,players=[game,peer,await context.add({...kit,world,position:{x:boss.home.x-.5,y:boss.home.y-1},backpack:[-1,...Array(7).fill(0xa23)]})];
  const start=await save('captain-start');await fight({...context,players,save});
  const defeated=await save('captain-defeated');
  game.check('same journey defeats captain and commits exactly one Locker clear',!defeated.diagnostic.entities.some(e=>e.kind===0xe32)&&defeated.diagnostic.character.davy_lockers===start.diagnostic.character.davy_lockers+1);
  m=await scene();const exit=m.entities.find(e=>e.kind===0x704),bag=m.entities.find(e=>e.items.some(item=>item>=0)&&e.extra_stats.some(([stat,owner])=>stat===54&&owner===defeated.view.session.account));assert(exit&&bag,'Clear must publish Realm exit and an actual eligible personal reward');
  await walk(bag.pos,mask);await game.act({keys:[],duration_ms:700},{name:'captain-earned-reward',when:()=>true});const before=await save('before-loot');
  await game.act({keys:[],duration_ms:350,drag:{from:{x:634,y:528},to:{x:634,y:428}}});const picked=await save('reward-collected');
  game.check('normal loot drag commits eligible reward once',picked.view.character.items[4]>=0&&picked.view.character.inventory_revision===before.view.character.inventory_revision+1);
  await game.act({keys:[],duration_ms:1500});const stable=await save('clear-stable');
  game.check('Locker completion does not repeat after defeat',stable.diagnostic.character.davy_lockers===defeated.diagnostic.character.davy_lockers);
  await walk(exit.pos,mask);await game.act({keys:[],mouse:{x:700,y:575},duration_ms:700});for(let n=0;n<8&&(await game.observe()).world!=='Realm';n++)await game.act({keys:[],duration_ms:500});
  const returned=await save('earned-Realm-exit');
  game.check('earned exit retains same living character, reward and clear',returned.view.map.name==='Realm'&&returned.view.lifecycle.alive&&returned.view.session.character===start.view.session.character&&returned.view.character.items[4]===picked.view.character.items[4]&&returned.diagnostic.character.davy_lockers===defeated.diagnostic.character.davy_lockers);
  await game.act({keys:[],duration_ms:1200},{name:'locker-clear-Realm-return',when:()=>true});
}
