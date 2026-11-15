import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';

const root=process.argv[process.argv.indexOf('--run-dir')+1];
export const fixture={class:0x310,level:20,seed:12351,world:'sprite-world',worldSeed:1,position:'spawn',equipment:[0xa87,-1,0xb05,0xacd],backpack:Array(8).fill(-1),stats:[670,385,50,25,50,50,40,75]};
export async function run(game){
  const config=JSON.parse(await readFile(join(root,'native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const initial=await game.inspect();
  const map=async()=>{
    const response=await fetch(config.origin+'/dev/map?player='+initial.diagnostic.player,{headers:{Authorization:'Bearer '+token},signal:AbortSignal.timeout(10000)});
    assert(response.ok);return response.json();
  };
  const before=await game.observe();
  await game.act({keys:[],duration_ms:800});
  const idle=await game.observe();
  game.check('white entrance remains stationary',Math.hypot(idle.position.x-before.position.x,idle.position.y-before.position.y)<.05);
  const layout=await map(),detail=await game.inspect(),tiles=detail.view.tiles;
  const ground=(x,y)=>tiles.find(t=>t.x===Math.floor(x)&&t.y===Math.floor(y))?.ground;
  const approaches=[{dx:1,dy:0,keys:['d','s']},{dx:-1,dy:0,keys:['a','w']},{dx:0,dy:1,keys:['a','s']},{dx:0,dy:-1,keys:['d','w']}];
  const choices=layout.entities.filter(e=>e.kind>=0x174&&e.kind<=0x179).sort((a,b)=>Math.hypot(a.pos.x-before.position.x,a.pos.y-before.position.y)-Math.hypot(b.pos.x-before.position.x,b.pos.y-before.position.y)).flatMap(tree=>approaches.map(direction=>({tree,...direction}))).filter(({tree,dx,dy})=>[0,1,2].every(n=>ground(tree.pos.x-n*dx,tree.pos.y-n*dy)===0xa0)&&!layout.blocked.some(([x,y])=>x===Math.floor(tree.pos.x-dx)&&y===Math.floor(tree.pos.y-dy)));
  assert(choices.length,'seed must expose a tree with a stationary clear approach');
  const {tree,dx,dy,keys}=choices[0];
  const {worldSeed,...prepared}=fixture;
  const peer=await game.join({...prepared,seed:12352,world:before.session.world,position:{x:tree.pos.x-dx,y:tree.pos.y-dy}});
  const start=await peer.observe();
  await peer.act({keys:[],duration_ms:800});
  await peer.act({keys,duration_ms:600},{name:'sprite-tree-intact',when:v=>v.lifecycle.alive&&v.game_ms>=start.game_ms+1000});
  const stopped=await peer.observe();
  const projection=pos=>(pos.x-tree.pos.x)*dx+(pos.y-tree.pos.y)*dy;
  game.check('selected tree has normal 100 HP',tree.hp===100);
  game.check('native movement approaches tree',projection(stopped.position)>projection(start.position)+.05);
  game.check('intact tree blocks crossing',projection(stopped.position)<-.3);
  await peer.act({keys:[],target:{entity:tree.id,camera_angle:7*Math.PI/4,fire:true},duration_ms:1000});
  const dug=await map();
  game.check('ordinary native wand fire destroys tree',!dug.entities.some(e=>e.id===tree.id));
  game.check('destroyed tree cell becomes walkable',!dug.blocked.some(([x,y])=>x===Math.floor(tree.pos.x)&&y===Math.floor(tree.pos.y)));
  await peer.act({keys,duration_ms:200});
  const crossed=await peer.observe();
  game.check('native movement enters cleared tree cell',projection(crossed.position)>-.3);
  await peer.act({keys:[],duration_ms:800},{name:'sprite-tree-cleared',when:v=>v.lifecycle.alive});
  game.check('same prepared character survives tree traversal',crossed.lifecycle.alive&&crossed.session.character===start.session.character);
  await writeFile(join(root,'tree-observations.json'),JSON.stringify({initial,tree,dx,dy,start,stopped,dug,crossed},null,2));
}
