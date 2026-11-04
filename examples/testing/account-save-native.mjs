import assert from 'node:assert/strict';
import {spawn,execFileSync} from 'node:child_process';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomBytes} from 'node:crypto';
import {createServer} from 'node:net';
import {setTimeout as delay} from 'node:timers/promises';
import {connect,socket} from '../../scripts/record-demo.mjs';

const out=process.env.QA_RESULTS_DIR, origin=process.env.ACCOUNT_SAVE_ORIGIN, phase=process.env.ACCOUNT_SAVE_PHASE;
assert(out&&origin&&phase&&process.env.PI_BROWSER_TOOLS&&process.env.PI_NATIVE_TEST_CHROME,'Explicit owned QA/browser inputs required');
const chunks=[];for await(const chunk of process.stdin)chunks.push(chunk);const password=Buffer.concat(chunks).toString('utf8').trimEnd();
const auth=join(out,`${phase}.Xauthority`),cookie=randomBytes(16).toString('hex');await writeFile(auth,'',{mode:0o600});
execFileSync('xauth',['-f',auth,'add',':99','MIT-MAGIC-COOKIE-1',cookie]);
const display=spawn('Xvfb',['-displayfd','3','-screen','0','1400x1000x24','-nolisten','tcp','-auth',auth],{stdio:['ignore','ignore','pipe','pipe']});
let number='',chrome,cdp,receipt,browserVersion,graphics,errors='',frames=0,verified=0,consoleLog=[],verifyRequests=new Set(),charLists=0,charRequests=new Set(),blockedOrigins=[],protocolErrors=[],corsRequests=[];display.stdio[3].on('data',bytes=>number+=bytes);
try {
  const deadline=Date.now()+10000;while(!number.includes('\n')&&Date.now()<deadline)await delay(50);assert(number.includes('\n'),'private display failed');
  const env={...process.env,DISPLAY:':'+number.trim(),XAUTHORITY:auth};delete env.WAYLAND_DISPLAY;
  execFileSync('xauth',['-f',auth,'add',env.DISPLAY,'MIT-MAGIC-COOKIE-1',cookie]);
  const probe=createServer();await new Promise(ok=>probe.listen(0,'127.0.0.1',ok));const port=probe.address().port;await new Promise(ok=>probe.close(ok));
  const owner=`account-save:${out}:${phase}`;env.PI_BROWSER_OWNER=owner;env.PI_BROWSER_TAB_POLICY='agent_only';env.CHROME_REAL_BINARY=process.env.PI_NATIVE_TEST_CHROME;env.CHROME_MCP_CPUQUOTA='1600%';
  chrome=spawn(process.execPath,[join(process.env.PI_BROWSER_TOOLS,'launch-browser.mjs'),`--remote-debugging-port=${port}`,`--user-data-dir=${join(out,phase+'-profile')}`,'--ozone-platform=x11','--window-size=1280,960','--no-first-run','--no-default-browser-check',...JSON.parse(process.env.PI_NATIVE_TEST_CHROME_ARGS??'[]'),'about:blank'],{env,stdio:['ignore','pipe','pipe']});
  let output='';chrome.stdout.on('data',bytes=>output+=bytes);chrome.stderr.on('data',bytes=>errors+=bytes);
  const launched=Date.now()+30000;while(!output.includes('\n')&&Date.now()<launched)await delay(100);assert(output.includes('\n'),errors);
  receipt=JSON.parse(output.split('\n')[0]);assert(receipt.owner===owner&&receipt.pid===chrome.pid&&receipt.target);
  process.env.PI_BROWSER_OWNER=owner;
  cdp=await connect({port,target:receipt.target,url:'about:blank',helper:join(process.env.PI_BROWSER_TOOLS,'direct-cdp.mjs'),onEvent:event=>{
    if(event.method==='Fetch.requestPaused') {
      const request=event.params;
      const requestOrigin=new URL(request.request.url).origin;
      const local=requestOrigin===origin||requestOrigin===origin.replace(/^http/,'ws')||requestOrigin===process.env.ACCOUNT_SAVE_CORS_API;
      if(!local)blockedOrigins.push(new URL(request.request.url).origin);
      cdp.call(local?'Fetch.continueRequest':'Fetch.failRequest',{requestId:request.requestId,...(local?{}:{errorReason:'BlockedByClient'})}).catch(error=>protocolErrors.push(String(error)));
    }
    if(event.method==='Network.requestWillBeSent'&&process.env.ACCOUNT_SAVE_CORS_API&&event.params.request.url===process.env.ACCOUNT_SAVE_CORS_API+'/api/account/export')corsRequests.push({method:event.params.request.method,url:event.params.request.url});
    if(event.method==='Log.entryAdded')consoleLog.push(event.params.entry.text);
    if(event.method==='Network.webSocketFrameError')consoleLog.push(event.params.errorMessage);
    if(event.method==='Network.webSocketFrameReceived')frames++;
    if(event.method==='Network.responseReceived'&&new URL(event.params.response.url).pathname==='/legacy-api/account/verify')verifyRequests.add(event.params.requestId);
    if(event.method==='Network.loadingFinished'&&verifyRequests.has(event.params.requestId))verified++;
    if(event.method==='Network.responseReceived'&&new URL(event.params.response.url).pathname==='/legacy-api/char/list')charRequests.add(event.params.requestId);
    if(event.method==='Network.loadingFinished'&&charRequests.has(event.params.requestId))charLists++;
    if(event.method==='Runtime.consoleAPICalled')consoleLog.push(event.params.args.map(a=>a.value??a.description).join(' '));
  }});
  browserVersion=await cdp.call('Browser.getVersion');
  await cdp.call('Runtime.enable');await cdp.call('Log.enable');await cdp.call('Network.enable');await cdp.call('Page.enable');await cdp.call('Fetch.enable',{patterns:[{urlPattern:'*',requestStage:'Request'}]});await cdp.call('Page.navigate',{url:origin});
  if(process.env.ACCOUNT_SAVE_CORS_API) {
    const ready=Date.now()+10000;while(!(await cdp.eval(`location.origin===${JSON.stringify(origin)}&&document.readyState==='complete'`))&&Date.now()<ready)await delay(50);
    const result=await cdp.eval(`(async()=>{const response=await fetch(${JSON.stringify(process.env.ACCOUNT_SAVE_CORS_API+'/api/account/export')},{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({email:'backup-true@example.invalid',password:${JSON.stringify(password)}}),credentials:'omit',cache:'no-store',redirect:'error'});if(response.status!==200)throw Error('CORS export status '+response.status);return{status:response.status,contentType:response.headers.get('content-type'),cacheControl:response.headers.get('cache-control'),disposition:response.headers.get('content-disposition'),text:await response.text()};})()`);
    assert.equal(result.contentType,'application/json');assert.equal(result.cacheControl,'no-store');
    const save=JSON.parse(result.text);assert.equal(result.disposition,`attachment; filename="openrotmg-account-${save.exported_at}.json"`);
    const bytes=Buffer.from(result.text);assert(bytes.length<=32*1024*1024);await writeFile(join(out,'cors-player.json'),bytes);delete result.text;
    assert(corsRequests.some(r=>r.method==='POST'));assert.deepEqual(blockedOrigins,[]);assert.deepEqual(protocolErrors,[]);
    await writeFile(join(out,'cors-result.json'),JSON.stringify({receipt,browserVersion,extraChromeArguments:JSON.parse(process.env.PI_NATIVE_TEST_CHROME_ARGS??'[]'),corsRequests,...result,downloadBytes:bytes.length,qualification:'Real cross-origin API fetch/attachment; not actual website UI or launcher integration'},null,2));
  } else {
  const ready=Date.now()+60000;while(!(await cdp.eval('document.querySelector("ruffle-player")?.ruffle().isPlaying===true'))&&Date.now()<ready)await delay(100);assert(await cdp.eval('document.querySelector("ruffle-player")?.ruffle().isPlaying===true'));
  await delay(3000);await cdp.eval('(()=>{const m=document.querySelector("ruffle-player").shadowRoot.querySelector("#hardware-acceleration-modal");if(m&&!m.classList.contains("hidden"))m.querySelector(".close-modal").click();})()');
  const rect=await cdp.eval('(()=>{const r=document.querySelector("ruffle-player").getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height};})()');assert(rect.width===800&&rect.height===600);
  const mouse=(type,x,y,extra={})=>cdp.call('Input.dispatchMouseEvent',{type,x:rect.x+x,y:rect.y+y,...extra});
  const capture=async name=>{const shot=await cdp.call('Page.captureScreenshot',{format:'png'});await writeFile(join(out,`${phase}-${name}.png`),Buffer.from(shot.data,'base64'));};
  const click=async(x,y)=>{await mouse('mousePressed',x,y,{button:'left',buttons:1,clickCount:1});await mouse('mouseReleased',x,y,{button:'left',buttons:0,clickCount:1});await delay(300);};
  await capture('title');await click(500,550);await capture('account-menu');await click(400,438);await capture('login-form');await click(400,228);await cdp.call('Input.insertText',{text:process.env.ACCOUNT_SAVE_LOGIN});await click(400,315);await cdp.call('Input.insertText',{text:password});await click(492,420);const signedIn=Date.now()+30000;while(verified<2&&Date.now()<signedIn)await delay(100);assert(verified>=2,'normal sign-in and Current account verification did not finish');await delay(500);await cdp.eval('new Promise(ok=>requestAnimationFrame(()=>requestAnimationFrame(ok)))');await capture('verified');await click(480,386);const refreshed=Date.now()+30000;while(charLists<2&&Date.now()<refreshed)await delay(100);assert(charLists>=2,'native post-login character list did not finish');await delay(500);await cdp.eval('new Promise(ok=>requestAnimationFrame(()=>requestAnimationFrame(ok)))');await capture('logged-in-title');await click(400,550);await delay(500);await cdp.eval('new Promise(ok=>requestAnimationFrame(()=>requestAnimationFrame(ok)))');await capture('characters');await click(160,130);
  const playing=Date.now()+30000;while(frames<10&&Date.now()<playing)await delay(100);assert(frames>=10,'imported character never entered native gameplay');await delay(1000);
  const account=async()=>{const response=await fetch(origin+'/legacy-api/char/list',{method:'POST',body:new URLSearchParams({guid:process.env.ACCOUNT_SAVE_LOGIN,password})});return await response.text();};
  const equipment=xml=>{const ch=/<Char id="1">([\s\S]*?)<\/Char>/.exec(xml);assert(ch,'imported living character missing');return /<Equipment>([^<]+)<\/Equipment>/.exec(ch[1])[1].split(',').map(Number);};
  const before=equipment(await account());
  if(phase==='before') {
    // A normal inventory drop proves the imported character, not merely a dispatched menu click.
    assert(before[0]===2711,'synthetic starter weapon missing');await mouse('mouseMoved',640,385);await mouse('mousePressed',640,385,{button:'left',buttons:1,clickCount:1});for(let i=1;i<=12;i++)await mouse('mouseMoved',640+(430-640)*i/12,385+(380-385)*i/12,{button:'left',buttons:1});await mouse('mouseReleased',430,380,{button:'left',buttons:0,clickCount:1});
    const committed=Date.now()+10000;let after;do{after=equipment(await account());if(after[0]===-1)break;await delay(100);}while(Date.now()<committed);const expected=[...before];expected[0]=-1;assert.deepEqual(after,expected);await writeFile(join(out,'native-expected.json'),JSON.stringify(expected));
  } else assert.deepEqual(before,JSON.parse(await readFile(join(out,'native-expected.json'),'utf8')));
  graphics=await cdp.eval(`(()=>{const canvas=document.querySelector('ruffle-player').shadowRoot.querySelector('canvas');const gl=canvas.getContext('webgl2');if(!gl)throw Error('Native renderer has no WebGL2 context');const info=gl.getExtension('WEBGL_debug_renderer_info');return{webgl2:true,lost:gl.isContextLost(),renderer:gl.getParameter(gl.RENDERER),vendor:gl.getParameter(gl.VENDOR),unmaskedRenderer:info?gl.getParameter(info.UNMASKED_RENDERER_WEBGL):null};})()`);assert.equal(graphics.lost,false,'native WebGL2 context lost');
  assert(consoleLog.some(s=>/Creating wgpu webgl renderer|Created renderer.*WebGL|Renderer:.*WebGL/i.test(s)),'native renderer did not report WebGL');
  assert.deepEqual(protocolErrors,[],'owned browser request control failed');
  const shot=await cdp.call('Page.captureScreenshot',{format:'png'});await writeFile(join(out,`${phase}-imported-native.png`),Buffer.from(shot.data,'base64'));
  await writeFile(join(out,`${phase}-native-result.json`),JSON.stringify({receipt,browserVersion,chromeBinary:process.env.PI_NATIVE_TEST_CHROME,extraChromeArguments:JSON.parse(process.env.PI_NATIVE_TEST_CHROME_ARGS??'[]'),graphics,qualification:'QA-enabled software WebGL; not default GPU, OS offline, default-browser-route or all-platform qualification',frames,inventory:equipment(await account()),blockedOrigins,consoleLog},null,2));
  }
} catch(error) {
  if(cdp) {
    const shot=await cdp.call('Page.captureScreenshot',{format:'png'});await writeFile(join(out,`${phase}-failure.png`),Buffer.from(shot.data,'base64'));
    await writeFile(join(out,`${phase}-failure.json`),JSON.stringify({frames,blockedOrigins,protocolErrors,consoleLog,error:String(error)},null,2));
  }
  throw error;
} finally {
  const failures=[];
  cdp?.close();
  try {
    if(chrome&&receipt) {
      const argument=chrome.spawnargs.find(s=>s.startsWith('--remote-debugging-port='));const port=Number(argument.split('=')[1]);
      const info=await(await fetch(`http://127.0.0.1:${port}/json/version`)).json();const protocol=await socket(info.webSocketDebuggerUrl);await protocol.call('Browser.close').catch(error=>{if(!/closed/.test(String(error)))throw error;});protocol.close();
      const deadline=Date.now()+10000;while(chrome.exitCode===null&&Date.now()<deadline)await delay(100);assert(chrome.exitCode===0,'owned browser failed to close');
    } else if(chrome&&chrome.exitCode===null) {chrome.kill('SIGTERM');await new Promise(ok=>chrome.once('close',ok));}
  } catch(error) {failures.push(error);}
  try {display.kill('SIGTERM');await new Promise(ok=>display.once('close',ok));}catch(error){failures.push(error);}
  await writeFile(join(out,`${phase}-browser.log`),errors);
  if(failures.length)throw new AggregateError(failures,'Native client cleanup failed');
}
