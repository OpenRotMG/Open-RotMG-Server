import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {setTimeout as delay} from 'node:timers/promises';
import {connect} from '../../scripts/record-demo.mjs';

export const fixture={class:0x30e,level:20,seed:860,world:'tomb-of-the-ancients',worldSeed:0,position:'spawn',equipment:[0xb08,-1,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,75,25,50,75,40,60]};
export async function run(game){
  const out=process.env.QA_RESULTS_DIR,config=JSON.parse(await readFile(join(out,'native/native.json'),'utf8'));
  const headers={Authorization:'Bearer '+(await readFile(join(config.state,'control.token'),'utf8')).trim(),'Content-Type':'application/json'};
  let serial=0,expiry=0;
  const api=async command=>{const r=await fetch(config.origin+'/dev/testing',{method:'POST',headers,body:JSON.stringify({id:'tomb-encounters-'+(++serial),command}),signal:AbortSignal.timeout(10000)});const s=await r.json();assert(r.ok&&s.status!=='failed',JSON.stringify(s));return s;};
  const initial=await game.observe(),{worldSeed,...spec}=fixture;
  const scout=(await api({op:'spawn',spec:{...spec,world:initial.session.world,position:initial.position,guid:'tomb-tour-scout',controller:'direct',profile:null}})).result.actor;
  const name=(await api({op:'inspect',actor:scout})).diagnostic.character.name;
  const map=async()=>{const r=await fetch(config.origin+'/dev/map?player='+initial.session.player,{headers,signal:AbortSignal.timeout(10000)});assert(r.ok);return r.json();};
  const layout=await map(),kinds=[0xd31,0xd32,0xd33,0xd34,0xd35,0xd36,0xd11,0xd13,0xd15,0xd19,0xd1a,0xd3a,0xd3b,0xd3c];
  const targets=kinds.map(kind=>{const e=layout.entities.find(e=>e.kind===kind);assert(e,'Generated roster lacks '+kind.toString(16));return e;});
  const browser=config.native.find(n=>n.actor===game.actor),connection=await connect({...browser,url:config.origin+'/',helper:config.browserTools});
  const samples=[],conditions=new Set();let pending,error;
  async function protect(){
    const now=(await game.observe()).game_ms;if(now<expiry)return;
    for(const actor of [game.actor,scout])await api({op:'cheat',actor,change:{kind:'protection',condition:actor===game.actor?'invulnerable':'invincible',duration_ms:60000}});
    await api({op:'cheat',actor:game.actor,change:{kind:'stat_modifier',index:2,amount:100,duration_ms:60000}});expiry=now+60000;
  }
  async function act(input,capture){
    await protect();const remaining=expiry-(await game.observe()).game_ms;
    if(remaining<input.duration_ms){await game.act({keys:[],duration_ms:remaining});await protect();}
    await game.act(input,capture);
  }
  async function travel(point){
    await protect();if(expiry-(await game.observe()).game_ms<2500){await game.act({keys:[],duration_ms:expiry-(await game.observe()).game_ms});await protect();}
    await api({op:'cheat',actor:scout,change:{kind:'teleport',position:point}});
    const action=game.act({keys:[],duration_ms:2000});
    const typing=(async()=>{
      for(let n=0;(await game.observe()).paused;n++){assert(n<250,'Native action did not unpause');await delay(20);}
      async function enter(){try{await connection.call('Input.dispatchKeyEvent',{type:'keyDown',key:'Enter',code:'Enter',windowsVirtualKeyCode:13});}finally{await connection.call('Input.dispatchKeyEvent',{type:'keyUp',key:'Enter',code:'Enter',windowsVirtualKeyCode:13});}}
      await enter();await connection.call('Input.insertText',{text:'/teleport '+name});await enter();
    })();
    const results=await Promise.allSettled([action,typing]);for(const r of results)if(r.status==='rejected')throw r.reason;
    const arrived=await game.observe();assert(Math.hypot(arrived.position.x-point.x,arrived.position.y-point.y)<.2,'Ordinary chat teleport did not reach scout');
    await api({op:'cheat',actor:scout,change:{kind:'teleport',position:initial.position}});
  }
  try{
    await protect();
    for(const target of targets){
      await travel({x:target.pos.x-4,y:target.pos.y});
      const observations=[];
      const timer=setInterval(()=>{if(!pending)pending=game.inspect().then(s=>{observations.push(s);conditions.add(s.view.character.conditions);}).catch(e=>error=e).finally(()=>pending=null);},100);
      try{await act({keys:[],duration_ms:6500},[0xd32,0xd35,0xd19,0xd1a].includes(target.kind)?{name:'tomb-family-'+target.kind.toString(16),when:()=>true}:undefined);}finally{clearInterval(timer);if(pending)await pending;}
      if(error)throw error;
      game.check(target.kind.toString(16)+': generated encounter produces natural native-visible projectiles',observations.some(s=>s.view.projectiles.some(p=>p.owner===target.id)));
      if([0xd11,0xd13,0xd15].includes(target.kind)){
        const counts=observations.map(s=>s.diagnostic.entities.filter(e=>e.tomb?.producer===target.id&&e.kind===target.kind-1).length);
        game.check(target.kind.toString(16)+': owned swarm reaches but never exceeds eight living followers',Math.max(...counts)===8&&counts.every(n=>n<=8));
        const xp=(await game.observe()).character.experience;
        for(let n=0;n<10&&(await map()).entities.some(e=>e.id===target.id);n++)await act({keys:[],duration_ms:1000,target:{entity:target.id,camera_angle:-Math.PI/4,fire:true}});
        const dead=await map();game.check(target.kind.toString(16)+': native staff kills the real leader and earns XP',!dead.entities.some(e=>e.id===target.id)&&(await game.observe()).character.experience>xp);
        const survivors=dead.entities.filter(e=>e.tomb?.producer===target.id).map(e=>e.id);
        await act({keys:[],duration_ms:4500});
        game.check(target.kind.toString(16)+': leader loss cannot replenish its followers',(await map()).entities.filter(e=>e.tomb?.producer===target.id).every(e=>survivors.includes(e.id)));
      }
      samples.push({target,observations});await writeFile(join(out,'encounters.json'),JSON.stringify(samples));
    }
    game.check('natural turret hits apply Slow, Paralyze and Armor Broken',[4,14,27].every(bit=>[...conditions].some(mask=>mask&(1<<(bit-1)))));
    game.check('ordinary encounters never grant Tomb completion',(await game.inspect()).diagnostic.character.tombs===0);
    await act({keys:['Insert'],duration_ms:300});const escaped=await game.observe();game.check('ordinary escape preserves the same living character',escaped.world==='Nexus'&&escaped.lifecycle.alive&&escaped.session.character===initial.session.character&&(await game.inspect()).diagnostic.character.tombs===0);
    await writeFile(join(out,'encounter-conditions.json'),JSON.stringify([...conditions]));
  }finally{connection.close();}
}
