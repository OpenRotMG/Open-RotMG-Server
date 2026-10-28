import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';

const out=process.env.QA_RESULTS_DIR;
const spec={class:0x307,level:20,seed:787,world:1,equipment:[0xc02,0xade,0xaf9,0xacd],backpack:Array(8).fill(-1),stats:[700,252,75,25,50,50,40,50]};
const roots=new Set([0x668,0x67d,0x91f,0x91b,0x928,0x675,0x678,0xd56,0xd59,0xd5f,0xd50,0xd54,0xd61,0xe37]);
export async function prepare(api){
  const world=await api({op:'world_info',world:1}),boss=world.encounter_markers.find(e=>e.kind===0x67d);
  assert(boss,'An ordinarily populated Cyclops encounter is required');
  return {...spec,position:boss.position};
}
export async function run(game){
  const config=JSON.parse(await readFile(join(out,'native/native.json'),'utf8')),token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const headers={Authorization:'Bearer '+token,'Content-Type':'application/json'},protection=new Map(),records=[];
  let attackUntil=0;
  async function api(command){
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers,body:JSON.stringify({id:randomUUID(),command}),signal:AbortSignal.timeout(10000)}),value=await response.json();
    assert(response.ok&&value.status==='succeeded',JSON.stringify(value));return value;
  }
  const cheat=(client,change)=>api({op:'cheat',actor:client.actor,change});
  async function events(){return (await readFile(join(out,'native/server.log'),'utf8')).split('\n').filter(l=>l.startsWith('{')).map(l=>JSON.parse(l).fields);}
  async function map(){
    const response=await fetch(config.origin+'/dev/map?player='+(await game.observe()).session.player,{headers,signal:AbortSignal.timeout(10000)});
    assert(response.ok,'Privileged map observation failed');return response.json();
  }
  async function save(label){const value={label,main:await game.inspect(),peer:await peer.observe()};records.push(value);await writeFile(join(out,'closure.json'),JSON.stringify(records));return value;}
  const initial=await game.observe(),origin=initial.session.world,peer=await game.join({...spec,seed:788,world:origin,position:initial.position}),peerInitial=await peer.observe();
  async function act(input,capture){
    const now=(await game.observe()).game_ms;
    for(const client of [game,peer])if(now+input.duration_ms>=(protection.get(client.actor)??0)){
      await cheat(client,{kind:'protection',condition:'invulnerable',duration_ms:60000});protection.set(client.actor,now+60000);
    }
    if(now>=attackUntil){await cheat(game,{kind:'stat_modifier',index:2,amount:100,duration_ms:60000});attackUntil=now+60000;}
    return game.act(input,capture);
  }
  const opening=await map(),blocked=new Set(opening.blocked.map(([x,y])=>x+','+y)),original=opening.entities.filter(e=>roots.has(e.kind)&&(e.quest===e.id||[0x668,0x67d].includes(e.kind)));
  assert(original.length>20,'Full ordinary roster required, not a prepared final encounter');
  await writeFile(join(out,'opening.json'),JSON.stringify({original,initial,peerInitial}));
  await act({keys:[],duration_ms:500},{name:'realm-opening',when:()=>true});
  await save('opening');
  const seen=new Map(original.map(e=>[e.quest??e.id,e]));
  for(let encounter=0;encounter<100;encounter++){
    const log=await events(),credits=log.filter(e=>e.message==='qualifying Realm hero defeated'&&e.origin===origin),defeated=new Set(credits.map(e=>e.hero));
    const current=await map();
    for(const e of current.entities)if(roots.has(e.kind)&&e.quest===e.id)seen.set(e.id,e);
    const next=[...seen.keys()].find(id=>!defeated.has(id));
    if(next===undefined){assert(credits.at(-1)?.remaining===0,'Every counted root must earn actual death credit');break;}
    let completed=false;
    for(let step=0;step<100;step++){
      const log=await events();if(log.some(e=>e.message==='qualifying Realm hero defeated'&&e.origin===origin&&e.hero===next)){completed=true;break;}
      const state=await map(),family=state.entities.filter(e=>e.id===next||e.quest===next);
      if(!family.length){await act({keys:[],duration_ms:1000});continue;}
      const vulnerable=family.filter(e=>e.hp>0&&!(e.condition&((1<<24)|(1<<25))));
      const healer=vulnerable.find(e=>e.kind===0x91d);
      const target=healer??vulnerable.find(e=>roots.has(e.kind)||[0x920,0x91c,0x92d,0x6c1,0x929,0xd5e,0xd62].includes(e.kind));
      const anchor=target??family.find(e=>roots.has(e.kind)||[0x920,0x91c,0x92d,0x6c1,0x6c2].includes(e.kind));assert(anchor,'Encounter has no living leader or required combat target');
      const view=await game.observe();
      if(Math.hypot(anchor.pos.x-view.position.x,anchor.pos.y-view.position.y)>3){
        const position=Array.from({length:8},(_,n)=>({x:anchor.pos.x+1.5*Math.cos(n*Math.PI/4),y:anchor.pos.y+1.5*Math.sin(n*Math.PI/4)})).find(p=>!blocked.has(Math.floor(p.x)+','+Math.floor(p.y)));
        assert(position,'No walkable prepared observation point');
        for(const client of [game,peer])await cheat(client,{kind:'teleport',position});
        await act({keys:[],duration_ms:100});
        continue;
      }
      if(target){
        const nearby=(await game.inspect()).view.objects.some(e=>e.id===target.id);assert(nearby,'Combat target must be visible to the native client');
        await act({keys:[],duration_ms:2000,target:{entity:target.id,camera_angle:-Math.PI/4,fire:true}});
      }else await act({keys:[],duration_ms:5000});
    }
    assert(completed||(await events()).some(e=>e.message==='qualifying Realm hero defeated'&&e.origin===origin&&e.hero===next),'Native combat did not finish counted encounter '+next);
    const receipt={root:next,kind:seen.get(next).kind,view:await game.observe()};
    await writeFile(join(out,'root-'+next+'.json'),JSON.stringify(receipt));
  }
  const log=await events(),credits=log.filter(e=>e.message==='qualifying Realm hero defeated'&&e.origin===origin);
  game.check('full ordinary roster plus admitted events earns once-only closure credit',credits.length===seen.size&&new Set(credits.map(e=>e.hero)).size===credits.length&&credits.at(-1)?.remaining===0);
  const closing=await save('closed');
  game.check('both original native characters remain in the closed Realm',closing.main.view.session.world===origin&&closing.peer.session.world===origin);
  await act({keys:[],duration_ms:700},{name:'realm-closed',when:()=>true});
  // Reconnect retains charId rather than creating a successor: https://github.com/moistosaurus/realm-cli/blob/a13c855dc5e09ee301fd0a66fcd303cfb5dff74e/src/kabam/rotmg/messaging/impl/GameServerConnection.as#L1635-L1690
  for(let n=0;n<30&&(await game.observe()).session.world===origin;n++)await act({keys:[],duration_ms:5000});
  const arrived=await save('castle');
  for(const [label,before,after,settled] of [['killer',initial,arrived.main.view,closing.main.view],['observer',peerInitial,arrived.peer,closing.peer]]){
    game.check(label+' follows the forced route into Castle with the same living character',after.world==="Oryx's Castle"&&after.lifecycle.alive&&after.session.account===before.session.account&&after.session.character===before.session.character&&after.session.world!==origin);
    game.check(label+' retains exact equipment and revision through closure',JSON.stringify(after.character.items)===JSON.stringify(before.character.items)&&after.character.inventory_revision===before.character.inventory_revision);
    game.check(label+' retains earned XP through closure',after.character.experience>=settled.character.experience);
  }
  game.check('collective recipients enter the same Castle instance',arrived.main.view.session.world===arrived.peer.session.world);
  game.check('observer never fired native shots',(await peer.inspect()).diagnostic.character.shots===0);
  await act({keys:[],duration_ms:3000},{name:'castle-arrival',when:v=>v.world==="Oryx's Castle"&&v.lifecycle.alive});
  const finalLog=await events(),route=finalLog.find(e=>e.message==='Realm closed; collective Castle route and fresh Realm installed'&&e.origin===origin),departure=finalLog.findIndex(e=>e.message==='all collective recipients durably departed; replacement delay started'&&e.origin===origin),published=finalLog.findIndex(e=>e.message==='safe collective departure completed; replacement published'&&e.origin===origin);
  game.check('normal replacement is published only after collective durable departure',route?.recipients===2&&departure>=0&&published>departure&&finalLog[published].replacement===route.replacement);
  await writeFile(join(out,'lifecycle.json'),JSON.stringify({original:[...seen.values()],credits,route,events:finalLog.filter(e=>e.origin===origin)}));
}
