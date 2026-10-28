import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';

export const fixture={class:0x30e,level:20,seed:897,world:-2,position:'spawn',equipment:[0xa9f,0xa2e,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,50,0,50,50,0,0]};
export async function run(game){
  const root=process.env.QA_RESULTS_DIR,config=JSON.parse(await readFile(join(root,'native','native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim(),samples=[];
  async function vitals(client,hp,mp){
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:client.actor,change:{kind:'vitals',hp,mp}}})});
    const value=await response.json();assert(response.ok&&value.status==='succeeded',JSON.stringify(value));
  }
  async function save(name,client=game){const view=await client.observe();samples.push({name,view});await writeFile(join(root,'nexus-observations.json'),JSON.stringify({preparedStatsGearAndVitals:true,samples},null,2));return view;}
  async function walk(point){
    for(let i=0;i<80;i++){
      const v=await game.observe(),dx=point.x-v.position.x,dy=point.y-v.position.y,d=Math.hypot(dx,dy);
      if(d<.8)return;
      const x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2,keys=[];
      if(Math.abs(x)>.12)keys.push(x>0?'d':'a');if(Math.abs(y)>.12)keys.push(y>0?'s':'w');
      await game.act({keys,duration_ms:Math.min(350,Math.max(50,Math.round(d*80)))});
    }
    assert.fail('Native walk failed to reach '+JSON.stringify(point));
  }
  async function transfer(keys,mouse,name,world){
    await game.act({keys,...(mouse?{mouse}:{}),duration_ms:700});
    for(let i=0;i<12&&(await game.observe()).world!==world;i++)await game.act({keys:[],duration_ms:500});
    const v=await save(name);assert.equal(v.world,world);return v;
  }
  const initial=await save('spawn');
  await game.act({keys:[],duration_ms:1000},{name:'nexus-spawn',when:v=>v.world==='Nexus'});
  await walk({x:72.5,y:72.5});
  const ally=await game.join({...fixture,seed:898,position:{x:72.5,y:73.5}});
  const outside=await game.join({...fixture,seed:899,position:{x:79.5,y:73.5}});
  for(const client of [game,ally,outside])await vitals(client,100,100);
  for(const [name,client] of [['near-before',game],['ally-before',ally],['outside-before',outside]])await save(name,client);
  await game.act({keys:[],duration_ms:2200},{name:'nexus-fountain',when:v=>v.character.hp>=200});
  const near=await save('near-after'),second=await save('ally-after',ally),far=await save('outside-after',outside);
  // Comparator policy, not authenticated 123.5.1: https://github.com/moistosaurus/realm-src/blob/1855c017eab870d13f0b61b1cf0b9e66a01922ff/wServer/logic/behaviors/HealPlayer.cs
  game.check('one fountain heals both injured native recipients',near.character.hp>=300&&second.character.hp>=300);
  game.check('outside every fountain radius only natural HP regeneration applies',far.character.hp>=100&&far.character.hp<=104);
  game.check('fountains do not restore MP beyond natural regeneration',[near,second,far].every(v=>v.character.mp>=100&&v.character.mp<=103));
  await vitals(game,669,100);await game.act({keys:[],duration_ms:1500});
  game.check('fountain healing clips at effective maximum HP',(await save('clipped')).character.hp===670);
  const entities=(await game.inspect()).diagnostic.entities;
  game.check('Nexus publishes sheep and the nearby fountain',entities.some(e=>e.kind===0x1ff)&&entities.some(e=>e.kind===0x1c7));
  await walk({x:78.5,y:75.5});
  const vault=(await game.inspect()).diagnostic.entities.find(e=>e.kind===0x720);assert(vault,'Vault portal missing');
  await walk(vault.pos);
  const arrived=await transfer([],{x:700,y:575},'vault','Vault');
  const stayed=await save('other-account-stays',ally);
  game.check('Vault portal isolates accounts and retains the living character',stayed.world==='Nexus'&&arrived.lifecycle.alive&&arrived.session.character===initial.session.character&&arrived.session.account===initial.session.account);
  await transfer(['Insert'],null,'vault-escape','Nexus');
  await walk({x:78.5,y:42.5});
  const realm=(await game.inspect()).diagnostic.entities.find(e=>e.kind===0x700);assert(realm,'Realm portal missing');
  await walk(realm.pos);
  const entered=await transfer([],{x:700,y:575},'realm','Realm');
  const returned=await transfer(['Insert'],null,'realm-escape','Nexus');
  game.check('ordinary Realm entry and escape retain identity equipment and inventory',entered.lifecycle.alive&&returned.lifecycle.alive&&returned.session.character===initial.session.character&&returned.session.account===initial.session.account&&JSON.stringify(returned.character.items)===JSON.stringify(initial.character.items)&&returned.character.inventory_revision===initial.character.inventory_revision);
  game.check('escape returns inside a configured Nexus spawn cell',returned.position.x>=76&&returned.position.x<80&&returned.position.y>=72&&returned.position.y<76);
  await game.act({keys:[],duration_ms:1000},{name:'nexus-return',when:v=>v.world==='Nexus'&&v.lifecycle.alive});
}
