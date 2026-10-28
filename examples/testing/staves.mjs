import {writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import assert from 'node:assert/strict';
const root=process.argv[process.argv.indexOf('--run-dir')+1];
const groups=[
  [0x30e,[0xa97,0xa98,0xa99,0xa9a,0xa9b,0xa9c]],
  [0x323,[0xa9d,0xa9e,0xa9f,0xaa0,0xaa1,0xaa2]],
  [0x321,[0xb08,0xb09,0xb0a,0xc03,0xc15,0xc24]],
  [0x31e,[0xa00,0xa97]],
];
const selected=process.env.OPENROTMG_STAFF_GROUP;
assert(['0','1','2','3'].includes(selected),'Set OPENROTMG_STAFF_GROUP: 0=Wizard, 1=Mystic, 2=Necromancer, 3=Knight refusal');
const [classId,items]=groups[Number(selected)];
export const fixture={class:classId,level:20,seed:873,world:-2,position:'spawn',equipment:[items[0],-1,-1,-1],backpack:[...items.slice(1),0xa04,...Array(8-items.length).fill(-1)],stats:[600,200,50,25,30,50,30,30]};
export async function run(game){
  const observations=[];
  const equip=slot=>game.act({keys:[],duration_ms:350,drag:{from:{x:634+44*(slot%4),y:428+44*Math.floor(slot/4)},to:{x:634,y:385}}});
  await game.act({keys:[],duration_ms:600});
  const initial=await game.observe();
  await equip(classId===0x31e?0:items.length-1);
  const denied=await game.observe();
  game.check('class '+classId+' rejects incompatible weapon atomically',JSON.stringify(denied.character.items)===JSON.stringify(initial.character.items)&&denied.character.inventory_revision===initial.character.inventory_revision);
  if(classId===0x31e)return;
  for(const [i,item] of items.entries()){
    if(i){
      const before=await game.observe();await equip(i-1);
      const equipped=await game.observe();
      game.check(item.toString(16)+' swaps once through native inventory',equipped.character.items[0]===item&&equipped.character.items[3+i]===before.character.items[0]&&equipped.character.inventory_revision===before.character.inventory_revision+1);
    }
    await game.act({keys:[],duration_ms:700});
    const before=await game.inspect();
    await game.act({keys:[],mouse:{x:480,y:325},duration_ms:item===0xc24?3500:2200},i===0||[0xc03,0xc15,0xc24].includes(item)?{name:'staff-'+item.toString(16),when:view=>view.verification.shots>before.diagnostic.character.shots}:undefined);
    const after=await game.inspect(),observed=await game.observe(),shots=after.diagnostic.character.shots-before.diagnostic.character.shots;
    const pellets=item===0xc03?10:item===0xc24?1:2;
    game.check(item.toString(16)+' admits whole native volleys',shots>=pellets&&shots%pellets===0);
    game.check(item.toString(16)+' preserves stats mana inventory and living identity',JSON.stringify(observed.character.stats)===JSON.stringify(initial.character.stats)&&after.view.character.mp===before.view.character.mp&&JSON.stringify(after.view.character.items)===JSON.stringify(before.view.character.items)&&after.view.lifecycle.alive&&after.view.session.character===initial.session.character);
    observations.push({item,shots,before,after});
    await writeFile(join(root,'staves.json'),JSON.stringify(observations,null,2));
  }
}
