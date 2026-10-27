import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {connect} from '../../scripts/record-demo.mjs';
import {setTimeout as wait} from 'node:timers/promises';

let connection;
const lists=[], requests=new Set();
export async function beforeNavigate({browser}) {
  connection=await connect({...browser,url:'about:blank',helper:join(process.env.PI_BROWSER_TOOLS,'direct-cdp.mjs'),onEvent:event=>{
    if(event.method==='Network.responseReceived'&&event.params.response.url.endsWith('/legacy-api/char/list'))requests.add(event.params.requestId);
    if(event.method==='Network.loadingFinished'&&requests.delete(event.params.requestId))connection.call('Network.getResponseBody',{requestId:event.params.requestId}).then(result=>lists.push(result.body));
  }});
  await connection.call('Network.enable');
  await connection.call('Emulation.setFocusEmulationEnabled',{enabled:true});
}

export const fixture = {
  class:0x30e, level:1, seed:746, world:-2, position:'spawn',
  equipment:[0xa97,0xa2e,-1,-1], backpack:Array(8).fill(-1), stats:null,
};
export async function run(game) {
  const root=join(process.env.QA_RESULTS_DIR,'native');
  const {origin}=JSON.parse(await readFile(join(root,'result.json'),'utf8'));
  const token=(await readFile(join(root,'state/control.token'),'utf8')).trim();
  const before=await game.observe();
  const profile=process.env.GUEST_BASELINE==='1'?'BoxxyCC':'guest:v1:1';
  const response=await fetch(origin+'/dev/testing',{method:'POST',headers:{Authorization:`Bearer ${token}`,'Content-Type':'application/json'},body:JSON.stringify({id:'guest-persona',command:{op:'persona',profile,world:before.session.world,position:{x:before.position.x+2,y:before.position.y}}})});
  const provisioned=await response.json();
  game.check('ordinary persona provisioning succeeded',response.ok&&provisioned.status!=='failed');
  await game.act({keys:[],duration_ms:2500});
  const view=await game.observe();
  const peer=(await game.inspect()).view.objects.find(p=>p.account===provisioned.result.account);
  game.check('native observer sees the actual provisioned player',Boolean(peer));
  if(process.env.GUEST_BASELINE!=='1') {
    const name=`Guest${provisioned.result.account}`;
    game.check('guest player has account-visible name',peer.name===name);
    const feeds=await (await fetch(origin+'/api/bots')).json();
    const feed=feeds.bots.find(b=>b.name===name);
    game.check('CCTV binds the account-visible guest name, not its registry slot',Boolean(feed)&&!feeds.bots.some(b=>b.name===profile));
    game.check('guest begins as a living level-one Wizard',feed.frame.class==='Wizard'&&feed.frame.level===1&&feed.frame.hp>0);
    await writeFile(join(root,'guest-identity.json'),JSON.stringify({profile,provisioned,peer,feed},null,2));
  }
  game.check('observer remains the same living ordinary guest',view.lifecycle.alive&&view.session.character===before.session.character);
  game.check('native guest list has no NameChosen claim',lists.length===1&&!lists[0].includes('<NameChosen/>'));
  await writeFile(join(root,'native-account.xml'),lists[0]);
  const rect=await connection.eval('(()=>{const r=document.querySelector("ruffle-player").getBoundingClientRect();return{x:r.x,y:r.y};})()');
  await connection.call('Input.dispatchMouseEvent',{type:'mouseMoved',x:rect.x+680,y:rect.y+518});
  await game.act({keys:['d'],duration_ms:8000},{name:'guest-persona',when:v=>v.game_ms>=view.game_ms+1000});
  const rendered=await game.observe();
  game.check('native AFTER advances ordinary movement',Math.hypot(rendered.position.x-view.position.x,rendered.position.y-view.position.y)>0.3);
  if(process.env.GUEST_BASELINE!=='1') {
    const reconnectBefore=await game.observe();
    const reload=(async()=>{
      const movingDeadline=Date.now()+10000;
      let moved=false;
      while(Date.now()<movingDeadline) {
        const current=await game.observe();
        if(!current.paused&&Math.hypot(current.position.x-reconnectBefore.position.x,current.position.y-reconnectBefore.position.y)>0.3){moved=true;break;}
        await wait(100);
      }
      game.check('reconnect starts only after actual bounded native input',moved);
      const previousDocument=await connection.eval('performance.timeOrigin');
      await connection.call('Page.reload');
      const deadline=Date.now()+8000;
      while(Date.now()<deadline&&!await connection.eval(`performance.timeOrigin!==${previousDocument}&&document.querySelector("ruffle-player")?.ruffle().isPlaying===true`))await wait(100);
      game.check('reconnect loaded a new playing native document',await connection.eval(`performance.timeOrigin!==${previousDocument}&&document.querySelector("ruffle-player")?.ruffle().isPlaying===true`));
      await connection.eval('(()=>{const p=document.querySelector("ruffle-player");const m=p.shadowRoot.querySelector("#hardware-acceleration-modal");if(m&&!m.classList.contains("hidden"))m.querySelector(".close-modal").click();})()');
      for(let i=0;i<2;i++) {
        const r=await connection.eval('(()=>{const r=document.querySelector("ruffle-player").getBoundingClientRect();return{x:r.x+400,y:r.y+550};})()');
        const p={...r,button:'left',clickCount:1};
        await connection.call('Input.dispatchMouseEvent',{type:'mousePressed',buttons:1,...p});
        await connection.call('Input.dispatchMouseEvent',{type:'mouseReleased',buttons:0,...p});
        await connection.eval('new Promise(ok=>requestAnimationFrame(()=>requestAnimationFrame(ok)))');
      }
    })();
    await Promise.all([game.act({keys:['d'],duration_ms:10000},{name:'observer-reconnect',when:v=>v.session.player!==reconnectBefore.session.player&&v.game_ms>=reconnectBefore.game_ms+6000}),reload]);
    const after=await game.observe();
    game.check('ordinary native observer reload resumes its own account and living character',after.lifecycle.alive&&after.session.account===before.session.account&&after.session.character===before.session.character&&after.session.player!==before.session.player);
    game.check('ordinary native observer reload preserves level and equipment',after.character.level===before.character.level&&JSON.stringify(after.character.items)===JSON.stringify(before.character.items));
    game.check('reconnected native observer guest still has no chosen-name claim',lists.length===2&&!lists[1].includes('<NameChosen/>'));
  }
  connection.close();
}
