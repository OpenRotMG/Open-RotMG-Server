import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import assert from 'node:assert/strict';
const root=process.argv[process.argv.indexOf('--run-dir')+1];
export const fixture={class:0x31e,level:20,seed:869,world:'undead-lair',worldSeed:12351,position:'spawn',equipment:[0xa00,-1,-1,-1],backpack:[0xb0b,0xa03,0xc01,0xc05,-1,-1,-1,-1],stats:[770,252,50,40,50,50,75,50]};
export async function run(game){
  const config=JSON.parse(await readFile(join(root,'native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const initial=await game.inspect();
  const response=await fetch(config.origin+'/dev/map?player='+initial.diagnostic.player,{headers:{Authorization:'Bearer '+token},signal:AbortSignal.timeout(10000)});assert(response.ok);
  const boss=(await response.json()).entities.find(e=>e.kind===0xd90);assert(boss);
  const {worldSeed,...knight}=fixture;
  game=await game.join({...knight,world:initial.view.session.world,position:{x:boss.pos.x-1,y:boss.pos.y}});
  await game.act({keys:[],duration_ms:600});
  const start=await game.observe(),observations=[];
  for(const [i,item] of [0xa00,0xb0b,0xa03,0xc01,0xc05].entries()){
    if(i){
      await game.act({keys:[],duration_ms:350,drag:{from:{x:634+44*(i-1),y:428},to:{x:634,y:385}}});
      game.check(item.toString(16)+' equips for native combat',(await game.observe()).character.items[0]===item);
    }
    let before=await game.inspect(),target=before.diagnostic.entities.find(e=>e.id===boss.id);assert(target);
    for(let wait=0;(before.view.objects.find(e=>e.id===boss.id).condition_mask&(1<<23))&&wait<12;wait++){
      await game.act({keys:[],duration_ms:500});before=await game.inspect();target=before.diagnostic.entities.find(e=>e.id===boss.id);assert(target);
    }
    await game.act({keys:[],target:{entity:boss.id,camera_angle:7*Math.PI/4,fire:true},duration_ms:500},item===0xc01?{name:'demon-damage',when:view=>view.verification.shots>before.diagnostic.character.shots}:undefined);
    const after=await game.inspect(),enemy=after.diagnostic.entities.find(e=>e.id===boss.id);assert(enemy);
    observations.push({item,before,after});
    await writeFile(join(root,'sword-damage.json'),JSON.stringify(observations,null,2));
    game.check(item.toString(16)+' damages a natural enemy through native fire',enemy.hp<target.hp&&after.diagnostic.character.shots>before.diagnostic.character.shots);
    game.check(item.toString(16)+' leaves mana and inventory unchanged',after.view.character.mp===before.view.character.mp&&JSON.stringify(after.view.character.items)===JSON.stringify(before.view.character.items));
    game.check(item.toString(16)+' retains the same living Knight',after.view.lifecycle.alive&&after.view.session.character===start.session.character);
  }
}
