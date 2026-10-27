import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
import assert from 'node:assert/strict';
const root=process.argv[process.argv.indexOf('--run-dir')+1];
export const fixture={class:0x310,level:20,seed:766,world:'pirate-cave',position:'spawn',equipment:[-1,0xc09,-1,-1],backpack:[0xa50,0xa51,-1,-1,-1,-1,-1,-1],stats:[670,385,50,0,50,50,40,75]};
export async function run(game){
  const config=JSON.parse(await readFile(join(root,'native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const observations=[];
  async function prepare(actor,change){
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor,change}}),signal:AbortSignal.timeout(10000)});
    const receipt=await response.json();assert(response.ok&&receipt.status==='succeeded',JSON.stringify(receipt));
  }
  const initial=await game.observe();
  const ally=await game.join({...fixture,class:0x31d,world:initial.session.world,position:{x:initial.position.x,y:initial.position.y-2},equipment:[-1,0xb29,-1,-1],backpack:[0xa50,0xa51,0xc41,-1,-1,-1,-1,-1],stats:null});
  const bit=index=>1<<(index-1),negative=bit(9)|bit(10),speedy=bit(15),berserk=bit(20),hexed=bit(28);
  async function snapshot(name){
    const view={name,caster:await game.observe(),ally:await ally.observe()};observations.push(view);return view;
  }
  async function cast(name){
    await prepare(game.actor,{kind:'vitals',hp:100,mp:300});
    await game.act({keys:[],duration_ms:600});
    const before=await snapshot(name+'-before');
    await game.act({keys:[' '],duration_ms:250},{name,when:view=>view.character.mp<250});
    const after=await snapshot(name+'-after');
    game.check(name+' spends one 100 MP cost',after.caster.character.mp>=before.caster.character.mp-100&&after.caster.character.mp<before.caster.character.mp-90);
    return after;
  }
  for(const player of [game,ally]){
    await player.act({keys:['1'],duration_ms:150});
    await player.act({keys:['2'],duration_ms:150});
    const view=await player.observe();
    game.check('native consumables apply Drunk and Hallucinating to '+player.actor,(view.character.conditions&negative)===negative&&view.character.items[4]===-1&&view.character.items[5]===-1);
  }
  let after=await cast('purify-consumables');
  game.check('Purification removes both effects from caster and nearby native ally',!(after.caster.character.conditions&negative)&&!(after.ally.character.conditions&negative));
  game.check('Purification heals its caster',after.caster.character.hp>=350&&after.caster.character.hp<360);
  await ally.act({keys:[' '],duration_ms:150});
  let buffed=await snapshot('helmet-buffs');
  game.check('ordinary Warrior helmet applies Speedy and Berserk',(buffed.ally.character.conditions&(speedy|berserk))===(speedy|berserk));
  after=await cast('purify-positive-buffs');
  game.check('unhexed Speedy and Berserk survive Purification',(after.ally.character.conditions&(speedy|berserk))===(speedy|berserk));
  await ally.act({keys:['3'],duration_ms:150});
  buffed=await snapshot('transformation');
  game.check('ordinary Transformation Potion applies Hexed while buffs remain',(buffed.ally.character.conditions&(hexed|speedy|berserk))===(hexed|speedy|berserk)&&buffed.ally.character.items[6]===-1);
  // Keep conditional Speedy removal separate: selected 123.5.1 0xc09, https://github.com/OpenRotMG/Open-RotMG-Server/blob/127784905114f1b50292f4dec6eab5abefc7858a/data/objects.xml#L7560
  after=await cast('purify-hexed-speedy');
  game.check('Hexed and associated Speedy are removed but Berserk survives',!(after.ally.character.conditions&(hexed|speedy))&&Boolean(after.ally.character.conditions&berserk));
  const retained=after.caster;
  await game.act({keys:['Insert'],duration_ms:250});
  after=await snapshot('native-nexus-reconnect');
  game.check('ordinary Nexus escape reconnects the same living Priest',after.caster.world==='Nexus'&&after.caster.lifecycle.alive&&after.caster.session.character===retained.session.character);
  game.check('reconnect retains tome and consumed inventory',JSON.stringify(after.caster.character.items)===JSON.stringify(retained.character.items)&&after.caster.character.inventory_revision===retained.character.inventory_revision);
  await writeFile(join(root,'purification-observations.json'),JSON.stringify({preparedClassLevelGearVitals:true,conditionsFromNativeItems:true,observations},null,2));
}
