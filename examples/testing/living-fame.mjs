import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
export const fixture={class:0x30e,level:20,seed:778,world:'sprite-world',worldSeed:1,
  position:world=>{const boss=world.encounter_markers.find(e=>e.kind===0xd06);if(!boss)throw new Error('Limon missing');return{x:boss.position.x-4,y:boss.position.y};},
  equipment:[0xb08,0xa2e,0xb05,-1],backpack:[0xc42,0xc42,-1,-1,-1,-1,-1,-1],stats:[670,385,75,25,50,75,40,60]};
export async function run(game){
  const root=process.argv[process.argv.indexOf('--run-dir')+1];
  const config=JSON.parse(await readFile(join(root,'native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const results=[];
  for(const level of [20,15]){
    if(level===15)game=await game.join({...fixture,level});
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invincible',duration_ms:60000}}}),signal:AbortSignal.timeout(10000)});
    const receipt=await response.json();if(!response.ok||receipt.status!=='succeeded')throw new Error(JSON.stringify(receipt));
    await game.act({keys:[],duration_ms:1800});
    await game.act({keys:[],duration_ms:600},{name:'fame-level-'+level,when:()=>true});
    const initial=await game.inspect();
    game.check('prepared level '+level+' has ordinary conversion',initial.view.character.fame===Math.floor(initial.view.character.experience/1000));
    await game.act({keys:['1'],duration_ms:300});
    const used=await game.inspect();
    game.check('XP Booster eligibility at level '+level,used.view.character.items[4]===(level===20?0xc42:-1)&&used.diagnostic.character.xp_boost.active===(level<20));
    await game.act({keys:['2'],duration_ms:300});
    const denied=await game.inspect();
    game.check('second booster is not consumed at level '+level,denied.view.character.items[5]===0xc42&&denied.view.character.inventory_revision===used.view.character.inventory_revision);
    const boss=denied.diagnostic.entities.find(e=>e.kind===0xd06);if(!boss)throw new Error('Visible Limon missing');
    for(let n=0;n<24;n++){
      if(!(await game.inspect()).diagnostic.entities.some(e=>e.id===boss.id))break;
      await game.act({keys:[],duration_ms:800,target:{entity:boss.id,camera_angle:7*Math.PI/4,fire:true}});
    }
    await game.act({keys:[],duration_ms:1200},{name:'fame-earned-'+level,when:()=>true});
    const after=await game.inspect(),ch=after.diagnostic.character;
    results.push({level,initial,used,denied,after});
    await writeFile(join(process.env.QA_RESULTS_DIR,'fame-observations.json'),JSON.stringify(results,null,2));
    game.check('native Limon defeat awards committed XP at level '+level,!after.diagnostic.entities.some(e=>e.id===boss.id)&&ch.exp>initial.diagnostic.character.exp);
    game.check('living Fame retains fractional XP carry at level '+level,ch.fame===Math.floor((ch.exp-ch.xp_boost.bonus_exp)/1000));
    if(level===15){
      game.check('booster doubles the earned XP but not Fame',ch.exp-initial.diagnostic.character.exp===2*ch.xp_boost.bonus_exp&&ch.fame<Math.floor(ch.exp/1000));
    }
    game.check('same living character and booster inventory retained at level '+level,after.view.lifecycle.alive&&after.view.session.character===initial.view.session.character&&after.view.character.items[5]===0xc42);
  }
}
