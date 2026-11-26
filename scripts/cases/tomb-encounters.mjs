import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';

const groups={turrets:[[0xd31,0xd32,0xd33],[0xd34,0xd35,0xd36]],swarms:[[0xd11,0xd13,0xd15],[0xd3a,0xd3b,0xd3c]],jackals:[[0xd19],[0xd1a]]};
const rooms=groups[process.env.TOMB_GROUP];assert(rooms,'Set TOMB_GROUP to turrets, swarms or jackals');
export const fixture={class:0x30e,level:20,seed:860,world:'tomb-of-the-ancients',worldSeed:12,position:'spawn',equipment:[0xb08,-1,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,75,25,50,75,40,60]};
export async function run(observer){
  const out=process.env.QA_RESULTS_DIR,config=JSON.parse(await readFile(join(out,'native/native.json'),'utf8'));
  const headers={Authorization:'Bearer '+(await readFile(join(config.state,'control.token'),'utf8')).trim(),'Content-Type':'application/json'};
  let serial=0;
  const api=async command=>{const r=await fetch(config.origin+'/dev/testing',{method:'POST',headers,body:JSON.stringify({id:'tomb-encounters-'+(++serial),command}),signal:AbortSignal.timeout(10000)});const s=await r.json();assert(r.ok&&s.status!=='failed',JSON.stringify(s));return s;};
  const initial=await observer.observe(),{worldSeed,...spec}=fixture;
  const map=async()=>{const r=await fetch(config.origin+'/dev/map?player='+initial.session.player,{headers,signal:AbortSignal.timeout(10000)});assert(r.ok);return r.json();};
  const layout=await map(),samples=[],conditions=new Set();
  for(const kinds of rooms){
    const targets=kinds.map(kind=>{const e=layout.entities.find(e=>e.kind===kind);assert(e,'Generated roster lacks '+kind.toString(16));return e;});
    // Center Tomb Turrets attack only with a player within six tiles (actors.xml:7514-7564), so stand by the middle member.
    const root=targets[Math.floor(targets.length/2)],game=await observer.join({...spec,world:initial.session.world,position:{x:root.pos.x,y:root.pos.y+3}});
    let expiry=0;
    // Prepared protection cannot be reapplied while active: finish its window idle, then renew it before the next action.
    async function act(input,capture){
      let now=(await game.observe()).game_ms;
      if(now<expiry&&expiry-now<input.duration_ms){await game.act({keys:[],duration_ms:Math.max(50,expiry-now)});now=(await game.observe()).game_ms;}
      if(now>=expiry){
        await api({op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}});
        await api({op:'cheat',actor:game.actor,change:{kind:'stat_modifier',index:2,amount:100,duration_ms:60000}});
        expiry=now+60000;
      }
      return game.act(input,capture);
    }
    await act({keys:[],duration_ms:2500});
    const observations=[];let pending,error;
    const timer=setInterval(()=>{if(!pending)pending=game.inspect().then(s=>{observations.push(s);conditions.add(s.view.character.conditions);}).catch(e=>error=e).finally(()=>pending=null);},100);
    try{await act({keys:[],duration_ms:6500},{name:'tomb-family-'+root.kind.toString(16),when:()=>true});}finally{clearInterval(timer);if(pending)await pending;}
    if(error)throw error;
    for(const target of targets){
      game.check(target.kind.toString(16)+': generated encounter produces natural native-visible projectiles',observations.some(s=>s.view.projectiles.some(p=>p.owner===target.id)));
      if([0xd11,0xd13,0xd15].includes(target.kind)){
        const counts=observations.map(s=>s.diagnostic.entities.filter(e=>e.parent===target.id&&e.kind===target.kind-1).length);
        // MakeMinions maxChildren6 (actors.xml:5720).
        game.check(target.kind.toString(16)+': owned swarm reaches but never exceeds six living followers',Math.max(...counts)===6&&counts.every(n=>n<=6));
        const xp=(await game.observe()).character.experience;
        // XML Swarm Masters Follow/Wander behind six MakeMinions shields (actors.xml:5718-5720), so close in while firing.
        for(let n=0;n<30;n++){
          const leader=(await map()).entities.find(e=>e.id===target.id);if(!leader)break;
          const v=(await game.inspect()).view,seen=v.objects.some(o=>o.id===target.id),dx=leader.pos.x-v.position.x,dy=leader.pos.y-v.position.y,x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2;
          await act({keys:Math.hypot(dx,dy)>4||!seen?[Math.abs(x)>.15?(x>0?'d':'a'):null,Math.abs(y)>.15?(y>0?'s':'w'):null].filter(Boolean):[],duration_ms:seen?1000:300,...(seen?{target:{entity:target.id,camera_angle:-Math.PI/4,fire:true}}:{})});
        }
        const dead=await map();game.check(target.kind.toString(16)+': native staff kills the real leader and earns XP',!dead.entities.some(e=>e.id===target.id)&&(await game.observe()).character.experience>xp);
        const survivors=dead.entities.filter(e=>e.parent===target.id).map(e=>e.id);
        await act({keys:[],duration_ms:4500});
        game.check(target.kind.toString(16)+': leader loss cannot replenish its followers',(await map()).entities.filter(e=>e.parent===target.id).every(e=>survivors.includes(e.id)));
      }
    }
    samples.push({targets,observations,after:await game.inspect()});await writeFile(join(out,'encounters.json'),JSON.stringify(samples));
    game.check('ordinary encounters never grant Tomb completion',(await game.inspect()).diagnostic.character.tombs===0);
    const before=await game.observe();await act({keys:['Insert'],duration_ms:300});const escaped=await game.observe();game.check('ordinary escape preserves the same living character',escaped.world==='Nexus'&&escaped.lifecycle.alive&&escaped.session.character===before.session.character&&(await game.inspect()).diagnostic.character.tombs===0);
  }
  if(process.env.TOMB_GROUP==='turrets')observer.check('natural turret hits apply Slow, Paralyze and Armor Broken',[4,14,27].every(bit=>[...conditions].some(mask=>mask&(1<<(bit-1)))));
  await writeFile(join(out,'encounter-conditions.json'),JSON.stringify([...conditions]));
}
