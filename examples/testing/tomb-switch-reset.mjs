import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {setTimeout as delay} from 'node:timers/promises';

export const fixture={class:0x31e,level:20,seed:12351,world:'tomb-of-the-ancients',worldSeed:12,position:{x:16.9,y:16.1},equipment:[0xa00,0xc0f,0xafc,0xacd],backpack:Array(8).fill(-1),stats:[770,252,50,40,50,50,75,50]};
export async function run(game){
  const out=process.env.QA_RESULTS_DIR,snapshots=[],samples=[],abilities=[];let player,switchId,originals,mode='initial',firstEpoch,resetAt,secondEpoch,reentrySeen,lastOutsideAt,firstDisabled,replacementIds,observerFailure,failure;
  const safe=s=>assert(s.view.lifecycle.alive&&s.diagnostic.character.alive&&s.diagnostic.character.hp>350&&s.diagnostic.character.mp===252&&s.diagnostic.time_ms<26000&&s.diagnostic.character.shots===0&&s.diagnostic.character.kills===0&&s.diagnostic.character.tombs===0,'Living HP>350/MP252/time<26000 and zero firing/kills/credit required');
  const save=async label=>{const s={label,observed:await game.observe(),inspected:await game.inspect()};snapshots.push(s);safe(s.inspected);if(switchId!==undefined&&label!=='escaped')phase(s.inspected);return s;};
  const localOff=s=>s.diagnostic.entities.filter(e=>e.kind===0xd39&&Math.hypot(e.pos.x-16.5,e.pos.y-16.5)<8);
  const active=s=>localOff(s).length===0&&originals.every(t=>s.diagnostic.entities.some(e=>e.id===t.id&&e.kind===0xd38&&e.tomb?.anchor===switchId&&Math.hypot(e.pos.x-t.pos.x,e.pos.y-t.pos.y)<.01));
  const replaced=s=>{const dead=localOff(s);return dead.length===8&&new Set(dead.map(e=>e.id)).size===8&&originals.every(t=>!s.diagnostic.entities.some(e=>e.id===t.id)&&dead.filter(e=>Math.hypot(e.pos.x-t.pos.x,e.pos.y-t.pos.y)<.01).length===1);};
  const distance=s=>Math.hypot(s.diagnostic.pos.x-16.5,s.diagnostic.pos.y-16.5);
  const phase=s=>{
    safe(s);const now=s.diagnostic.time_ms,e=s.diagnostic.entities.find(e=>e.id===switchId&&e.kind===0xd37);assert(e?.tomb&&Math.hypot(e.pos.x-16.5,e.pos.y-16.5)<.01,'Original switch/context/position required');assert(Number.isFinite(now)&&now>=0&&distance(s)<4,'Finite clock and bounded ordinary room movement required');if(mode==='initial')assert(Math.hypot(s.diagnostic.pos.x-16.9,s.diagnostic.pos.y-16.1)<.02,'Partial first hold must retain its supplied position');
    if(e.stage===null){assert(mode==='initial'&&firstEpoch===undefined&&e.tomb.press_since===null&&active(s),'Only the complete pre-first-tick switch may be uninitialized');return e;}
    assert(e.stage===0||e.stage===1,'Known switch phase required');if(firstDisabled)assert(e.stage===1,'Disabled switch must not regress');if(e.stage===0&&distance(s)>=1)lastOutsideAt=now;if(mode==='held')assert(distance(s)<1&&secondEpoch!==undefined,'Fresh continuous hold must stay within radius');
    if(e.stage===0){
      assert(active(s),'All eight unchanged originals, without premature off traps, required');const since=e.tomb.press_since;
      if(since===null){
        assert(firstEpoch!==undefined&&secondEpoch===undefined&&['leaving','outside','returning'].includes(mode),'Pressure may reset only during the intended interruption');
        if(mode==='leaving'){assert(distance(s)>=1,'Reset before leaving the pressure radius');resetAt??=now;}else assert(resetAt!==undefined,'A real interruption must precede the outside/return phases');
        if(mode==='outside')assert(distance(s)>=1,'Outside hold must remain outside');
        // MOVE can arrive between AI ticks; absence must not persist while standing inside.
        if(mode==='returning'&&distance(s)<1){reentrySeen??=now;assert(now<reentrySeen+100,'Reentry did not establish pressure within two AI ticks');}
      }else{
        assert(Number.isFinite(since)&&since>=0&&since<=now,'Finite pressure epoch required');
        if(mode==='initial'||(mode==='leaving'&&resetAt===undefined)){if(firstEpoch===undefined)firstEpoch=since;else assert(since===firstEpoch,'Partial first epoch changed before interruption');}
        else{assert(resetAt!==undefined&&['returning','held'].includes(mode)&&distance(s)<1&&lastOutsideAt!==undefined&&since>=lastOutsideAt&&since>firstEpoch&&since>=resetAt&&(reentrySeen===undefined||since<reentrySeen+100),'New epoch requires occupancy, no outside backdating and timely initialization');if(secondEpoch===undefined)secondEpoch=since;else assert(since===secondEpoch,'Second uninterrupted epoch reset');}
        assert(now<since+3000,'Three seconds of continuous pressure did not deactivate');
      }
    }else{
      assert(secondEpoch!==undefined&&now>=secondEpoch+3000&&e.tomb.press_since===null&&replaced(s),'Early, unbound or incomplete deactivation');
      const ids=localOff(s).map(e=>e.id).sort((a,b)=>a-b);if(!firstDisabled){firstDisabled={time:now};replacementIds=ids;}else assert.deepEqual(ids,replacementIds,'Released room replacement identities changed');
    }
    return e;
  };
  async function events(){const text=await readFile(join(out,'native/server.log'),'utf8');assert(text.endsWith('\n'),'Complete accepted-ability records required');abilities.splice(0,abilities.length,...text.trimEnd().split('\n').filter(x=>x.startsWith('{')).map(JSON.parse).filter(x=>x.fields?.message==='ability activated'&&x.fields.player===player).map(x=>x.fields));assert(abilities.length===0,'Passive interruption case must have no accepted abilities');return abilities;}
  async function sample(){
    const s=await game.inspect();try{const e=s.diagnostic.entities.find(e=>e.id===switchId);samples.push({time:s.diagnostic.time_ms,hp:s.diagnostic.character.hp,mp:s.diagnostic.character.mp,pos:s.diagnostic.pos,mode,switchMissing:!e,stage:e?.stage,pressSince:e?.tomb?.press_since,originalTrapsActive:active(s),originalTrapsReplaced:replaced(s)});phase(s);}
    catch(error){observerFailure={inspected:s,error:String(error)};throw error;}
  }
  async function watched(input,capture){
    let running=true;const action=game.act(input,capture).finally(()=>running=false);
    const observing=(async()=>{for(let n=0;running&&(await game.observe()).paused;n++){assert(n<250,'Native actor did not unpause');await delay(20);}while(running){await sample();await delay(80);}})();
    const results=await Promise.allSettled([action,observing]);for(const r of results)if(r.status==='rejected')throw r.reason;
  }
  try{
    const before=await save('prepared');player=before.inspected.diagnostic.player;
    assert(before.observed.world==='Tomb of the Ancients'&&before.inspected.view.character.hp===970&&before.inspected.view.character.mp===252&&!before.inspected.diagnostic.entities.some(e=>e.kind===0xd1a),'Fresh legal Knight outside Mummy encounters required');
    assert(Math.hypot(before.inspected.diagnostic.pos.x-16.9,before.inspected.diagnostic.pos.y-16.1)<.02,'Exact supplied start required');
    const switches=before.inspected.diagnostic.entities.filter(e=>e.kind===0xd37&&Math.hypot(e.pos.x-16.5,e.pos.y-16.5)<.01);assert(switches.length===1,'Unique original switch required');switchId=switches[0].id;
    originals=before.inspected.diagnostic.entities.filter(e=>e.kind===0xd38&&e.tomb?.anchor===switchId).map(e=>({id:e.id,pos:e.pos}));assert(originals.length===8&&new Set(originals.map(t=>t.id)).size===8&&new Set(originals.map(t=>`${t.pos.x},${t.pos.y}`)).size===8,'Eight distinct original trap identities and positions required');phase(before.inspected);
    await watched({keys:[],duration_ms:300});const partial=await save('partial');assert(phase(partial.inspected).stage===0&&firstEpoch!==undefined&&partial.inspected.diagnostic.time_ms<firstEpoch+1500,'First partial pressure must leave time for an interruption');
    mode='leaving';await watched({keys:['w'],duration_ms:300});const left=await save('left');assert(phase(left.inspected).stage===0&&resetAt!==undefined&&distance(left.inspected)>=1&&left.inspected.diagnostic.entities.find(e=>e.id===switchId).tomb.press_since===null,'Ordinary movement must interrupt pressure');
    mode='outside';await watched({keys:[],duration_ms:3300},{name:'tomb-interrupted-pressure-active',when:v=>v.lifecycle.alive&&v.game_ms>=firstEpoch+3500});const outside=await save('outside');assert(phase(outside.inspected).stage===0&&outside.inspected.diagnostic.time_ms>=firstEpoch+3500,'Interrupted first epoch must not activate at its old deadline');
    game.check('leaving early resets pressure beyond the original three-second deadline',active(outside.inspected)&&resetAt!==undefined);
    mode='returning';let returned=await save('return-start');
    for(let n=0;secondEpoch===undefined||distance(returned.inspected)>=.8;n++){assert(n<12,'Bounded ordinary return did not establish stable pressure');await watched({keys:['s'],duration_ms:50});returned=await save('return-'+n);phase(returned.inspected);}
    assert(phase(returned.inspected).stage===0&&returned.inspected.diagnostic.time_ms<secondEpoch+2300,'Fresh second epoch must still be incomplete');
    game.check('ordinary reentry establishes a fresh pressure epoch',secondEpoch>firstEpoch&&secondEpoch>=resetAt&&distance(returned.inspected)<.8);
    mode='held';await watched({keys:[],duration_ms:3200});const disabled=await save('disabled');assert(phase(disabled.inspected).stage===1&&replaced(disabled.inspected),'New full uninterrupted hold must replace all eight originals');
    game.check('fresh three-second pressure replaces the eight original trap positions',Boolean(firstDisabled&&firstDisabled.time>=secondEpoch+3000));
    mode='release';await watched({keys:['w'],duration_ms:300});const released=await save('released');assert(phase(released.inspected).stage===1&&distance(released.inspected)>=1,'Deactivated switch must survive release');
    await watched({keys:[],duration_ms:5000},{name:'tomb-pressure-release-disabled',when:v=>v.lifecycle.alive&&v.game_ms>=secondEpoch+5200});const stable=await save('stable');assert(phase(stable.inspected).stage===1&&distance(stable.inspected)>=1&&replaced(stable.inspected),'Same replacements must remain after releasing pressure');
    await events();game.check('release preserves disabled traps without firing, abilities or credit',abilities.length===0&&JSON.stringify(localOff(stable.inspected).map(e=>e.id).sort((a,b)=>a-b))===JSON.stringify(replacementIds));
    await game.act({keys:['Insert'],duration_ms:300});const escaped=await save('escaped');await events();game.check('same living legal Knight returns with unchanged gear and no abilities',escaped.observed.world==='Nexus'&&escaped.observed.session.character===before.observed.session.character&&abilities.length===0&&JSON.stringify(escaped.inspected.view.character.items)===JSON.stringify(before.inspected.view.character.items));
  }catch(error){failure=error;throw error;}finally{
    let auditError;try{await events();}catch(error){auditError=error;}
    await writeFile(join(out,'switch-reset.json'),JSON.stringify({qualification:'Distinct fresh prepared single legal Knight, base770HP/252MP, ShortSword/Ogmur/Acropolis/HPring effective970HP/78DEF/75VIT/50WIS, suppliedseed12start16.9,16.1. Ordinary empty-key/W/S/Insert inputs only; no firing/abilities/companions/protection/stat or NPC-HP-RNG edits/refills/relocation/revival/adoption. Tests interrupted pressure/new full epoch and same replacements after release in one room, not replay of acceptedf836 for better media. Non-atomic observer/capture, holds not total exposure. Prepared start not earned entry/traversal; not instance reentry/multiplayer/quicksand/other rooms/historical precision/full Tomb or eight-body gallery acceptance.',snapshots,samples,abilities,switchId,originals,firstEpoch,resetAt,secondEpoch,reentrySeen,lastOutsideAt,firstDisabled,replacementIds,observerFailure,error:failure?String(failure):null,auditError:auditError?String(auditError):null},null,2));
    if(auditError)throw new AggregateError([...(failure?[failure]:[]),auditError],'Case and final accepted-ability audit must both remain observable');
  }
}
