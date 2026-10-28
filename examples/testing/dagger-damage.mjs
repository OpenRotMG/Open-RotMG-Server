import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import assert from 'node:assert/strict';
const root=process.argv[process.argv.indexOf('--run-dir')+1];
export const fixture={class:0x300,level:20,seed:870,world:'undead-lair',worldSeed:12351,position:{x:14.5,y:120.5},equipment:[0xa14,-1,-1,-1],backpack:[0xa17,0xc0a,0xb21,0xc22,0xc29,-1,-1,-1],stats:[720,252,50,25,50,50,40,50]};
export async function run(game){
  const config=JSON.parse(await readFile(join(root,'native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const initial=await game.inspect(),boss=initial.diagnostic.entities.find(e=>e.kind===0xd90);assert(boss);
  assert(boss.pos.x===15.5&&boss.pos.y===120.5,'Seeded Septavius placement changed');
  const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:'dagger-damage-protection',command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invincible',duration_ms:60000}}})});
  const protection=await response.json();await writeFile(join(root,'protection.json'),JSON.stringify(protection,null,2));assert(response.ok&&protection.status==='succeeded',JSON.stringify(protection));
  await game.act({keys:[],duration_ms:600});
  const start=await game.observe(),observations=[];
  for(const [i,item] of [0xa14,0xa17,0xc0a,0xb21,0xc22,0xc29].entries()){
    if(i){
      await game.act({keys:[],duration_ms:350,drag:{from:{x:634+44*((i-1)%4),y:428+44*Math.floor((i-1)/4)},to:{x:634,y:385}}});
      game.check(item.toString(16)+' equips for combat',(await game.observe()).character.items[0]===item);
    }
    let before=await game.inspect(),target=before.diagnostic.entities.find(e=>e.id===boss.id);assert(target);
    for(let wait=0;(before.view.objects.find(e=>e.id===boss.id).condition_mask&(1<<24))&&wait<12;wait++){
      await game.act({keys:[],duration_ms:500});before=await game.inspect();target=before.diagnostic.entities.find(e=>e.id===boss.id);assert(target);
    }
    await game.act({keys:[],target:{entity:boss.id,camera_angle:7*Math.PI/4,fire:true},duration_ms:500},{name:'damage-'+item.toString(16),when:view=>view.verification.shots>before.diagnostic.character.shots});
    const after=await game.inspect(),enemy=after.diagnostic.entities.find(e=>e.id===boss.id);assert(enemy);
    const damaged=before.diagnostic.entities.filter(e=>e.kind>=0xd90&&e.kind<=0xdb2&&after.diagnostic.entities.some(a=>a.id===e.id&&a.hp<e.hp));
    observations.push({item,damaged:damaged.map(e=>({id:e.id,kind:e.kind,before:e.hp,after:after.diagnostic.entities.find(a=>a.id===e.id).hp})),before,after});
    await writeFile(join(root,'dagger-damage.json'),JSON.stringify(observations,null,2));
    game.check(item.toString(16)+' damages natural enemies or earns a real kill',after.diagnostic.character.shots>before.diagnostic.character.shots&&(damaged.length>0||after.diagnostic.character.kills>before.diagnostic.character.kills));
    game.check(item.toString(16)+' firing preserves MP inventory and living Rogue',after.view.character.mp===before.view.character.mp&&JSON.stringify(after.view.character.items)===JSON.stringify(before.view.character.items)&&after.view.lifecycle.alive&&after.view.session.character===start.session.character);
  }
}
