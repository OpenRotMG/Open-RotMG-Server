import {writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
export const fixture={class:0x30e,level:1,seed:765,world:-2,position:'spawn',equipment:[0xa97,0xa2e,-1,-1],backpack:[0xb24,0xc0c,0xa4b,0xb05,0xa35,0xa35,-1,-1],stats:null};
export async function run(game){
  let serial=0;
  const save=async name=>{const state=await game.inspect();await writeFile(join(process.env.QA_RESULTS_DIR,name+'.json'),JSON.stringify(state,null,2));return state;};
  async function prepare(change){
    const request=join(process.env.QA_RESULTS_DIR,'preparation-'+serial+'.json');
    await writeFile(request,JSON.stringify({id:'wizard-preparation-'+serial++,command:{op:'cheat',actor:game.actor,change}}));
    execFileSync(process.execPath,[resolve('scripts/game.mjs'),join(process.env.QA_RESULTS_DIR,'native/native.json'),request]);
  }
  await game.act({keys:[],duration_ms:100});
  const initial=await save('wizard-before');
  game.check('prepared level-one Wizard loads through native guest menu',initial.view.character.class===0x30e&&initial.view.character.level===1);
  for(const [item,cost] of [[0xa2e,30],[0xb24,80],[0xc0c,75]]){
    if(item!==0xa2e)await prepare({kind:'item',slot:1,item});
    await prepare({kind:'vitals',hp:100,mp:100});
    await game.act({keys:[],mouse:{x:400,y:300},duration_ms:150});
    await game.act({keys:[],duration_ms:1000});
    const before=await save('before-'+item);
    await game.act({keys:[' '],duration_ms:150});
    const after=await save('cast-'+item),shots=after.diagnostic.own_projectiles.filter(p=>p.id>=128);
    game.check(item+': native cast spends one spell cost',after.view.character.mp<=before.view.character.mp-cost+2);
    game.check(item+': twenty nova shots recorded before cover removes bullets',after.diagnostic.character.shots-before.diagnostic.character.shots===20);
    game.check(item+': surviving nova bullets share cursor origin and 18-degree rays',shots.length>0&&shots.every(p=>Math.abs(p.angle*10/Math.PI-Math.round(p.angle*10/Math.PI))<0.00001&&p.start.x===shots[0].start.x&&p.start.y===shots[0].start.y));
    game.check(item+': cursor target differs from caster',Math.hypot(shots[0].start.x-before.view.position.x,shots[0].start.y-before.view.position.y)>1);
    await game.act({keys:[],duration_ms:1000},{name:'wizard-'+item,when:()=>true});
    if(item!==0xa2e){
      const low=await save('low-mana-'+item);
      await game.act({keys:[' '],duration_ms:150});
      const denied=await save('denied-'+item);
      game.check(item+': insufficient mana creates no nova and spends no mana',denied.diagnostic.character.shots===low.diagnostic.character.shots&&denied.view.character.mp>=low.view.character.mp);
    }
  }
  await game.act({keys:[],duration_ms:500,drag:{from:{x:766,y:428},to:{x:722,y:385}}});
  const robed=await save('robed');
  game.check('native robe equips and applies MP, DEF and Wisdom bonuses',robed.view.character.items[2]===0xb05&&robed.view.character.stats[1]===155&&robed.view.character.stats[3]===13&&robed.view.character.stats[7]===18);
  await game.act({keys:[],duration_ms:500,drag:{from:{x:722,y:428},to:{x:632,y:385}}});
  game.check('Wizard refuses a wand without losing either item',JSON.stringify((await game.inspect()).view.character.items)===JSON.stringify(robed.view.character.items));
  const base=[...robed.view.character.base_stats];base[7]=59;
  await prepare({kind:'stats',values:base});
  await game.act({keys:[],duration_ms:500});
  await game.act({keys:['5'],duration_ms:150});
  const capped=await save('wisdom-capped');
  game.check('native Wisdom potion raises prepared base 59 to 60 once',capped.view.character.base_stats[7]===60&&capped.view.character.items[8]===-1);
  await game.act({keys:['6'],duration_ms:150});
  const final=await save('wizard-after');
  game.check('Wisdom cap refuses another potion without consuming it',final.view.character.base_stats[7]===60&&final.view.character.items[9]===0xa35&&final.view.character.inventory_revision===capped.view.character.inventory_revision);
  game.check('native casts retain the same living Wizard and equipment',final.view.lifecycle.alive&&final.view.session.character===initial.view.session.character&&final.view.character.items[1]===0xc0c);
  game=await game.join({...fixture,level:20,world:'kitchen',position:{x:13.5,y:40.5},equipment:[-1,0xa2e,0xb05,-1],backpack:Array(8).fill(-1),stats:[670,385,75,25,50,75,40,60]});
  const combat=await save('combat-before'),boss=combat.diagnostic.entities.find(e=>e.kind===0x926);
  game.check('ordinary Kitchen boss is visible',!!boss);
  for(let n=0;n<8;n++){
    if(!(await game.inspect()).diagnostic.entities.some(e=>e.id===boss.id))break;
    await game.act({keys:[],duration_ms:150,target:{entity:boss.id,camera_angle:-Math.PI/4,fire:false}});
    await game.act({keys:[' '],duration_ms:600},n===0?{name:'wizard-combat',when:view=>view.character.mp<combat.view.character.mp}:undefined);
  }
  const victory=await save('combat-after');
  game.check('native spell alone defeats Bonegrind and awards XP',!victory.diagnostic.entities.some(e=>e.id===boss.id)&&victory.view.character.experience>combat.view.character.experience);
  game.check('native combat spends mana and preserves the same living Wizard',victory.view.character.mp<combat.view.character.mp&&victory.view.lifecycle.alive&&victory.view.session.character===combat.view.session.character);
}
