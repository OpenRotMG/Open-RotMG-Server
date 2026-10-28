import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';

const mode=process.env.CASTLE_MODE??'armor';
const positions={armor:{x:114.5,y:39.5},ordinary:{x:94.5,y:110.5},tower:{x:24.5,y:139.5}};
assert(Object.hasOwn(positions,mode),'CASTLE_MODE must be armor, ordinary or tower');
export const fixture={class:0x307,level:20,seed:866,world:'oryx-castle',position:positions[mode],equipment:[0xc02,0xb28,-1,0xac5],backpack:Array(8).fill(-1),stats:[700,252,75,25,50,50,40,50]};
export async function run(game){
  const root=process.env.QA_RESULTS_DIR,config=JSON.parse(await readFile(join(root,'native/native.json'),'utf8'));
  const headers={Authorization:'Bearer '+(await readFile(join(config.state,'control.token'),'utf8')).trim(),'Content-Type':'application/json'};
  const api=async command=>{const r=await fetch(config.origin+'/dev/testing',{method:'POST',headers,body:JSON.stringify({id:randomUUID(),command}),signal:AbortSignal.timeout(10000)});const s=await r.json();assert(r.ok&&s.status==='succeeded',JSON.stringify(s));return s;};
  const map=async()=>{const v=await game.observe(),r=await fetch(config.origin+'/dev/map?player='+v.session.player,{headers,signal:AbortSignal.timeout(10000)});assert(r.ok);return r.json();};
  const distance=(a,b)=>Math.hypot(a.x-b.x,a.y-b.y),samples=[];
  async function save(name){const state=await game.inspect();samples.push({name,state});await writeFile(join(root,'castle-journey.json'),JSON.stringify(samples));return state;}
  const initial=await save('initial');
  if(mode==='tower'){
    await api({op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}});
    await game.act({keys:[],duration_ms:1000},{name:'castle-quiet',when:()=>true});
    const hit=await save('tower-hit');
    game.check('natural Living Floor projectile applies Quiet and drains MP',!!(hit.view.character.conditions&2)&&hit.view.character.mp===0);
    game.check('selected tower does not add the later-guide Weak condition',!(hit.view.character.conditions&4)&&hit.view.lifecycle.alive);
    return;
  }
  await api({op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invincible',duration_ms:60000}});
  await api({op:'cheat',actor:game.actor,change:{kind:'stat_modifier',index:2,amount:100,duration_ms:60000}});
  if(mode==='armor')await game.act({keys:[],duration_ms:1500},{name:'castle-armor',when:()=>true});
  else await game.act({keys:[],duration_ms:100});
  const before=await save('armor-before'),population=await map();
  const armor=population.entities.filter(e=>e.kind===0xd7e&&(mode==='armor'?e.parent!==null:e.parent===null)).sort((a,b)=>distance(a.pos,before.view.position)-distance(b.pos,before.view.position))[0];
  assert(armor&&distance(armor.pos,before.view.position)<5,'Requested authored/spawned armor absent');
  async function kill(original){
    const start=await game.inspect();
    for(let n=0;n<40;n++){
      const present=(await map()).entities.some(e=>e.id===original.id);
      if(!present){
        const end=await game.inspect();
        assert(end.diagnostic.character.kills>start.diagnostic.character.kills&&(original.expires===null||end.view.time_ms<original.expires),'Target disappeared without a credited native defeat before expiry');
        await writeFile(join(root,'defeat-'+original.id+'.json'),JSON.stringify({original,start,end}));return end;
      }
      await game.act({keys:[],duration_ms:350,target:{entity:original.id,camera_angle:-Math.PI/4,fire:true}});
    }
    throw Error('No bounded native armor defeat: '+original.id);
  }
  const first=await kill(armor);
  await save('armor-defeated');
  // The release note names respawns; selected XML also excludes authored armor: https://wikiwiki.jp/rmd/version/build123.4.4
  game.check('actual '+mode+' armor defeat grants no XP or base fame',first.view.character.experience===before.view.character.experience&&first.diagnostic.character.fame===before.diagnostic.character.fame);
  if(mode==='armor'){
    await game.act({keys:[],duration_ms:4000});
    const replacement=(await map()).entities.find(e=>e.kind===0xd7e&&e.parent===armor.parent&&e.id!==armor.id);assert(replacement,'Same spawner did not replace its defeated armor');
    const second=await kill(replacement);
    await save('replacement-defeated');
    game.check('same spawner replacement has a fresh identity and still grants no XP/fame',second.view.character.experience===first.view.character.experience&&second.diagnostic.character.fame===first.diagnostic.character.fame);
    await writeFile(join(root,'castle-armor-identities.json'),JSON.stringify({armor,replacement}));
  }
  const final=await game.observe();
  game.check('same living character retains its prepared gear',final.lifecycle.alive&&final.session.character===initial.view.session.character&&JSON.stringify(final.character.items)===JSON.stringify(initial.view.character.items));
}
