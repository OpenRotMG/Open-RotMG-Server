import assert from 'node:assert/strict';
import {writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {setTimeout as delay} from 'node:timers/promises';
import {connect} from '../../scripts/record-demo.mjs';
export const fixture={class:0x30e,level:20,seed:905,world:-2,position:'spawn',equipment:[-1,-1,-1,-1],backpack:Array(8).fill(-1),stats:null};
const clients=[],events=[];
async function until(test,label){const end=Date.now()+30000;while(!await test()){assert(Date.now()<end,label);await delay(30);}}
function textPacket(bytes){
  let i=0;const utf=()=>{const n=bytes.readUInt16BE(i);i+=2;const s=bytes.toString('utf8',i,i+n);i+=n;return s;};
  const name=utf();i+=9;return {name,recipient:utf(),text:utf()};
}
export async function beforeNavigate({browser,signal}){
  const client={browser,name:['Whispa','Whispb','Whispc'][clients.length],texts:[],sent:[],lists:0},streams=new Map();assert(client.name);
  client.cdp=await connect({port:browser.port,target:browser.target,url:'about:blank',helper:process.env.PI_BROWSER_TOOLS+'/direct-cdp.mjs',onEvent:e=>{
    if(e.method==='Network.requestWillBeSent'&&e.params.request.url.endsWith('/legacy-api/char/list')){client.request=e.params.requestId;client.lists++;}
    if(!['Network.webSocketFrameReceived','Network.webSocketFrameSent'].includes(e.method)||e.params.response.opcode!==2)return;
    try{
      const incoming=e.method.endsWith('Received'),stream=e.params.requestId+incoming;
      if(!streams.has(stream)){
        const key=Buffer.from(incoming?'72c5583cafb6818995cbd74b80':'311f80691451c71b09a13a2a6e','hex'),s=Array.from({length:256},(_,i)=>i);let j=0;
        for(let i=0;i<256;i++){j=(j+s[i]+key[i%key.length])&255;[s[i],s[j]]=[s[j],s[i]];}
        streams.set(stream,{s,i:0,j:0,buffer:Buffer.alloc(0)});
      }
      const state=streams.get(stream);state.buffer=Buffer.concat([state.buffer,Buffer.from(e.params.response.payloadData,'base64')]);assert(state.buffer.length<=2097152);
      while(state.buffer.length>=5){
        const n=state.buffer.readInt32BE();assert(n>=5&&n<=1048576);if(state.buffer.length<n)break;
        const id=state.buffer[4],body=Buffer.from(state.buffer.subarray(5,n));state.buffer=state.buffer.subarray(n);
        for(let k=0;k<body.length;k++){state.i=(state.i+1)&255;state.j=(state.j+state.s[state.i])&255;[state.s[state.i],state.s[state.j]]=[state.s[state.j],state.s[state.i]];body[k]^=state.s[(state.s[state.i]+state.s[state.j])&255];}
        if(incoming&&id===25)client.texts.push(textPacket(body));
        if(!incoming&&id===69)client.sent.push(body.toString('utf8',2,2+body.readUInt16BE(0)));
      }
    }catch(error){client.error=String(error);}
  }});signal.addEventListener('abort',()=>client.cdp.close(),{once:true});clients.push(client);
  await client.cdp.call('Network.enable',{maxPostDataSize:65536});
}
export async function enter({browser}){
  const c=clients.find(c=>c.browser.target===browser.target),cdp=c.cdp;await until(()=>c.request,'Observed char/list request required');
  const {postData}=await cdp.call('Network.getRequestPostData',{requestId:c.request});c.guid=new URLSearchParams(postData).get('guid');assert(c.guid,'Observed disposable guest GUID required');
  const named=await cdp.eval(`fetch('/legacy-api/account/setName',{method:'POST',body:new URLSearchParams(${JSON.stringify({guid:c.guid,password:'',name:c.name})})}).then(async r=>({ok:r.ok,text:await r.text()}))`);
  assert(named.ok&&named.text.includes('<Success'),JSON.stringify(named));
  const lists=c.lists,guid=c.guid;await cdp.call('Page.reload');
  await until(()=>c.lists>lists,'same guest reload');
  const reloaded=await cdp.call('Network.getRequestPostData',{requestId:c.request});assert.equal(new URLSearchParams(reloaded.postData).get('guid'),guid);
  await until(()=>cdp.eval('document.querySelector("ruffle-player")?.ruffle().isPlaying===true'),'named menu reload');
  await cdp.eval('(()=>{const m=document.querySelector("ruffle-player").shadowRoot.querySelector("#hardware-acceleration-modal");if(m&&!m.classList.contains("hidden"))m.querySelector(".close-modal").click();})()');
  await cdp.call('Emulation.setFocusEmulationEnabled',{enabled:true});
  for(let i=0;i<2;i++){
    const r=await cdp.eval('(()=>{const r=document.querySelector("ruffle-player").getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height}})()');assert(r.width===800&&r.height===600);
    const p={x:r.x+400,y:r.y+550,button:'left',clickCount:1};
    try{await cdp.call('Input.dispatchMouseEvent',{type:'mousePressed',buttons:1,...p});}finally{await cdp.call('Input.dispatchMouseEvent',{type:'mouseReleased',buttons:0,...p});}
    await cdp.eval('new Promise(ok=>requestAnimationFrame(()=>requestAnimationFrame(ok)))');
  }
}
export async function run(game){
  const out=process.env.QA_RESULTS_DIR;
  const other=await game.join({...fixture,world:-1}),third=await game.join(fixture);
  const [a,b,c]=clients;let serial=0;
  const send=async(c,method,params)=>{assert(c.inputOpen&&Date.now()<c.deadline,'Native input outside its bounded action');const view=await c.game.observe();assert(!view.paused&&view.game_ms<c.endGameMs,'Native input missed its active window');return c.cdp.call(method,params);};
  const key=async(c,key,code,n,modifiers=0)=>{try{await send(c,'Input.dispatchKeyEvent',{type:'keyDown',key,code,windowsVirtualKeyCode:n,modifiers});}finally{await c.cdp.call('Input.dispatchKeyEvent',{type:'keyUp',key,code,windowsVirtualKeyCode:n,modifiers});}};
  async function input(g,c,work,label){
    let finished=false,done=false;const before=await g.observe();
    const action=g.act({keys:[],duration_ms:6000},{name:label+'-'+serial++,when:v=>finished&&v.game_ms>before.game_ms+1800});
    const closed=()=>{done=true;c.inputOpen=false;};action.then(closed,closed);
    const typing=(async()=>{
      await until(async()=>!(await g.observe()).paused&&await c.cdp.eval('document.querySelector("ruffle-player").ruffle().isPlaying'),'bounded native resume');
      assert(!done,'Native action ended before input');c.game=g;c.inputOpen=true;c.deadline=Date.now()+4000;c.endGameMs=before.game_ms+5000;
      await c.cdp.eval('document.querySelector("ruffle-player").focus({preventScroll:true})');
      try{await work();finished=true;}finally{c.inputOpen=false;}
    })();
    const results=await Promise.allSettled([action,typing]);for(const r of results)if(r.status==='rejected')throw r.reason;
    for(const c of clients)assert(!c.error,c.error);
    events.push({label,actor:g.actor,sent:[...c.sent],texts:clients.map(c=>({name:c.name,texts:[...c.texts]}))});
  }
  const say=(g,c,text,label)=>input(g,c,async()=>{await key(c,'Enter','Enter',13);await send(c,'Input.insertText',{text});await key(c,'Enter','Enter',13);},label);
  try{
    const initial=await game.observe(),initialOther=await other.observe();
    game.check('ordinary clients occupy distinct worlds with a living bystander',initial.world==='Nexus'&&initialOther.world!=='Nexus'&&initial.lifecycle.alive&&initialOther.lifecycle.alive&&(await third.observe()).lifecycle.alive);
    await input(game,a,async()=>{await key(a,'Enter','Enter',13);await send(a,'Input.insertText',{text:'/tell Whispb cancelled'});await key(a,'a','KeyA',65,2);await key(a,'Backspace','Backspace',8);await key(a,'Enter','Enter',13);},'cancelled-composition');
    game.check('clearing a composition sends no private message',!a.sent.some(t=>t.includes('cancelled'))&&!b.texts.some(t=>t.text==='cancelled'));
    await say(game,a,'/tell wHiSpB private-one','sender-echo');
    game.check('case insensitive cross-world tell reaches recipient and echoes once',[a,b].every(c=>c.texts.filter(t=>t.name==='Whispa'&&t.recipient==='Whispb'&&t.text==='private-one').length===1));
    await input(other,b,async()=>{await key(b,'Tab','Tab',9);await send(b,'Input.insertText',{text:'reply-two'});await key(b,'Enter','Enter',13);},'recipient-tab-reply');
    game.check('native Tab replies to incoming counterpart',b.sent.includes('/tell Whispa reply-two')&&a.texts.some(t=>t.name==='Whispb'&&t.recipient==='Whispa'&&t.text==='reply-two'));
    await say(game,a,'/tell Whispc private-third','same-world-tell');
    game.check('same-world tell remains private to sender and recipient',c.texts.some(t=>t.name==='Whispa'&&t.recipient==='Whispc'&&t.text==='private-third')&&!b.texts.some(t=>t.text==='private-third')&&!c.texts.some(t=>t.text==='private-one'||t.text==='reply-two'));
    // The selected TextBox clamps both history directions: https://openrotmg.org/restoration/reference-data/client-123.5.1.swf (§_-4D§.§_-03z§).
    await input(game,a,async()=>{await key(a,'Tab','Tab',9);await key(a,'Tab','Tab',9);await key(a,'Tab','Tab',9);},'history-oldest-clamp');
    await input(game,a,async()=>{await key(a,'Tab','Tab',9,8);await key(a,'Tab','Tab',9,8);await send(a,'Input.insertText',{text:'history-counterpart'});await key(a,'Enter','Enter',13);},'history-newest-clamp');
    game.check('Tab and Shift Tab navigate two counterpart names without wrapping',a.sent.at(-1)==='/tell Whispc history-counterpart'&&c.texts.some(t=>t.text==='history-counterpart')&&!b.texts.some(t=>t.text==='history-counterpart'));
    await say(game,a,'/tell Nobodyexists offline-three','offline-refused');
    game.check('unknown recipient fails visibly without success echo',a.texts.some(t=>t.text==='Player not found.')&&!a.texts.some(t=>t.text==='offline-three'));
    await say(game,a,'/tell Whispa self-four','self-refused');
    game.check('self tell fails visibly',a.texts.some(t=>t.text==='You cannot tell yourself.'));
    await say(game,a,'/tell Whispb','syntax-refused');
    game.check('missing message fails visibly',a.texts.some(t=>t.text==='Usage: /tell <player> <message>'));
    await say(other,b,'/ignore Whispa','ignore');
    const before=b.texts.length;
    await say(game,a,'/t Whispb hidden-five','ignored-sender-echo');
    game.check('ignore suppresses recipient while preserving sender echo',a.texts.some(t=>t.text==='hidden-five')&&!b.texts.slice(before).some(t=>t.text==='hidden-five'));
    await say(other,b,'/unignore Whispa','unignore');
    await say(game,a,'/t Whispb restored-six','unignored-delivery');
    game.check('unignore restores ordinary alias delivery',b.texts.some(t=>t.text==='restored-six'));
    await input(game,a,async()=>{await key(a,'Tab','Tab',9);},'sender-tab-history');
    await input(game,a,async()=>{await key(a,'Enter','Enter',13);},'empty-history-refused');
    game.check('outgoing echo remains a native reply counterpart',a.sent.at(-1)==='/tell Whispb ');
    await other.act({keys:['Insert'],duration_ms:1500});
    game.check('normal Nexus reconnect preserves the recipient character',(await other.observe()).world==='Nexus'&&(await other.observe()).lifecycle.alive&&(await other.observe()).session.character===initialOther.session.character);
    await input(other,b,async()=>{await key(b,'Tab','Tab',9);},'reconnected-history');
    await input(other,b,async()=>{await key(b,'Enter','Enter',13);},'reconnected-empty-prefix');
    game.check('fresh GameSprite resets counterpart history',b.sent.at(-1)==='/tell ');
    await say(game,a,'/tell Whispb reconnected-seven','reconnected-sender');
    await input(other,b,async()=>{await key(b,'Tab','Tab',9);await send(b,'Input.insertText',{text:'reconnected-eight'});await key(b,'Enter','Enter',13);},'reconnected-recipient');
    game.check('both clients exchange new whispers after reconnect',b.texts.some(t=>t.text==='reconnected-seven')&&a.texts.some(t=>t.text==='reconnected-eight'));
  }finally{
    await writeFile(join(out,'whisper-observations.json'),JSON.stringify({preparedNames:true,events,clients:clients.map(({name,sent,texts})=>({name,sent,texts}))},null,2));
    for(const c of clients)c.cdp.close();
  }
}
