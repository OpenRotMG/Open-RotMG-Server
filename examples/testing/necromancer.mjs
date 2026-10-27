import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
import assert from 'node:assert/strict';
const root=process.argv[process.argv.indexOf('--run-dir')+1];
export const fixture={class:0x321,level:20,seed:771,world:'kitchen',position:{x:13.5,y:40.5},equipment:[-1,0xaaa,-1,-1],backpack:[0xb2b,0xc16,0xb05,0xa4b,-1,-1,-1,-1],stats:[670,385,60,25,50,60,30,75]};
export async function run(game){
  const config=JSON.parse(await readFile(join(root,'native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  async function prepare(actor,change){
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor,change}}),signal:AbortSignal.timeout(10000)});
    const receipt=await response.json();assert(response.ok&&receipt.status==='succeeded',JSON.stringify(receipt));
  }
  const initial=await game.inspect();
  const ally=await game.join({...fixture,class:0x30e,world:initial.view.session.world,position:{x:14.5,y:40.5},equipment:[-1,-1,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,60,25,50,60,30,60]});
  const observations=[];
  async function save(name){const state=await game.inspect();await writeFile(join(root,name+'.json'),JSON.stringify(state,null,2));return state;}
  async function cast(name,cost){
    for(const actor of [game.actor,ally.actor])await prepare(actor,{kind:'vitals',hp:100,mp:350});
    await game.act({keys:[],duration_ms:600});
    const visible=await game.inspect(),boss=visible.diagnostic.entities.find(e=>e.kind===0x926);
    game.check(name+' boss visible',!!boss);
    await game.act({keys:[],duration_ms:150,target:{entity:boss.id,camera_angle:-Math.PI/4,fire:false}});
    const before=await save(name+'-before'),otherBefore=await ally.observe();
    await game.act({keys:[' '],duration_ms:300},{name,when:view=>view.character.mp<before.view.character.mp-cost/2});
    const after=await save(name+'-after'),otherAfter=await ally.observe();
    const target=after.diagnostic.entities.find(e=>e.id===boss.id);
    game.check(name+' native cast damages boss',!!target&&target.hp<boss.hp);
    const remaining=new Map(after.diagnostic.entities.map(e=>[e.id,e.hp]));
    const damage=before.diagnostic.entities.reduce((sum,e)=>sum+Math.max(0,e.hp-(remaining.has(e.id)?remaining.get(e.id):0)),0);
    observations.push({name,damage,before,after,otherBefore,otherAfter});
    await writeFile(join(root,'drain-observations.json'),JSON.stringify({preparedVitals:true,observations},null,2));
    game.check(name+' spends one mana cost',after.view.character.mp>=before.view.character.mp-cost&&after.view.character.mp<before.view.character.mp-cost+15);
    game.check(name+' heals caster by observed damage',after.view.character.hp>=before.view.character.hp+damage-10&&after.view.character.hp<before.view.character.hp+damage+15);
    game.check(name+' heals nearby native ally by observed damage',otherAfter.character.hp>=otherBefore.character.hp+damage-10&&otherAfter.character.hp<otherBefore.character.hp+damage+15);
  }
  await cast('necrotic',40);
  async function equip(slot,item,to={x:678,y:385}){
    await game.act({keys:[],duration_ms:400,drag:{from:{x:634+44*(slot%4),y:428+44*Math.floor(slot/4)},to}});
    game.check('native inventory equips '+item,(await game.observe()).character.items.includes(item));
  }
  await equip(0,0xb2b);game.check('T6 equipped',(await game.observe()).character.items[1]===0xb2b);
  await cast('bloodsucker',140);
  await equip(1,0xc16);game.check('Crystal equipped',(await game.observe()).character.items[1]===0xc16);
  await cast('crystal',140);
  await prepare(game.actor,{kind:'vitals',hp:100,mp:0});
  const poor=await save('poor');await game.act({keys:[' '],duration_ms:250});const denied=await save('denied');
  const boss=poor.diagnostic.entities.find(e=>e.kind===0x926);
  game.check('insufficient mana does not drain',denied.diagnostic.entities.find(e=>e.id===boss.id).hp===boss.hp&&denied.view.character.hp<poor.view.character.hp+15);
  await equip(2,0xb05,{x:722,y:385});const robed=await game.observe();
  game.check('robe adds selected defense and MP',robed.character.items[2]===0xb05&&robed.character.stats.max_mp===440&&robed.character.stats.defense===38);
  await game.act({keys:[],duration_ms:400,drag:{from:{x:766,y:428},to:{x:634,y:385}}});
  game.check('Necromancer rejects wand and retains inventory',JSON.stringify((await game.observe()).character.items)===JSON.stringify(robed.character.items));
  game.check('same living Necromancer retained',(await game.observe()).session.character===initial.view.session.character&&(await game.observe()).lifecycle.alive);
  await writeFile(join(root,'drain-observations.json'),JSON.stringify({preparedVitals:true,observations,poor,denied},null,2));
}
