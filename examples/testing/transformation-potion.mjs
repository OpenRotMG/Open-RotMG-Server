import {writeFile} from 'node:fs/promises';
import {join} from 'node:path';

export const fixture={class:0x300,level:20,seed:888,world:'mad-lab',worldSeed:12351,position:{x:73.5,y:15.5},equipment:[0xa14,0xb27,-1,-1],backpack:[0xc41,0xc41,0xc41,0xc41,-1,-1,-1,-1],stats:null};
const HEXED=1<<27,INVISIBLE=1<<12,SPEEDY=1<<14,STUNNED=1<<6;
export async function run(game){
  const samples=[];
  async function save(name){const state=await game.inspect(),view=await game.observe();samples.push({name,state,view});await writeFile(join(process.env.QA_RESULTS_DIR,'transformation.json'),JSON.stringify(samples,null,2));return view;}
  async function use(key,name){
    const before=await game.observe();
    await game.act({keys:[key],duration_ms:800},{name,when:v=>v.character.inventory_revision===before.character.inventory_revision+1});
    const after=await save(name);
    game.check(name+' consumes exactly one potion/revision',after.character.inventory_revision===before.character.inventory_revision+1&&after.character.items[3+Number(key)]===-1);
    game.check(name+' preserves stats and spends no mana',JSON.stringify(after.character.stats)===JSON.stringify(before.character.stats)&&after.character.mp>=before.character.mp);
    return after;
  }
  async function walk(axis,target){
    for(let n=0;n<30;n++){
      const value=(await game.observe()).position[axis];
      if(Math.abs(value-target)<.65)return;
      const keys=axis==='x'?(value<target?['d','s']:['a','w']):(value<target?['a','s']:['d','w']);
      await game.act({keys,duration_ms:150});
    }
    throw Error('Native walk did not reach '+axis+'='+target);
  }
  const before=await save('initial');
  await game.act({keys:[],duration_ms:1000},{name:'transformation-before',when:v=>!(v.character.conditions&(HEXED|INVISIBLE))});
  await game.act({keys:[' '],duration_ms:200});
  game.check('ordinary cloak applies Invisible',Boolean((await game.observe()).character.conditions&INVISIBLE));
  const useStarted=(await game.observe()).game_ms;
  const transformed=await use('1','transformation-hexed');
  game.check('potion replaces Invisible with Hexed without Speedy or Stunned',(transformed.character.conditions&(HEXED|INVISIBLE|SPEEDY|STUNNED))===HEXED);
  await game.act({keys:['1'],duration_ms:200});
  game.check('empty slot cannot consume again',(await game.observe()).character.inventory_revision===transformed.character.inventory_revision);
  await game.act({keys:[],mouse:{x:500,y:300},duration_ms:800});
  game.check('potion Hexed alone still permits native weapon fire',(await game.observe()).verification.shots>transformed.verification.shots);
  // Bracket application by observed clocks, not screenshot completion: https://openrotmg.org/restoration/reference-data/client-123.5.1.swf
  let early=false,late=false;
  while((await game.observe()).game_ms<transformed.game_ms+31000){
    await game.act({keys:[],duration_ms:500});
    const view=await game.observe(),hexed=Boolean(view.character.conditions&HEXED);
    samples.push({name:'timer',view});
    early ||= view.game_ms<useStarted+30000&&!hexed;
    late ||= view.game_ms>=transformed.game_ms+30000&&hexed;
  }
  game.check('Hexed expiry stays inside the observed 30-second application bracket',!early&&!late);
  await game.act({keys:[],duration_ms:800},{name:'transformation-expired',when:v=>!(v.character.conditions&HEXED)});
  game.check('Hexed expires after the selected 30 seconds',!((await save('expired')).character.conditions&HEXED));
  game.check('second potion applies Hexed before the blue cure',Boolean((await use('2','transformation-second')).character.conditions&HEXED));
  await walk('x',75.5);await walk('y',12.5);
  await game.act({keys:[],duration_ms:800},{name:'transformation-blue',when:v=>!(v.character.conditions&HEXED)});
  const blue=await save('blue');
  game.check('ordinary blue-pool entry clears potion Hexed before expiry',!(blue.character.conditions&HEXED)&&blue.position.x>74&&blue.position.y<14);
  const onBlue=await use('3','transformation-on-blue');
  game.check('potion on blue is consumed but immediately cured',!(onBlue.character.conditions&HEXED));
  await walk('y',15.5);
  await use('4','transformation-before-travel');
  game.check('dry-ground use transforms again',Boolean((await game.observe()).character.conditions&HEXED));
  const retained=await game.observe();
  await game.act({keys:['Insert'],duration_ms:250});
  const returned=await save('nexus');
  game.check('Nexus escape retains living identity and consumed inventory',returned.world==='Nexus'&&returned.lifecycle.alive&&returned.session.character===before.session.character&&JSON.stringify(returned.character.items)===JSON.stringify(retained.character.items)&&returned.character.inventory_revision===retained.character.inventory_revision);
  game.check('world-local Hexed does not survive Nexus reconnect',!(returned.character.conditions&HEXED));
}
