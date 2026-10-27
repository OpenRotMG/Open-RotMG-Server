import assert from 'node:assert/strict';
import {readFile, appendFile} from 'node:fs/promises';
import {randomUUID} from 'node:crypto';

export const fixture = {
  class:0x30e, level:20, seed:784, world:'ocean-trench', position:'spawn',
  equipment:[0xa97,0xa2e,-1,-1], backpack:Array(8).fill(-1),
  stats:[500,300,50,20,40,50,30,40],
};
export async function run(game) {
  const out=process.argv[process.argv.indexOf('--run-dir')+1];
  const config=JSON.parse(await readFile(out+'/native.json','utf8'));
  const token=(await readFile(config.state+'/control.token','utf8')).trim();
  async function api(command) {
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:'grave-'+randomUUID(),command})});
    const value=await response.json();assert(response.ok&&value.status!=='failed',JSON.stringify(value));return value;
  }
  async function inspect(label) {
    const value=(await game.inspect()).diagnostic;
    await appendFile(out+'/gravestones.jsonl',JSON.stringify({label,...value})+'\n');return value;
  }
  const start=await game.observe(), origin=start.position;
  const victims=[];
  for(const [i,level,stats,kind] of [
    [0,1,[100,100,12,0,10,15,12,12],0x723],
    [1,20,[500,300,50,20,40,50,30,40],0x725],
    [2,20,[670,385,75,25,50,75,40,60],0x735],
  ]) {
    const position={x:origin.x+i,y:origin.y+2};
    const created=await api({op:'spawn',spec:{...fixture,guid:'grave-victim-'+i,world:start.session.world,position,level,stats,controller:'direct',profile:null}});
    const actor=created.result.actor;
    const before=(await api({op:'inspect',actor})).diagnostic;
    assert(before.entities.filter(e=>e.kind===0x731).every(e=>Math.hypot(e.pos.x-position.x,e.pos.y-position.y)>=1),'Victim must start outside oxygen vents');
    victims.push({actor,name:before.character.name,position,kind,items:before.character.items});
  }
  await appendFile(out+'/gravestones.jsonl',JSON.stringify({label:'prepared-direct-victims',victims})+'\n');
  await game.act({keys:[],duration_ms:9000});
  await game.act({keys:[],duration_ms:9000});
  game.check('level-one death has already produced a grave',(await game.inspect()).diagnostic.entities.some(e=>e.kind===0x723));
  await game.act({keys:[],duration_ms:1200},{name:'gravestone-level-one',when:()=>true});
  await game.act({keys:[],duration_ms:9000});
  const dead=await inspect('three-committed-deaths');
  for(const victim of victims) {
    const graves=dead.entities.filter(e=>e.kind===victim.kind&&e.name===victim.name);
    game.check('one attributed grave for '+victim.kind,graves.length===1);
    const grave=graves[0];
    game.check('death position and empty contents for '+victim.kind,grave.pos.x===victim.position.x&&grave.pos.y===victim.position.y&&grave.items.length===0);
    game.check('grave is shared, not owner-restricted for '+victim.kind,grave.bag_owners.length===0);
  }
  await game.act({keys:[],duration_ms:1200},{name:'gravestones-shared',when:()=>true});
  for(let i=0;i<3;i++)await game.act({keys:[],duration_ms:9000});
  const expired=await inspect('level-one-expired');
  game.check('level-one grave expires while level-twenty and maxed graves remain',!expired.entities.some(e=>e.kind===0x723)&&[0x725,0x735].every(kind=>expired.entities.some(e=>e.kind===kind)));
}
