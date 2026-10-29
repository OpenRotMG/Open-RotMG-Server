import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {connect} from '../../scripts/record-demo.mjs';

export const fixture={class:0x30e,level:1,seed:930,world:-1,position:{x:12.5,y:164.5},equipment:[0xa97,0xa2e,-1,-1],backpack:Array(8).fill(-1),stats:null};
let browser;
const root=()=>process.env.QA_RESULTS_DIR;
const open=()=>connect({...browser,helper:join(process.env.PI_BROWSER_TOOLS,'direct-cdp.mjs')});
export async function beforeNavigate(context){
  browser=context.browser;
  const cdp=await open();
  try{await cdp.call('Page.addScriptToEvaluateOnNewDocument',{source:`(${networkFaults.toString()})()`});}finally{cdp.close();}
}
function networkFaults(){
  const Socket=WebSocket,timer=setTimeout,rows=[];
  window.reconnectQA=rows;
  window.WebSocket=class extends Socket{
    constructor(...args){
      super(...args);
      if(new URL(args[0]).pathname!=='/game')return;
      const row={socket:rows.length+1,opened:performance.now(),held:0,released:null,dropped:false};rows.push(row);
      if(row.socket===2){
        const queued=[],replayed=new WeakSet();let holding=true;
        this.addEventListener('message',event=>{
          if(!holding||replayed.has(event))return;
          event.stopImmediatePropagation();queued.push(event);row.held++;
        });
        timer(()=>{
          holding=false;row.released=performance.now();
          for(const event of queued){const copy=new MessageEvent('message',{data:event.data,origin:event.origin});replayed.add(copy);this.dispatchEvent(copy);}
        },1500);
      }
      if(row.socket===3)this.addEventListener('open',event=>{event.stopImmediatePropagation();row.dropped=true;this.close(1000,'owned reconnect interruption');});
    }
  };
}
export async function run(game){
  const observations=[];
  async function save(name){const view=await game.observe();observations.push({name,view});await writeFile(join(root(),'reconnect.json'),JSON.stringify(observations,null,2));return view;}
  const initial=await save('tutorial');
  await game.act({keys:[],duration_ms:1000},{name:'tutorial-before',when:v=>v.world==='Tutorial'&&v.game_ms>=initial.game_ms+300});
  await game.act({keys:['Insert'],duration_ms:3500});
  const home=await save('delayed-nexus');assert.equal(home.world,'Nexus');
  await game.act({keys:[],duration_ms:1200},{name:'delayed-nexus',when:v=>v.world==='Nexus'&&v.game_ms>=home.game_ms+500});
  const cdp=await open();
  try{
    const delayed=await cdp.eval('window.reconnectQA');
    game.check('destination initialization tolerates an ordered 1500ms network delay',delayed.length===2&&delayed[1].held>0&&delayed[1].released-delayed[1].opened>=1450);
    await game.act({keys:['d'],duration_ms:400});const moved=await save('nexus-moved');
    game.check('delayed destination releases input and remains playable',moved.world==='Nexus'&&Math.hypot(moved.position.x-home.position.x,moved.position.y-home.position.y)>.3);
    await game.act({keys:['Enter'],duration_ms:50});
    await cdp.call('Input.insertText',{text:'/tutorial'});
    await game.act({keys:['Enter'],duration_ms:3500});
    const returned=await save('retried-tutorial');assert.equal(returned.world,'Tutorial');
    await game.act({keys:[],duration_ms:1200},{name:'retried-tutorial',when:v=>v.world==='Tutorial'&&v.game_ms>=returned.game_ms+500});
    const faults=await cdp.eval('window.reconnectQA');await writeFile(join(root(),'network-faults.json'),JSON.stringify(faults,null,2));
    game.check('one interrupted pre-HELLO transfer retries through the ordinary client',faults.length===4&&faults[2].dropped&&!faults[3].dropped);
    // The selected client carries charId into RECONNECT, not CREATE: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Rotmg123.5.1/src/com/company/assembleegameclient/net/_1f.as#L1207-L1210
    game.check('both transfers retain the same living character exact inventory revision and XP',[home,returned].every(v=>v.lifecycle.alive&&v.session.account===initial.session.account&&v.session.character===initial.session.character&&v.session.player!==initial.session.player&&JSON.stringify(v.character.items)===JSON.stringify(initial.character.items)&&v.character.inventory_revision===initial.character.inventory_revision&&v.character.experience===initial.character.experience));
    await game.act({keys:['d'],duration_ms:400});const final=await save('tutorial-moved');
    game.check('retried destination releases input and remains playable',final.world==='Tutorial'&&Math.hypot(final.position.x-returned.position.x,final.position.y-returned.position.y)>.3);
    const lines=(await readFile(join(root(),'native/server.log'),'utf8')).split('\n').filter(l=>l.startsWith('{')).map(l=>JSON.parse(l));
    const grants=message=>lines.filter(r=>r.fields?.message===message&&r.fields.account===initial.session.account);
    game.check('interruption neither duplicates nor replays transfer grants',grants('transfer grant issued').length===2&&grants('transfer grant consumed').length===2);
  }finally{cdp.close();}
}
