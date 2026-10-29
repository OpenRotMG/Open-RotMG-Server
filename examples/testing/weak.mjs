import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
const root=join(process.env.QA_RESULTS_DIR,'native'),weak=1<<2;
process.env.RUST_LOG='info,openrotmg::world=debug';
export const fixture={class:0x307,level:20,seed:936,world:'spider-den',worldSeed:12351,position:'spawn',equipment:[0xa1a,0xa61,-1,-1],backpack:[0xc21,-1,-1,-1,-1,-1,-1,-1],stats:[700,252,75,25,50,50,40,50]};
export async function run(game){
  const config=JSON.parse(await readFile(join(root,'native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim(),samples=[];
  async function command(command){
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command}),signal:AbortSignal.timeout(10000)});
    const receipt=await response.json();assert(response.ok,JSON.stringify(receipt));
    assert.equal(receipt.status,'succeeded',JSON.stringify(receipt));return receipt.result;
  }
  const cheat=change=>command({op:'cheat',actor:game.actor,change});
  async function save(name){const state=await game.inspect();samples.push({name,state});await writeFile(join(root,'weak-observations.json'),JSON.stringify(samples,null,2));return state;}
  async function shots(){return (await readFile(join(root,'server.log'),'utf8')).split('\n').slice(0,-1).filter(l=>l.startsWith('{')).map(l=>JSON.parse(l).fields).filter(f=>f.message==='player shot'&&f.player===player);}
  const initial=await save('prepared'),player=initial.view.session.player;
  await cheat({kind:'protection',condition:'invulnerable',duration_ms:60000});
  await game.act({keys:[],duration_ms:2800});
  await game.act({keys:[],duration_ms:1200,mouse:{x:500,y:325}},{name:'weak-normal',when:v=>!(v.character.conditions&weak)});
  await save('normal');const normalShots=await shots();
  game.check('normal bow damage uses Attack 75',normalShots.length>0&&normalShots.every(s=>s.damage>=20&&s.damage<=78));
  await cheat({kind:'teleport',position:{x:75.5,y:16.5}});
  await game.act({keys:[],duration_ms:200});
  const approached=await save('approached'),boss=approached.diagnostic.entities.find(e=>e.kind===0x936);assert(boss);
  const target={entity:boss.id,camera_angle:-Math.PI/4,fire:true};
  const hp=state=>state.diagnostic.entities.find(e=>e.id===boss.id).hp;
  for(let n=0;n<40&&!((await game.observe()).character.conditions&weak);n++)await game.act({keys:[],duration_ms:250});
  const hit=await save('applied');
  game.check('natural Arachna blob applies Weak through Invulnerable',Boolean(hit.view.character.conditions&weak)&&hit.view.character.hp===700);
  let offset=(await shots()).length;
  await game.act({keys:[],duration_ms:1000,target},{name:'weak-weapon',when:v=>Boolean(v.character.conditions&weak)});
  const weakened=await save('weak-weapon'),weakShots=(await shots()).slice(offset);
  game.check('Weak bow shots deal half base damage, not half boosted damage',weakShots.length>0&&weakShots.every(s=>s.damage>=5&&s.damage<=19));
  game.check('Weak shots actually reduce the boss HP',hp(weakened)<hp(hit));
  const paladin=await game.join({class:0x31f,level:20,seed:937,world:hit.view.session.world,position:hit.view.position,equipment:[-1,0xa55,-1,-1],backpack:Array(8).fill(-1),stats:[770,252,50,25,50,50,40,50]});
  await command({op:'cheat',actor:paladin.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}});
  await paladin.act({keys:[' '],duration_ms:200});
  game.check('ordinary allied seal adds Damaging while Weak',((await game.observe()).character.conditions&(weak|(1<<18)))===(weak|(1<<18)));
  offset=(await shots()).length;
  await game.act({keys:[],duration_ms:800,target},{name:'weak-damaging',when:v=>(v.character.conditions&(weak|(1<<18)))===(weak|(1<<18))});
  const damagingShots=(await shots()).slice(offset);
  game.check('Weak overrides native allied Damaging',damagingShots.length>0&&damagingShots.every(s=>s.damage>=5&&s.damage<=19));
  const beforeAbility=await save('weak-damaging');
  offset=(await shots()).length;
  const mana=beforeAbility.view.character.mp;
  await game.act({keys:[' '],duration_ms:300,target:{...target,fire:false}},{name:'weak-quiver',when:v=>Boolean(v.character.conditions&weak)&&v.character.mp<mana});
  await game.act({keys:[],duration_ms:500});
  const ability=await save('weak-quiver'),abilityShots=(await shots()).slice(offset);
  game.check('Weak quiver retains unscaled 100–139 damage and spends mana',abilityShots.length===1&&abilityShots[0].damage>=100&&abilityShots[0].damage<=139&&ability.view.character.mp<mana);
  game.check('quiver actually damages the boss while Weak',hp(beforeAbility)-hp(ability)>=abilityShots[0].damage-4&&Boolean(ability.view.character.conditions&weak));
  const revision=ability.view.character.inventory_revision;
  await game.act({keys:['1'],duration_ms:200});
  const cured=await save('cured');
  game.check('Holy Water removes Weak, preserves Damaging and consumes once',!(cured.view.character.conditions&weak)&&Boolean(cured.view.character.conditions&(1<<18))&&cured.view.character.items[4]===-1&&cured.view.character.inventory_revision===revision+1);
  for(let n=0;n<40&&!((await game.observe()).character.conditions&weak);n++)await game.act({keys:[],duration_ms:250});
  const reapplied=await save('reapplied');
  game.check('cure grants no Weak immunity against the next natural hit',Boolean(reapplied.view.character.conditions&weak));
  await cheat({kind:'teleport',position:initial.view.position});
  await game.act({keys:[],duration_ms:5500},{name:'weak-expired',when:v=>!(v.character.conditions&weak)});
  const expired=await save('expired');
  game.check('Weak expires without further hits and preserves Attack',!(expired.view.character.conditions&weak)&&expired.view.character.stats[2]===75);
  game.check('same living Archer retains bow and quiver',expired.view.lifecycle.alive&&expired.view.session.character===initial.view.session.character&&expired.view.character.items[0]===0xa1a&&expired.view.character.items[1]===0xa61);
}
