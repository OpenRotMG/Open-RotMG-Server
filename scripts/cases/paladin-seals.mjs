import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
import assert from 'node:assert/strict';
const root=process.argv[process.argv.indexOf('--run-dir')+1];
export const fixture={class:0x31f,level:20,seed:769,world:-2,position:'spawn',equipment:[0xa00,0xb26,-1,-1],backpack:[0xa52,0xc06,-1,-1,-1,-1,-1,-1],stats:[770,252,50,0,50,50,40,75]};
export async function run(game){
  const config=JSON.parse(await readFile(join(root,'native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const observations=[];
  async function vitals(actor,hp,mp){
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor,change:{kind:'vitals',hp,mp}}}),signal:AbortSignal.timeout(10000)});
    const receipt=await response.json();assert(response.ok&&receipt.status==='succeeded',JSON.stringify(receipt));
  }
  const initial=await game.observe();
  const ally=await game.join({...fixture,world:initial.session.world,position:{x:initial.position.x+2,y:initial.position.y},equipment:[-1,0xa52,-1,-1],backpack:Array(8).fill(-1)});
  const buffs=(1<<17)|(1<<18),invulnerable=1<<24;
  async function save(value){observations.push(value);await writeFile(join(root,'seal-observations.json'),JSON.stringify({preparedVitals:true,observations},null,2));return value;}
  async function sample(name){return save({name,caster:await game.observe(),ally:await ally.observe()});}
  async function cast(name,cost,prayer=false){
    await vitals(game.actor,100,200);await vitals(ally.actor,100,200);
    await game.act({keys:[],duration_ms:100});const before=await sample(name+'-before');
    let caster,other;
    await game.act({keys:[' '],duration_ms:250},{name,when:view=>{
      if(view.character.mp>=before.caster.character.mp-cost/2||(view.character.conditions&buffs)!==buffs||prayer&&!(view.character.conditions&invulnerable))return false;
      caster=view;other=ally.observe();return true;
    }});
    const during=await save({name:name+'-during',caster,ally:await other}),after=await sample(name);
    const regenerated=(after.caster.game_ms-before.caster.game_ms)/1000*(0.6+after.caster.character.stats.wisdom/16.7);
    game.check(name+' spends one mana cost',after.caster.character.mp>=before.caster.character.mp-cost&&after.caster.character.mp<=before.caster.character.mp-cost+Math.ceil(regenerated)+2);
    for(const role of ['caster','ally']){
      game.check(name+' buffs '+role,(during[role].character.conditions&buffs)===buffs);
      game.check(name+' has no max-HP buff '+role,during[role].character.stats.max_hp===770);
    }
    if(prayer)game.check('Prayer grants only caster Invulnerable',Boolean(during.caster.character.conditions&invulnerable)&&!(during.ally.character.conditions&invulnerable));
    return after;
  }
  const rateBefore=await cast('t6-seal',95);
  await game.act({keys:[],duration_ms:100},{name:'healing-rate',when:view=>view.game_ms>rateBefore.caster.game_ms});
  const rateAfter=await sample('rate-after');
  for(const role of ['caster','ally']){
    const actual=(rateAfter[role].character.hp-rateBefore[role].character.hp)*1000/(rateAfter[role].game_ms-rateBefore[role].game_ms);
    const expected=28+1+rateAfter[role].character.stats.vitality/8;
    game.check('Healing adds 28 HP/s above natural regeneration '+role,(rateAfter[role].character.conditions&buffs)===buffs&&Math.abs(actual-expected)<1);
  }
  async function equip(slot,item){
    await game.act({keys:[],duration_ms:350,drag:{from:{x:634+44*slot,y:428},to:{x:678,y:385}}});
    game.check('native inventory equipped '+item,(await game.observe()).character.items[1]===item);
  }
  await equip(0,0xa52);await cast('t0-seal',70);
  await equip(1,0xc06);const prayer=await cast('blasphemous-prayer',100,true);
  await game.act({keys:[],duration_ms:1800});const unprotected=await sample('invulnerability-expired');
  game.check('Prayer invulnerability expires',unprotected.caster.game_ms>=prayer.caster.game_ms+1400&&!(unprotected.caster.character.conditions&invulnerable));
  await game.act({keys:[],duration_ms:10000});const expired=await sample('auras-expired');
  game.check('both auras expire',!(expired.caster.character.conditions&buffs)&&!(expired.ally.character.conditions&buffs));
  await vitals(game.actor,100,0);await game.act({keys:[' '],duration_ms:250});const denied=await sample('no-mana');
  game.check('insufficient mana gives no aura or invulnerability',!(denied.caster.character.conditions&(buffs|invulnerable))&&!(denied.ally.character.conditions&buffs));
  game.check('same living Paladin retains equipment',denied.caster.lifecycle.alive&&denied.caster.session.character===initial.session.character&&denied.caster.character.items[1]===0xc06&&denied.caster.character.items[4]===0xb26&&denied.caster.character.items[5]===0xa52);
}
