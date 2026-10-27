import assert from 'node:assert/strict';
import {writeFile} from 'node:fs/promises';
import {join} from 'node:path';
export const fixture={class:0x31e,level:20,seed:12351,world:'abyss-of-demons',worldSeed:1,position:'spawn',equipment:[0xa00,-1,0xafc,-1],backpack:Array(8).fill(-1),stats:[770,252,50,40,50,50,0,50]};
export async function run(game){
  await game.act({keys:[],duration_ms:1000});
  const before=await game.observe(),detail=await game.inspect(),tiles=detail.view.tiles;
  await writeFile(join(process.env.QA_RESULTS_DIR,'abyss-start.json'),JSON.stringify(detail));
  const tile=(x,y)=>tiles.find(t=>t.x===x&&t.y===y);
  const enemies=detail.view.objects.filter(o=>o.hp>0&&!o.blocks);
  const clearance=o=>Math.min(...enemies.map(e=>Math.hypot(e.position.x-o.position.x,e.position.y-o.position.y)));
  const bridges=detail.view.objects.filter(o=>o.kind===0x153&&tile(Math.floor(o.position.x),Math.floor(o.position.y))?.ground===0x70&&clearance(o)>15).sort((a,b)=>clearance(b)-clearance(a));
  let crossing;
  for(const bridge of bridges){
    for(const [key,dx,dy] of [[['d','s'],1,0],[['a','w'],-1,0],[['a','s'],0,1],[['d','w'],0,-1]]){
      const x=Math.floor(bridge.position.x)+dx,y=Math.floor(bridge.position.y)+dy;
      if(tile(x,y)?.ground===0x70&&!detail.view.objects.some(o=>Math.floor(o.position.x)===x&&Math.floor(o.position.y)===y)){crossing={bridge,key,dx,dy};break;}
    }
    if(crossing)break;
  }
  assert(crossing,'entrance must expose a bridge with an unprotected lava neighbor');
  const bridge=await game.join({...fixture,class:0x31f,worldSeed:undefined,world:before.session.world,seed:12352,position:crossing.bridge.position,equipment:[0xa00,0xc06,0xafc,-1],stats:[770,252,50,25,50,50,0,50]});
  await bridge.act({keys:[],duration_ms:800});
  const protectedBefore=await bridge.observe();
  await bridge.act({keys:[],duration_ms:1000},{name:'abyss-bridge',when:v=>v.game_ms>protectedBefore.game_ms+500});
  const protectedAfter=await bridge.observe();
  game.check('protective floor prevents lava damage',protectedAfter.character.hp>=protectedBefore.character.hp);
  await bridge.act({keys:crossing.key,duration_ms:150});
  const crossed=await bridge.observe();
  await bridge.act({keys:[],mouse:{x:500,y:300},duration_ms:700},{name:'abyss-lava',when:v=>v.character.hp<protectedAfter.character.hp});
  const exposed=await bridge.observe();
  game.check('walking off the bridge restores ground damage',Math.hypot(crossed.position.x-protectedAfter.position.x,crossed.position.y-protectedAfter.position.y)>.5&&exposed.character.hp<protectedAfter.character.hp);
  game.check('ground reports preserve native shot RNG',exposed.verification.shots>crossed.verification.shots&&exposed.lifecycle.alive);
  await bridge.act({keys:[' '],duration_ms:50});
  const sealed=await bridge.observe();
  game.check('ordinary seal applies Invulnerable',Boolean(sealed.character.conditions&(1<<24)));
  await bridge.act({keys:[],duration_ms:200},{name:'abyss-seal',when:v=>v.game_ms>sealed.game_ms+50&&Boolean(v.character.conditions&(1<<24))});
  const sheltered=await bridge.observe();
  game.check('current Kithio-derived invulnerability protects from lava',sheltered.character.hp>=sealed.character.hp&&Boolean(sheltered.character.conditions&(1<<24)));
  await bridge.act({keys:[],duration_ms:1700});
  const expired=await bridge.observe();
  game.check('lava damage resumes after seal expires',!(expired.character.conditions&(1<<24))&&expired.character.hp<sheltered.character.hp);
  game.check('same living native character',expired.lifecycle.alive&&expired.session.character===protectedBefore.session.character);
  await writeFile(join(process.env.QA_RESULTS_DIR,'abyss-observations.json'),JSON.stringify({before,crossing,protectedBefore,protectedAfter,crossed,exposed,sealed,sheltered,expired},null,2));
}
