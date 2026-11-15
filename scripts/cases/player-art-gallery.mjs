import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {setTimeout as delay} from 'node:timers/promises';
import {createHash} from 'node:crypto';
import {crc32,deflateSync,inflateSync} from 'node:zlib';
import {connect} from '../../scripts/record-demo.mjs';
export {fixture} from './client-entry.mjs';
const checks=[];
export async function enter({browser,signal}){
  const out=process.env.QA_RESULTS_DIR,{origin}=JSON.parse(await readFile(join(out,'native/result.json'),'utf8'));
  const traffic=[],pending=new Map(),accountRequests=new Set();let cdp,accountLoads=0,readyLoads=0;
  const check=(name,ok)=>{assert(ok,name);checks.push(name);};
  const wait=async(fn,label)=>{for(let i=0;i<300;i++){signal.throwIfAborted();const value=await fn();if(value)return value;await delay(100,undefined,{signal});}throw Error(label);};
  cdp=await connect({...browser,url:origin+'/',helper:join(process.env.PI_BROWSER_TOOLS,'direct-cdp.mjs'),onEvent:e=>{
    if(e.method==='Network.requestWillBeSent'&&e.params.request.url===origin+'/legacy-api/char/list')accountRequests.add(e.params.requestId);
    if(e.method==='Network.loadingFinished'&&accountRequests.has(e.params.requestId))accountLoads++;
    if(e.method==='Network.requestWillBeSent'&&e.params.request.url.startsWith(origin+'/legacy-api/picture/')){
      const r={id:e.params.requestId,url:e.params.request.url};traffic.push(r);pending.set(r.id,r);
      cdp.call('Network.getRequestPostData',{requestId:r.id}).then(v=>r.form=Buffer.from(v.postData,v.base64Encoded?'base64':'utf8').toString('utf8')).catch(e=>r.error=String(e));
    }
    const r=pending.get(e.params?.requestId);
    if(r&&e.method==='Network.responseReceived')r.status=e.params.response.status;
    if(r&&e.method==='Network.loadingFinished')cdp.call('Network.getResponseBody',{requestId:r.id}).then(v=>r.body=v).catch(e=>r.error=String(e));
    if(r&&e.method==='Network.loadingFailed')r.error=e.params.errorText;
  }});
  const response=async(op,from=0)=>wait(()=>{const r=traffic.slice(from).find(r=>r.url.endsWith('/'+op));if(r?.error)throw Error(r.error);return r?.body&&r.form!==undefined?r:null;},'Missing native '+op+' response');
  let rect;
  const frames=()=>cdp.eval('new Promise(ok=>requestAnimationFrame(()=>requestAnimationFrame(ok)))');
  const click=async(x,y)=>{const p={x:rect.x+x,y:rect.y+y,button:'left',clickCount:1};await cdp.call('Input.dispatchMouseEvent',{type:'mouseMoved',x:p.x,y:p.y});try{await cdp.call('Input.dispatchMouseEvent',{type:'mousePressed',buttons:1,...p});}finally{await cdp.call('Input.dispatchMouseEvent',{type:'mouseReleased',buttons:0,...p});}await frames();};
  const key=async(key)=>{const p={key,code:'Key'+key.toUpperCase(),windowsVirtualKeyCode:key.toUpperCase().charCodeAt(0)};await cdp.call('Input.dispatchKeyEvent',{type:'keyDown',...p});await cdp.call('Input.dispatchKeyEvent',{type:'keyUp',...p});await frames();};
  const capture=async(name)=>writeFile(join(out,name+'.png'),Buffer.from((await cdp.call('Page.captureScreenshot',{format:'png'})).data,'base64'));
  const ready=async()=>{
    await wait(()=>accountLoads>readyLoads,'Native account refresh did not finish');readyLoads=accountLoads;
    await wait(()=>cdp.eval('document.querySelector("ruffle-player")?.ruffle().isPlaying===true'),'Ruffle not playing');await frames();
    await cdp.eval('(()=>{const p=document.querySelector("ruffle-player"),m=p.shadowRoot.querySelector("#hardware-acceleration-modal");if(m&&!m.classList.contains("hidden"))m.querySelector(".close-modal").click();p.focus();})()');
    rect=await cdp.eval('(()=>{const r=document.querySelector("ruffle-player").getBoundingClientRect();return {x:r.x,y:r.y,width:r.width,height:r.height};})()');assert(rect.width===800&&rect.height===600);
  };
  const post=async(op,form)=>{const r=await fetch(origin+'/legacy-api/'+op,{method:'POST',body:form,signal});const text=await r.text();assert.equal(r.status,200,text);return text;};
  const ids=xml=>[...xml.matchAll(/<Pic id="(\d+)"/g)].map(m=>Number(m[1]));
  const list=async(action,count)=>{const from=traffic.length;await action();const r=await response('list',from);assert.equal(r.status,200);assert.equal(ids(r.body.body).length,count,r.body.body);return r;};
  const thumbnails=async(r)=>{const wanted=ids(r.body.body);await wait(()=>wanted.every(id=>traffic.some(t=>t.url.endsWith('/get')&&new URLSearchParams(t.form).get('id')===String(id)&&t.body&&t.status===200)),'Missing PNG thumbnails');await frames();};
  try{
    await cdp.call('Network.enable');await cdp.call('Emulation.setFocusEmulationEnabled',{enabled:true});
    await cdp.call('Page.navigate',{url:origin+'/draw.html'});await ready();
    await capture('gallery-before');
    const empty=await list(()=>key('l'),0),guid=new URLSearchParams(empty.form).get('myGUID');assert(guid);
    check('fresh guest Mine gallery is empty',empty.body.body==='<Pictures offset="0"></Pictures>');
    await click(699,50);await click(132,122);await key('s');await capture('gallery-save-dialog');
    await click(350,176);await cdp.call('Input.insertText',{text:'Gallery Original'});
    await click(350,245);await click(350,326);
    await click(350,310);await cdp.call('Input.insertText',{text:'gallery,original'});
    let from=traffic.length;await click(520,455);const saved=await response('save',from);
    check('native drawing saves successfully',saved.status===200&&/^<Success><Id>\d+<\/Id><\/Success>$/.test(saved.body.body));
    const firstId=Number(saved.body.body.match(/<Id>(\d+)/)[1]);
    const png=Buffer.from(await (await fetch(origin+'/legacy-api/picture/get',{method:'POST',body:new URLSearchParams({id:String(firstId)}),signal})).arrayBuffer());
    assert.equal(png.subarray(0,8).toString('hex'),'89504e470d0a1a0a');
    // Respect the ordinary twenty-write/minute and 120-read/minute admission windows.
    await delay(61000,undefined,{signal});
    const save=async(name,type,tags,owner=guid,data=png)=>{
      const form=new FormData();for(const [k,v] of Object.entries({guid:owner,password:'',name,dataType:String(type),tags,overwrite:'on'}))form.set(k,v);
      form.set('data',new Blob([data],{type:'image/png'}),'Foo.png');const text=await post('picture/save',form);assert.match(text,/^<Success><Id>\d+<\/Id><\/Success>$/);return Number(text.match(/<Id>(\d+)/)[1]);
    };
    const owned=[firstId];for(let i=1;i<=18;i++)owned.push(await save('Gallery '+i,2,'gallery,common'));
    await delay(61000,undefined,{signal});
    for(let i=19;i<=23;i++)owned.push(await save('Gallery '+i,2,'gallery,common'));
    const chunk=(kind,data)=>{const body=Buffer.concat([Buffer.from(kind),data]),size=Buffer.alloc(4),crc=Buffer.alloc(4);size.writeUInt32BE(data.length);crc.writeUInt32BE(crc32(body));return Buffer.concat([size,body,crc]);};
    const rows=Buffer.alloc(8*33);for(let y=0;y<8;y++)for(let x=0;x<8;x++)if(x===y||x+y===7)Buffer.from([255,0,255,255]).copy(rows,y*33+1+x*4);
    const rarePng=Buffer.concat([png.subarray(0,33),chunk('IDAT',deflateSync(rows)),chunk('IEND',Buffer.alloc(0))]);
    const rare=await save('Gallery Rare',3,'gallery,rare',guid,rarePng);owned.push(rare);
    const foreign='gallery-other-933';await post('char/list',new URLSearchParams({guid:foreign,password:''}));const foreignId=await save('Foreign Art',2,'gallery,foreign',foreign);
    let page=await list(async()=>{await key('l');await click(640,85);},24);await thumbnails(page);check('Mine renders 24 of 25 owned artworks',ids(page.body.body).every(id=>owned.includes(id))&&!page.body.body.includes('<Admin'));
    await capture('gallery-page-one');
    const next=await list(()=>click(670,530),1);check('Next uses offset24 and retrieves final owned artwork',new URLSearchParams(next.form).get('offset')==='24'&&ids(next.body.body)[0]===firstId);await thumbnails(next);await capture('gallery-page-two');
    page=await list(()=>click(130,530),24);await thumbnails(page);check('Previous restores offset0',!new URLSearchParams(page.form).has('offset'));
    await click(155,85);await click(155,115);page=await list(()=>click(640,85),24);await thumbnails(page);check('All includes foreign artwork without foreign Mine marker',page.body.body.includes(`<Pic id="${foreignId}"><PicName>Foreign Art</PicName><DataType>2</DataType><Tags>gallery,foreign</Tags></Pic>`));await capture('gallery-all');
    await click(155,85);await click(155,145);const wild=await list(()=>click(640,85),0);check('Wild Shadow is explicitly empty, not substituted local art',new URLSearchParams(wild.form).get('guid')==='administrator@wildshadow.com');
    await click(155,85);await click(155,85);await click(285,85);await click(285,175);page=await list(()=>click(640,85),1);check('Environment type filter selects the prepared type3 artwork',ids(page.body.body)[0]===rare&&new URLSearchParams(page.form).get('dataType')==='3');
    await click(285,85);await click(285,85);await click(450,85);await cdp.call('Input.insertText',{text:'gallery,rare'});page=await list(()=>click(640,85),1);check('comma-separated tag intersection selects one artwork',ids(page.body.body)[0]===rare&&new URLSearchParams(page.form).get('tags')==='gallery,rare');await thumbnails(page);await capture('gallery-filtered');
    await click(150,120);await key('s');await capture('gallery-loaded');from=traffic.length;await click(520,455);
    const reSaved=await response('save',from);check('loading into the editor preserves name and type for overwrite',reSaved.body.body===`<Success><Id>${rare}</Id></Success>`);
    const loaded=Buffer.from(await (await fetch(origin+'/legacy-api/picture/get',{method:'POST',body:new URLSearchParams({id:String(rare)}),signal})).arrayBuffer());
    check('native load and save preserve the distinct prepared bitmap pixels',inflateSync(loaded.subarray(41,loaded.length-16)).equals(rows));
    await delay(61000,undefined,{signal});
    await cdp.call('Page.reload');await ready();page=await list(()=>key('l'),24);await thumbnails(page);check('page reload preserves Mine identity and saved artworks',new URLSearchParams(page.form).get('myGUID')===guid&&ids(page.body.body)[0]===rare);
    await click(185,115);await capture('gallery-delete-dialog');await click(320,339);
    const still=await post('picture/list',new URLSearchParams({myGUID:guid,guid,tags:'rare'}));check('Cancel retains the artwork',ids(still)[0]===rare);
    await click(185,115);from=traffic.length;await click(480,339);const deleted=await response('delete',from);check('confirmed native owner deletion commits',deleted.body.body==='<Success/>');await response('list',from);await capture('gallery-after-delete');
    check('confirmed deletion removes artwork from owner search',ids(await post('picture/list',new URLSearchParams({myGUID:guid,guid,tags:'rare'}))).length===0);
    const retained=await post('picture/list',new URLSearchParams({myGUID:foreign,guid:foreign}));check('owner deletion preserves foreign artwork',ids(retained).includes(foreignId));
    await writeFile(join(out,'gallery-receipt.json'),JSON.stringify({checks,owned,rare,foreignId,pngSha256:createHash('sha256').update(png).digest('hex'),traffic},null,2));
    await cdp.call('Page.navigate',{url:origin+'/'});await ready();await delay(1000,undefined,{signal});await click(400,550);await click(400,550);
  }finally{await writeFile(join(out,'gallery-traffic.json'),JSON.stringify({checks,traffic},null,2));cdp.close();}
}
export async function run(game){for(const name of checks)game.check(name,true);game.check('normal game entry remains usable after gallery journey',(await game.inspect()).view.lifecycle.alive);}
