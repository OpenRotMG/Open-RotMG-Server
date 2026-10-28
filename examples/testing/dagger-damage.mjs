import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import assert from 'node:assert/strict';
const root=process.argv[process.argv.indexOf('--run-dir')+1];
export const fixture={class:0x300,level:20,seed:870,world:'undead-lair',worldSeed:12351,position:'spawn',equipment:[0xa14,-1,-1,-1],backpack:[0xa17,0xc0a,0xb21,0xc22,0xc29,-1,-1,-1],stats:[720,252,50,40,50,50,40,50]};
export async function run(game){
  const config=JSON.parse(await readFile(join(root,'native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const initial=await game.inspect();
  const response=await fetch(config.origin+'/dev/map?player='+initial.diagnostic.player,{headers:{Authorization:'Bearer '+token},signal:AbortSignal.timeout(10000)});assert(response.ok);
  const boss=(await response.json()).entities.find(e=>e.kind===0xd90);assert(boss);
  const {worldSeed,...rogue}=fixture;
  game=await game.join({...rogue,world:initial.view.session.world,position:{x:boss.pos.x-1,y:boss.pos.y}});
  await game.act({keys:[],duration_ms:600});
  const start=await game.observe(),observations=[];
  for(const [i,item] of [0xa14,0xa17,0xc0a,0xb21,0xc22,0xc29].entries()){
    if(i){
      await game.act({keys:[],duration_ms:350,drag:{from:{x:634+44*((i-1)%4),y:428+44*Math.floor((i-1)/4)},to:{x:634,y:385}}});
      game.check(item.toString(16)+' equips for combat',(await game.observe()).character.items[0]===item);
    }
    let before=await game.inspect(),target=before.diagnostic.entities.find(e=>e.id===boss.id);assert(target);
    for(let wait=0;(before.view.objects.find(e=>e.id===boss.id).condition_mask&(1<<23))&&wait<12;wait++){
      await game.act({keys:[],duration_ms:500});before=await game.inspect();target=before.diagnostic.entities.find(e=>e.id===boss.id);assert(target);
    }
    await game.act({keys:[],target:{entity:boss.id,camera_angle:7*Math.PI/4,fire:true},duration_ms:500},{name:'damage-'+item.toString(16),when:view=>view.verification.shots>before.diagnostic.character.shots});
    const after=await game.inspect(),enemy=after.diagnostic.entities.find(e=>e.id===boss.id);assert(enemy);
    observations.push({item,before,after});
    await writeFile(join(root,'dagger-damage.json'),JSON.stringify(observations,null,2));
    game.check(item.toString(16)+' damages the same natural enemy',enemy.hp<target.hp&&after.diagnostic.character.shots>before.diagnostic.character.shots);
    game.check(item.toString(16)+' firing preserves MP inventory and living Rogue',after.view.character.mp===before.view.character.mp&&JSON.stringify(after.view.character.items)===JSON.stringify(before.view.character.items)&&after.view.lifecycle.alive&&after.view.session.character===start.session.character);
  }
}
