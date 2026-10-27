import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {connect} from '../../scripts/record-demo.mjs';

let connection;
export async function beforeNavigate({browser}) {
  connection=await connect({...browser,url:'about:blank',helper:join(process.env.PI_BROWSER_TOOLS,'direct-cdp.mjs')});
  await connection.call('Emulation.setFocusEmulationEnabled',{enabled:true});
}
export const fixture = {
  class:0x30e,level:1,seed:7464,world:'abyss-of-demons',
  position:world=>{if(!world.damaging_ground.length)throw Error('Abyss has no observed walkable damaging ground');return world.damaging_ground[0];},
  equipment:[0xa97,0xa2e,-1,-1],backpack:Array(8).fill(-1),stats:null,
};
export async function run(game) {
  const root=join(process.env.QA_RESULTS_DIR,'native');
  const {origin}=JSON.parse(await readFile(join(root,'result.json'),'utf8'));
  const token=(await readFile(join(root,'state/control.token'),'utf8')).trim();
  let serial=0;
  async function api(command) {
    const response=await fetch(origin+'/dev/testing',{method:'POST',headers:{Authorization:`Bearer ${token}`,'Content-Type':'application/json'},body:JSON.stringify({id:'guest-lifecycle-'+serial++,command})});
    const value=await response.json();
    if(!response.ok||value.status==='failed')throw Error(JSON.stringify(value));
    return value;
  }
  const before=await game.observe();
  await api({op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invincible',duration_ms:60000}});
  const created=await api({op:'persona',profile:'guest:v1:4',world:before.session.world,position:before.position});
  const actor=created.result.actor,name=`Guest${created.result.account}`;
  const ordinary=await api({op:'inspect',actor});
  game.check('ordinary guest starts level one with its actual default name',ordinary.diagnostic.character.name===name&&ordinary.view.character.level===1&&ordinary.view.character.hp===100);
  await api({op:'cheat',actor,change:{kind:'vitals',hp:1,mp:ordinary.view.character.mp}});
  await writeFile(join(root,'prepared-death-context.json'),JSON.stringify({created,ordinary,observer_protection:'invincible60s',bot_prepared_hp:1,actual_damaging_ground:before.position},null,2));
  let seen;
  await game.act([
    {actor:game.actor,input:{keys:[],duration_ms:500}},
    {actor:game.actor,input:{keys:['Insert'],duration_ms:8000}},
  ],{name:'guest-replacement',when:view=>{
    if(view.world!=='Nexus'||view.game_ms<before.game_ms+1500)return false;
    const peer=view.nearby.find(p=>p.name===name);
    if(peer){seen={peer,view};return true;}
    return false;
  }});
  const after=await game.observe();
  game.check('native observer ordinarily escapes the actual hazard world',after.world==='Nexus'&&after.lifecycle.alive&&after.session.character===before.session.character&&after.session.player!==before.session.player);
  const feeds=await (await fetch(origin+'/api/bots')).json();
  const replacement=feeds.bots.find(bot=>bot.name===name);
  game.check('guest retains its actual account-visible name across a committed replacement',Boolean(replacement)&&replacement.frame.character!==actor&&replacement.frame.class==='Wizard'&&replacement.frame.level===1&&replacement.frame.hp>0);
  game.check('native observer sees the actual ordinary guest replacement',Boolean(seen)&&seen.peer.kind===0x30e);
  await writeFile(join(root,'guest-replacement.json'),JSON.stringify({created,replacement,seen,observer:after},null,2));
  connection.close();
}
