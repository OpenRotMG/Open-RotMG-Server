import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {setTimeout as delay} from 'node:timers/promises';

export const fixture={class:0x31e,level:20,seed:12351,world:'tomb-of-the-ancients',worldSeed:12,position:{x:17.17,y:15.83},equipment:[0xa00,0xc0f,0xafc,0xacd],backpack:Array(8).fill(-1),stats:[770,252,50,40,50,50,75,50]};
export async function run(game){
  const out=process.env.QA_RESULTS_DIR,snapshots=[],samples=[],abilities=[];let player,switchId,originals,pressureSince,firstDisabled,failure;
  const safe=s=>assert(s.view.lifecycle.alive&&s.view.character.hp>300&&s.view.time_ms<26000&&s.diagnostic.character.shots===0&&s.diagnostic.character.kills===0&&s.diagnostic.character.tombs===0,'Living HP>300/time<26000 and no firing/kills/credit required');
  const save=async label=>{const s={label,observed:await game.observe(),inspected:await game.inspect()};safe(s.inspected);snapshots.push(s);return s;};
  const phase=s=>{const e=s.diagnostic.entities.find(e=>e.id===switchId&&e.kind===0xd37);assert(e&&e.tomb,'Original switch identity/context required');if(pressureSince!==undefined){assert(e.stage===0||e.stage===1,'Known switch phase required');if(firstDisabled)assert(e.stage===1,'Disabled switch must not regress');if(e.stage===0)assert(e.tomb.press_since===pressureSince&&active(s),'Standing pressure and all eight active traps required');else{assert(s.view.time_ms>=pressureSince+3000&&replaced(s),'Early or incomplete eight-trap deactivation');if(!firstDisabled)firstDisabled={time:s.view.time_ms,id:e.id};}}return e;};
  const active=s=>originals.every(t=>s.diagnostic.entities.some(e=>e.id===t.id&&e.kind===0xd38&&e.tomb?.anchor===switchId&&Math.hypot(e.pos.x-t.pos.x,e.pos.y-t.pos.y)<.01));
  const replaced=s=>{const dead=s.diagnostic.entities.filter(e=>e.kind===0xd39&&Math.hypot(e.pos.x-16.5,e.pos.y-16.5)<8);return dead.length===8&&originals.every(t=>!s.diagnostic.entities.some(e=>e.id===t.id)&&dead.filter(e=>Math.hypot(e.pos.x-t.pos.x,e.pos.y-t.pos.y)<.01).length===1);};
  async function events(){const text=await readFile(join(out,'native/server.log'),'utf8');assert(text.endsWith('\n'),'Complete accepted-ability records required');abilities.splice(0,abilities.length,...text.trimEnd().split('\n').filter(x=>x.startsWith('{')).map(JSON.parse).filter(x=>x.fields?.message==='ability activated'&&x.fields.player===player).map(x=>x.fields));return abilities;}
  async function sample(){const s=await game.inspect();safe(s);assert(Math.hypot(s.view.position.x-17.17,s.view.position.y-15.83)<.02,'Pressure actor must remain at its supplied position');const e=phase(s);samples.push({time:s.view.time_ms,hp:s.view.character.hp,mp:s.view.character.mp,stage:e.stage,pressSince:e.tomb.press_since,originalTrapsActive:active(s),originalTrapsReplaced:replaced(s)});}
  async function watched(input,capture){
    let running=true;const action=game.act(input,capture).finally(()=>running=false);
    const observing=(async()=>{for(let n=0;running&&(await game.observe()).paused;n++){assert(n<250,'Native actor did not unpause');await delay(20);}while(running){await sample();await delay(80);}})();
    const results=await Promise.allSettled([action,observing]);for(const result of results)if(result.status==='rejected')throw result.reason;
  }
  try{
    const before=await save('prepared');player=before.inspected.diagnostic.player;
    assert(before.observed.world==='Tomb of the Ancients'&&before.inspected.view.character.hp===970&&before.inspected.view.character.mp===252&&!before.inspected.diagnostic.entities.some(e=>e.kind===0xd1a),'Fresh legal Knight outside Mummy encounters required');
    assert(Math.hypot(before.inspected.view.position.x-17.17,before.inspected.view.position.y-15.83)<.02,'Supplied pressure position required');
    const switches=before.inspected.diagnostic.entities.filter(e=>e.kind===0xd37&&Math.hypot(e.pos.x-16.5,e.pos.y-16.5)<.01);assert(switches.length===1,'Unique original switch required');switchId=switches[0].id;
    assert(switches[0].stage===null||switches[0].stage===0,'Fresh inactive switch required');originals=before.inspected.diagnostic.entities.filter(e=>e.kind===0xd38&&e.tomb?.anchor===switchId).map(e=>({id:e.id,pos:e.pos}));assert(originals.length===8,'Exactly eight original local bomb traps required');
    // The offset exposes the switch without moving during its pressure interval.
    await watched({keys:[],duration_ms:300});
    const warm=await save('warm'),pressed=phase(warm.inspected);assert(pressed.stage===0&&Number.isFinite(pressed.tomb.press_since)&&active(warm.inspected),'Early uninterrupted pressure on all original active traps required');pressureSince=pressed.tomb.press_since;
    assert(warm.inspected.view.time_ms<pressureSince+2300,'Active screenshot window already closed');
    await watched({keys:[],duration_ms:200},{name:'tomb-active-bomb-traps',when:v=>v.lifecycle.alive&&v.game_ms<pressureSince+2500});
    const intact=await save('active');assert(phase(intact.inspected).stage===0&&active(intact.inspected),'Original eight traps must remain active before the deadline');
    await watched({keys:[],duration_ms:3500},{name:'tomb-disabled-bomb-traps',when:v=>v.lifecycle.alive&&v.game_ms>=pressureSince+3200});
    const disabled=await save('disabled');assert(phase(disabled.inspected).stage===1&&replaced(disabled.inspected),'Three-second standing pressure must replace exactly the eight original trap positions');
    await watched({keys:[],duration_ms:1000});const stable=await save('stable');
    game.check('standing pressure disables the eight original traps after three seconds',Boolean(firstDisabled&&firstDisabled.time>=pressureSince+3000&&phase(stable.inspected).stage===1&&replaced(stable.inspected)));
    await events();game.check('disabled room remains stable without firing, abilities, kills or completion credit',abilities.length===0&&stable.inspected.diagnostic.character.shots===0&&stable.inspected.diagnostic.character.kills===0&&stable.inspected.diagnostic.character.tombs===0);
    await game.act({keys:['Insert'],duration_ms:300});const escaped=await save('escaped');await events();
    game.check('same living legal Knight returns with unchanged gear and no abilities',escaped.observed.world==='Nexus'&&escaped.observed.session.character===before.observed.session.character&&abilities.length===0&&JSON.stringify(escaped.inspected.view.character.items)===JSON.stringify(before.inspected.view.character.items));
  }catch(error){failure=error;throw error;}finally{
    let auditError;try{await events();}catch(error){auditError=error;}
    await writeFile(join(out,'switch-pressure.json'),JSON.stringify({qualification:'Fresh prepared single native Knight with legal base770HP, ShortSword/Ogmur/Acropolis/HPring giving970HP/78DEF, suppliedseed12position17.17,15.83 insideone-tilepressure radius. No firing/abilities/joins/bots/NPC/HP/RNG/stat edits/refill/relocation/adoption/revival; onlypassivepressureandordinaryInsert. Read-only non-atomic observer/capture; holdsnot totalexposure. Currentone-roomthree-secondpressure/eighttrapreplacementonly. Imagesare representativeactive/disabledtrapandswitchviews, notaneight-bodyimagecensus. Notearnedentry/traversal, interruptedpressure/reentry/otherrooms/allturrets/historicalradiusorfullTombacceptance.',snapshots,samples,abilities,switchId,originals,pressureSince,firstDisabled,error:failure?String(failure):null,auditError:auditError?String(auditError):null},null,2));
    if(auditError)throw new AggregateError([...(failure?[failure]:[]),auditError],'Case and final accepted-ability audit must both remain observable');
  }
}
