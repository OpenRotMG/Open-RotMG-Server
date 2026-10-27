import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
import assert from 'node:assert/strict';
const root=process.argv[process.argv.indexOf('--run-dir')+1];
export const fixture={class:0x30e,level:20,seed:782,world:'pirate-cave',position:'spawn',equipment:[-1,0xa2e,-1,-1],backpack:[0xc3c,0xc3a,0xa22,0xa23,-1,-1,-1,-1],stats:[670,385,50,0,50,50,40,60]};
export async function run(game){
  const config=JSON.parse(await readFile(join(root,'native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const observations=[];
  async function prepare(change){
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:game.actor,change}}),signal:AbortSignal.timeout(10000)});
    const receipt=await response.json();assert(response.ok&&receipt.status==='succeeded',JSON.stringify(receipt));
  }
  async function snapshot(name){const view=await game.observe();observations.push({name,view});return view;}
  async function sample(name,seconds,bonus=0){
    const before=await snapshot(name+'-before');
    for(let n=0;n<seconds;n+=2)await game.act({keys:[],duration_ms:2000},n===0?{name,when:view=>view.game_ms>=before.game_ms+500}:undefined);
    const after=await snapshot(name+'-after'),elapsed=(after.game_ms-before.game_ms)/1000;
    for(const [resource,rate,cap] of [['hp',1+before.character.stats.vitality/8+bonus,before.character.stats.max_hp],['mp',0.6+before.character.stats.wisdom/16.7,before.character.stats.max_mp]]){
      const actual=after.character[resource]-before.character[resource],expected=Math.min(cap-before.character[resource],rate*elapsed);
      game.check(name+' '+resource+' observed rate',Math.abs(actual-expected)<1.1);
    }
    return after;
  }
  await game.act({keys:[],duration_ms:2000});
  await prepare({kind:'vitals',hp:100,mp:100});
  await sample('base-recovery',6);
  await game.act({keys:['1'],duration_ms:200});
  let view=await snapshot('native-vitality-drink');
  game.check('ordinary Lime Jungle Bay raises VIT to 50 and is consumed',view.character.stats.vitality===50&&view.character.items[4]===-1);
  await sample('boosted-recovery',6);
  await prepare({kind:'vitals',hp:669,mp:384});
  view=await sample('resource-caps',4);
  game.check('HP and MP stop at their effective maxima',view.character.hp===670&&view.character.mp===385);
  await game.act({keys:['2'],duration_ms:200});
  view=await snapshot('native-maxima-drink');
  game.check('ordinary Blue Paradise raises both resource ceilings',view.character.stats.max_hp===720&&view.character.stats.max_mp===435&&view.character.items[5]===-1);
  await game.act({keys:['3'],duration_ms:200});
  await game.act({keys:['4'],duration_ms:200});
  view=await snapshot('native-potions');
  game.check('ordinary potions fill boosted resource ceilings',view.character.hp===720&&view.character.mp===435&&view.character.items[6]===-1&&view.character.items[7]===-1);
  for(let n=0;n<11;n++)await game.act({keys:[],duration_ms:2000});
  view=await snapshot('boost-expiry');
  game.check('expiry clamps resources and restores base VIT without changing base stats',view.character.hp===670&&view.character.mp===385&&view.character.stats.max_hp===670&&view.character.stats.max_mp===385&&view.character.stats.vitality===40);
  const ally=await game.join({...fixture,class:0x31f,world:view.session.world,position:{x:view.position.x,y:view.position.y-2},equipment:[-1,0xa54,-1,-1],backpack:Array(8).fill(-1),stats:null});
  await prepare({kind:'vitals',hp:100,mp:100});
  await ally.act({keys:[' '],duration_ms:200});
  view=await snapshot('native-healing-aura');
  game.check('ordinary allied seal applies Healing',Boolean(view.character.conditions&(1<<17)));
  await sample('healing-recovery',4,20);
  const retained=await game.observe();
  await game.act({keys:['Insert'],duration_ms:500});
  view=await snapshot('reconnected');
  game.check('ordinary Nexus escape retains the living character and consumed inventory',view.world==='Nexus'&&view.lifecycle.alive&&view.session.character===retained.session.character&&JSON.stringify(view.character.items)===JSON.stringify(retained.character.items));
  await writeFile(join(root,'recovery-observations.json'),JSON.stringify({preparedLevelGearVitals:true,conditionsNotNativeVerified:['Sick','Quiet','Bleeding'],observations},null,2));
}
