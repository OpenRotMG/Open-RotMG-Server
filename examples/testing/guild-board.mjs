import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
import {setTimeout as delay} from 'node:timers/promises';
import {connect} from '../../scripts/record-demo.mjs';

export const fixture={class:0x30e,level:5,seed:914,world:-2,position:{x:76.5,y:77.1},equipment:[-1,-1,-1,-1],backpack:Array(8).fill(-1),stats:null};
const clients=[];
export async function beforeNavigate({browser,signal}){
  const responses=[],pending=new Map(),credentials={};let c;
  c=await connect({...browser,url:'about:blank',helper:join(process.env.PI_BROWSER_TOOLS,'direct-cdp.mjs'),onEvent:e=>{
    if(e.method==='Network.requestWillBeSent'&&/\/guild\/(getBoard|setBoard)$/.test(e.params.request.url)){
      const form=new URLSearchParams(e.params.request.postData);assert(form.has('guid')&&form.has('password'),'Native guild request lacks credentials');
      credentials.guid=form.get('guid');credentials.password=form.get('password');
    }
    if(e.method==='Network.responseReceived'&&/\/guild\/(getBoard|setBoard)$/.test(e.params.response.url))pending.set(e.params.requestId,e.params.response.url.split('/').at(-1));
    if(e.method==='Network.loadingFinished'&&pending.has(e.params.requestId)){
      const op=pending.get(e.params.requestId);pending.delete(e.params.requestId);
      c.call('Network.getResponseBody',{requestId:e.params.requestId}).then(r=>responses.push({op,text:r.base64Encoded?Buffer.from(r.body,'base64').toString():r.body})).catch(error=>responses.push({op,error:String(error)}));
    }
  }});
  await c.call('Network.enable');signal.addEventListener('abort',()=>c.close(),{once:true});clients.push({c,responses,credentials});
}
export async function run(game){
  const out=process.env.QA_RESULTS_DIR,root=join(out,'native'),peer=await game.join({...fixture,seed:915});
  const config=JSON.parse(await readFile(join(root,'native.json'),'utf8')),token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const api=async command=>{const r=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command})});const v=await r.json();assert(r.ok&&v.status==='succeeded',JSON.stringify(v));return v;};
  async function click(i,x,y){
    const {c}=clients[i],r=await c.eval('(()=>{const r=document.querySelector("ruffle-player").getBoundingClientRect();return{x:r.x,y:r.y};})()'),p={x:r.x+x,y:r.y+y,button:'left',clickCount:1};
    await c.call('Input.dispatchMouseEvent',{type:'mouseMoved',...p});
    try{await c.call('Input.dispatchMouseEvent',{type:'mousePressed',buttons:1,...p});}finally{await c.call('Input.dispatchMouseEvent',{type:'mouseReleased',buttons:0,...p});}
  }
  async function ui(player,i,name,work){
    let finished=false;const offset=(await readFile(join(root,'controller.stderr'),'utf8')).length;
    const action=player.act({keys:[],duration_ms:5000},{name,when:()=>finished});
    try{
      const deadline=Date.now()+10000;
      while(true){
        const lines=(await readFile(join(root,'controller.stderr'),'utf8')).slice(offset).split('\n').slice(0,-1),acked=new Set(lines.filter(l=>l.startsWith('{"event":"native_sync_verified_ack"')).map(l=>JSON.parse(l).context.actor));
        if(config.native.every(c=>acked.has(c.actor))&&!(await player.observe()).paused)break;
        assert(Date.now()<deadline,'Native UI cohort did not resume');await delay(20);
      }
      await work();await delay(400);finished=true;
    }finally{await action;await writeFile(join(out,'board-responses.json'),JSON.stringify(clients.map(c=>c.responses),null,2));}
  }
  await api({op:'cheat',actor:game.actor,change:{kind:'guild',name:'Board Research',members:[peer.actor]}});
  await game.act({keys:[],duration_ms:500});
  await ui(game,0,'board-before',async()=>{});
  await ui(game,0,'guild-hall-founder',()=>click(0,700,575));
  await ui(peer,1,'guild-hall-member',()=>click(1,700,575));
  game.check('two ordinary clients enter the same guild hall',(await game.observe()).world==='Guild Hall'&&(await peer.observe()).session.world===(await game.observe()).session.world);
  async function walk(player,x,y){
    for(let n=0;n<30;n++){
      const p=(await player.observe()).position,dx=x-p.x,dy=y-p.y,d=Math.hypot(dx,dy);if(d<.3)return;
      const sx=(dx-dy)/Math.SQRT2,sy=(dx+dy)/Math.SQRT2,keys=[];
      if(Math.abs(sx)>.1)keys.push(sx>0?'d':'a');if(Math.abs(sy)>.1)keys.push(sy>0?'s':'w');
      await player.act({keys,duration_ms:Math.min(1500,Math.max(50,Math.round(d*140)))});
    }
    throw Error(`Walk to ${x},${y} failed: ${JSON.stringify((await player.observe()).position)}`);
  }
  await walk(game,11.5,12.5);await walk(game,11.5,9.2);
  await walk(peer,11.5,12.5);await walk(peer,11.5,9.2);
  await ui(game,0,'board-founder-empty',()=>click(0,700,575));
  await ui(peer,1,'board-member-empty',()=>click(1,700,575));
  game.check('both members load the same initially empty board',clients.every(c=>c.responses.some(r=>r.op==='getBoard'&&r.text==='')));
  await ui(game,0,'board-edit',()=>click(0,250,510));
  await ui(game,0,'board-draft',async()=>{await click(0,270,120);await clients[0].c.call('Input.insertText',{text:'Meet at 10:00 + bring 50% & <friends>!'});});
  // Cancel returns to the cached text without saving: selected 123.5.1 _6e/_4L_.as at https://github.com/Zolmex/kithio-as3-source/tree/d15f321edcce0ef890001890c6991c25a1b0f718
  await ui(game,0,'board-cancelled',()=>click(0,250,510));
  game.check('Cancel sends no setBoard request',clients[0].responses.every(r=>r.op!=='setBoard'));
  await ui(game,0,'board-edit-again',()=>click(0,250,510));
  const text='Meet at 10:00 + bring 50% & <friends>!';
  await ui(game,0,'board-saved',async()=>{await click(0,270,120);await clients[0].c.call('Input.insertText',{text});await click(0,550,510);});
  game.check('Save returns the exact text including plus percent ampersand and angle brackets',clients[0].responses.some(r=>r.op==='setBoard'&&r.text===text));
  await ui(peer,1,'board-member-stale',async()=>{});
  await ui(peer,1,'board-member-closed',()=>click(1,550,510));
  await ui(peer,1,'board-member-saved',()=>click(1,700,575));
  game.check('member sees the shared saved text on reopening',clients[1].responses.at(-1).text===text);
  await ui(game,0,'board-founder-closed',()=>click(0,550,510));
  const request=async(credentials,op,board)=>{
    const response=await fetch(config.origin+'/legacy-api/guild/'+op,{method:'POST',body:new URLSearchParams({...credentials,...(board===undefined?{}:{board})})});
    return {body:await response.text(),type:response.headers.get('content-type'),cache:response.headers.get('cache-control')};
  };
  const founder=clients[0].credentials,member=clients[1].credentials;
  const denied=await request(member,'setBoard','forged member edit');
  game.check('authenticated lower rank cannot forge an edit',denied.body.includes('Only Officers')&&(await request(founder,'getBoard')).body===text);
  const outsider={guid:'board-outsider-'+randomUUID(),password:''};
  await fetch(config.origin+'/legacy-api/char/list',{method:'POST',body:new URLSearchParams(outsider)});
  game.check('authenticated nonmember cannot read or write', (await request(outsider,'getBoard')).body.includes('not in a guild')&&(await request(outsider,'setBoard','forged')).body.includes('not in a guild'));
  game.check('wrong credentials cannot read or write',(await request({...founder,password:'invalid'},'getBoard')).body.includes('credentials not valid')&&(await request({...founder,password:'invalid'},'setBoard','forged')).body.includes('credentials not valid'));
  const literal='  e\u0301 + %2B & <tag>\r\n日本語\t🧙  ';
  const literalReply=await request(founder,'setBoard',literal);
  game.check('API preserves whitespace Unicode combining marks CRLF and literal form characters',literalReply.body===literal&&literalReply.type==='text/plain; charset=utf-8'&&literalReply.cache==='no-store'&&(await request(member,'getBoard')).body===literal);
  for(const board of ['x'.repeat(4096),'🧙'.repeat(2048)])game.check('4096 UTF16 units save exactly '+board.length,(await request(founder,'setBoard',board)).body===board);
  for(const board of ['x'.repeat(4097),'🧙'.repeat(2048)+'x','bad\u0000control'])game.check('invalid board is refused without a write '+board.length,(await request(founder,'setBoard',board)).body.includes('<Error>')&&(await request(member,'getBoard')).body==='🧙'.repeat(2048));
  const stale=(await request(founder,'getBoard')).body;
  await request(founder,'setBoard','first save');await request(founder,'setBoard','stale editor replaces '+stale.slice(0,2));
  game.check('current API is last writer wins without a revision precondition',(await request(member,'getBoard')).body==='stale editor replaces 🧙');
  await request(founder,'setBoard',text);
  const beforeReconnect=await game.observe();
  await game.act({keys:['Insert'],duration_ms:1000});
  await game.act({keys:[],duration_ms:1000});
  game.check('ordinary Nexus reconnect preserves the same living character',(await game.observe()).world==='Nexus'&&(await game.observe()).lifecycle.alive&&(await game.observe()).session.character===beforeReconnect.session.character);
  await walk(game,76.5,77.1);
  await ui(game,0,'board-hall-reentry',()=>click(0,700,575));
  await walk(game,11.5,12.5);await walk(game,11.5,9.2);
  await ui(game,0,'board-after-reconnect',()=>click(0,700,575));
  game.check('reconnect and hall reentry retain saved board',clients[0].responses.at(-1).text===text);
  game.check('both ordinary native participants remain alive',(await game.observe()).lifecycle.alive&&(await peer.observe()).lifecycle.alive);
}
