import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';

const kind=Number(process.env.SPRITE_KIND);
assert(Number.isInteger(kind)&&kind>=0xd00&&kind<=0xd04,'Set SPRITE_KIND to an ordinary Native Sprite type (0xd00..0xd04)');
export const fixture={class:0x310,level:20,seed:12351,world:'sprite-world',worldSeed:1,position:'spawn',equipment:[0xa87,-1,0xb05,0xacd],backpack:Array(8).fill(-1),stats:[670,385,50,25,50,50,40,75]};
export async function run(game){
  const root=join(process.env.QA_RESULTS_DIR,'native');
  const config=JSON.parse(await readFile(join(root,'native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const initial=await game.inspect();
  const response=await fetch(config.origin+'/dev/map?player='+initial.diagnostic.player,{headers:{Authorization:'Bearer '+token},signal:AbortSignal.timeout(10000)});
  assert(response.ok);const layout=await response.json();
  const ordinary=layout.entities.filter(e=>e.kind>=0xd00&&e.kind<=0xd04);
  await writeFile(join(root,'ordinary-placement.json'),JSON.stringify({source:initial,ordinary},null,2));
  const sprite=ordinary.find(e=>e.kind===kind);assert(sprite,'Selected ordinary Sprite is absent from the generated map');
  game.check('generated ordinary Sprite starts at selected 100 HP',sprite.hp===100);
  const blocked=new Set(layout.blocked.map(p=>p.join(',')));
  const position=[{x:sprite.pos.x+1.5,y:sprite.pos.y},{x:sprite.pos.x-1.5,y:sprite.pos.y}].find(p=>!blocked.has([Math.floor(p.x),Math.floor(p.y)].join(',')));
  assert(position,'Generated ordinary Sprite needs a walkable prepared approach');
  const {worldSeed,...prepared}=fixture;
  const peer=await game.join({...prepared,seed:12352,world:initial.view.session.world,position});
  const start=await peer.inspect(),arrival=await peer.observe();
  game.check('native client observes the naturally generated ordinary Sprite',start.view.objects.some(o=>o.id===sprite.id&&o.kind===kind));
  await peer.act({keys:[],duration_ms:1000},{name:'sprite-ordinary-present',when:v=>v.lifecycle.alive&&v.game_ms>=arrival.game_ms+600});
  const before=await peer.inspect();
  await peer.act({keys:[],target:{entity:sprite.id,camera_angle:7*Math.PI/4,fire:true},duration_ms:1000});
  const defeated=await peer.inspect();
  await writeFile(join(root,'ordinary-combat.json'),JSON.stringify({sprite,position,start,before,defeated},null,2));
  game.check('ordinary native wand fire defeats selected Sprite and earns XP',!defeated.diagnostic.entities.some(e=>e.id===sprite.id)&&defeated.view.character.experience>before.view.character.experience);
  await peer.act({keys:[],duration_ms:800},{name:'sprite-ordinary-defeated',when:v=>v.lifecycle.alive});
  const end=await peer.observe();
  game.check('same prepared character survives the ordinary encounter',end.lifecycle.alive&&end.session.character===start.view.session.character);
}
