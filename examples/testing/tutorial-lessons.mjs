import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {setTimeout as delay} from 'node:timers/promises';
import {connect} from '../../scripts/record-demo.mjs';

export const fixture={class:0x30e,level:1,seed:899,world:-1,position:{x:12.5,y:166.5},equipment:[0xa97,0xa2e,-1,-1],backpack:Array(8).fill(-1),stats:null};
export async function run(game){
  const root=join(process.env.QA_RESULTS_DIR,'native'),configFile=join(root,'native.json'),records=[],first=game;
  async function record(label){const state=await game.inspect();records.push({label,actor:game.actor,...state});await writeFile(join(root,'lessons.json'),JSON.stringify(records));return state;}
  async function walk(goal){
    let previous=Infinity,stalled=0;
    for(let i=0;i<80;i++){
      const view=await game.observe(),dx=goal.x-view.position.x,dy=goal.y-view.position.y,distance=Math.hypot(dx,dy);
      if(distance<.3)return;
      stalled=distance>=previous-.02?stalled+1:0;previous=distance;
      assert(stalled<3,'Native walk stuck at '+JSON.stringify(view.position)+' toward '+JSON.stringify(goal));
      const x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2,keys=[];
      if(Math.abs(x)>.1)keys.push(x>0?'d':'a');if(Math.abs(y)>.1)keys.push(y>0?'s':'w');
      await game.act({keys,duration_ms:Math.min(700,Math.max(50,Math.round(distance*120)))});
    }
    throw Error('Native movement did not reach '+JSON.stringify(goal));
  }
  async function shoot(entity){
    for(let i=0;i<30;i++){
      if(!(await game.inspect()).diagnostic.entities.some(e=>e.id===entity.id))return;
      await game.act({keys:[],duration_ms:500,target:{entity:entity.id,camera_angle:-Math.PI/4,fire:true}});
    }
    throw Error('Native staff fire failed to defeat '+entity.id);
  }
  async function lesson(name,goal){await walk(goal);await game.act({keys:[],duration_ms:800},{name,when:()=>true});await record(name);}
  // Selected proximity overlays are independent, not sequential gates: https://github.com/OpenRotMG/Open-RotMG-Client/blob/0282b3a48ad0c73abd683572ece4e6c71b0cb055/client.swf
  await lesson('minimap',{x:12.5,y:164.5});
  await walk({x:12.5,y:164});
  await lesson('minimap-away',{x:17.5,y:164});
  const away=await game.observe();game.check('ordinary movement leaves the minimap trigger',Math.hypot(away.position.x-12.5,away.position.y-164.5)>3);
  await lesson('minimap-revisited',{x:12.5,y:164.5});
  game.check('ordinary backtracking revisits the minimap sign',(await game.observe()).lifecycle.alive);
  game=await game.join({...fixture,position:{x:12.5,y:153.5}});const health=game;
  await lesson('health',{x:12.5,y:157.5});
  game=await game.join({...fixture,position:{x:12.5,y:49.5}});
  const initial=await record('late-prepared-entry');
  await lesson('equipment',{x:12.5,y:47.5});
  for(const e of (await game.inspect()).diagnostic.entities.filter(e=>e.kind===0x163&&e.pos.y===43.5))await shoot(e);
  await lesson('ring-loot',{x:12.5,y:35.5});
  const before=await record('before-ring');
  await game.act({keys:[],duration_ms:700,drag:{from:{x:634,y:528},to:{x:766,y:385}}});
  const equipped=await record('equipped-ring');
  game.check('normal tutorial bag drag equips Minor Defense and commits one revision',equipped.view.character.items[3]===0xa4f&&equipped.view.character.inventory_revision===before.view.character.inventory_revision+1);
  game.check('ring leaves the authored bag and grants one defense',!equipped.diagnostic.entities.find(e=>e.kind===0x806)?.items.includes(0xa4f)&&equipped.view.character.stats[3]===before.view.character.stats[3]+1);
  for(const e of (await game.inspect()).diagnostic.entities.filter(e=>e.kind===0x163&&e.pos.y===30.5))await shoot(e);
  await lesson('ability',{x:12.5,y:26.5});
  const ability=await record('before-ability');await game.act({keys:[' '],duration_ms:300});const used=await record('used-ability');
  game.check('native Space uses the supplied starter spell and spends mana',used.view.character.mp<ability.view.character.mp&&used.view.character.ability_ready_ms>ability.view.character.ability_ready_ms);
  await lesson('portal',{x:12.5,y:4.5});
  await game.act({keys:[],mouse:{x:700,y:580},duration_ms:1800});
  const kitchen=await record('kitchen');game.check('ordinary selected final portal enters Kitchen with equipped ring',kitchen.view.map.name==='Kitchen'&&kitchen.view.character.items[3]===0xa4f);
  await game.act({keys:['Insert'],duration_ms:1800});game.check('native escape interrupts Kitchen into Nexus',(await game.observe()).world==='Nexus');
  const config=JSON.parse(await readFile(configFile,'utf8')),browser=config.native.find(n=>n.actor===game.actor),cdp=await connect({port:browser.port,target:browser.target,url:config.origin+'/',helper:config.browserTools});
  try{
    const action=game.act({keys:[],duration_ms:2200});
    const input=(async()=>{const deadline=Date.now()+5000;while((await game.observe()).paused){assert(Date.now()<deadline);await delay(20);}const enter=async()=>{for(const type of ['keyDown','keyUp'])await cdp.call('Input.dispatchKeyEvent',{type,key:'Enter',code:'Enter',windowsVirtualKeyCode:13});};await enter();await cdp.call('Input.insertText',{text:'/tutorial'});await enter();})();
    const results=await Promise.allSettled([action,input]);for(const r of results)if(r.status==='rejected')throw r.reason;
  }finally{cdp.close();}
  const returned=await record('reentry');
  game.check('ordinary tutorial command returns the same living character with earned ring',returned.view.map.name==='Tutorial'&&returned.view.lifecycle.alive&&returned.view.session.character===initial.view.session.character&&returned.view.character.items[3]===0xa4f&&returned.view.character.inventory_revision===used.view.character.inventory_revision);
  game.check('the early lesson participants also remain alive',(await first.observe()).lifecycle.alive&&(await health.observe()).lifecycle.alive);
}
