import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {mkdir, open, readFile, writeFile, realpath, rename} from 'node:fs/promises';
import {resolve, dirname, join, isAbsolute} from 'node:path';
import {pathToFileURL, fileURLToPath} from 'node:url';
import {randomBytes, randomUUID, createHash} from 'node:crypto';
import {writeSync,readFileSync} from 'node:fs';
import {startChild,stopChild,identity,registry} from './native-test-process.mjs';
import {createServer} from 'node:net';
import {parseArgs} from 'node:util';
import {setTimeout as delay} from 'node:timers/promises';
import assert from 'node:assert/strict';
import {connect} from './record-demo.mjs';

const exec = promisify(execFile), root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
let activeCase=false;
export function browserDiagnostic(line, closing=false) {
  const m=/^\[\d+:\d+:\d+\/\d+\.\d+:ERROR:([^\]]+):\d+\] (.*)$/.exec(line);
  // Push registration is outside this local game's protocol; retain its diagnostics without inventing game failures.
  if(m&&['google_apis/gcm/engine/registration_request.cc','google_apis/gcm/engine/mcs_client.cc'].includes(m[1]))return 'background-push';
  if(m&&m[1]==='dbus/object_proxy.cc'&&m[2]==='Failed to call method: org.freedesktop.DBus.Properties.GetAll: object_path= /org/freedesktop/UPower/devices/DisplayDevice: org.freedesktop.DBus.Error.ServiceUnknown: The name org.freedesktop.UPower was not provided by any .service files')return 'unavailable-power-service';
  if(closing&&m&&m[1]==='gpu/command_buffer/service/shared_image/shared_image_manager.cc'&&m[2]==='SharedImageManager::ProduceSkia: Trying to Produce a Skia representation from a non-existent mailbox.')return 'renderer-shutdown';
  return /\bERROR\b|panicked/.test(line)?'fatal':null;
}
export {startChild,stopChild} from './native-test-process.mjs';
async function freePort() {
  const server=createServer();await new Promise((ok,bad)=>{server.once('error',bad);server.listen(0,'127.0.0.1',ok);});
  const port=server.address().port;await new Promise((ok,bad)=>server.close(error=>error?bad(error):ok()));return port;
}
export async function runCase(scenario, options) {
  assert(options.runDir && isAbsolute(options.runDir)&&resolve(options.runDir)===options.runDir,'A canonical absolute new --run-dir is required');
  assert(options.runDir!==root&&!options.runDir.startsWith(root+'/'),'Keep generated runs outside the source checkout');
  assert(options.browserTools && isAbsolute(options.browserTools),'Set PI_BROWSER_TOOLS to the installed managed-browser directory');
  assert(options.chrome && isAbsolute(options.chrome),'Set PI_NATIVE_TEST_CHROME to the Chrome executable');
  assert(Array.isArray(options.chromeArgs) && options.chromeArgs.every(x=>typeof x==='string'),'Chrome arguments must be an explicit JSON array');
  if(options.client!==undefined)assert(isAbsolute(options.client),'--client must be an absolute checkout path');
  const timeout=options.timeoutMs;assert(Number.isInteger(timeout)&&timeout>=180000&&timeout<=3600000,'Timeout must be 180000–3600000ms, including 120 seconds for cleanup');
  assert(!activeCase,'Only one native case may own this process');
  const out=options.runDir;assert.equal(await realpath(dirname(out)),dirname(out),'Run parent must be canonical, not a symlink');await mkdir(out,{mode:0o700});activeCase=true;
  const previousOwner=process.env.PI_BROWSER_OWNER;
  const controller=new AbortController(), children=[], files=[], report={status:'running',scenario,prepared:true,phases:{},assertions:[],captures:[],children:[]};
  const started=performance.now();let phase='setup',connection,owner,port,chromePort,browser,actor,account,primary,closing=false,guardian,registryState;
  const elapsed=()=>Math.round(performance.now()-started);
  const save=async()=>{await writeFile(join(out,'result.tmp'),JSON.stringify({...report,wallMs:elapsed()},null,2)+'\n',{mode:0o600});await rename(join(out,'result.tmp'),join(out,'result.json'));};
  const register=()=>{registryState.children=children.filter(r=>r.identity).map(r=>({...r.identity,name:r.name}));registry(join(out,'children.json'),registryState);};
  const fail=error=>{if(closing)report.cleanupErrors.push(String(error));else if(!controller.signal.aborted)controller.abort(error);};
  const interrupted=()=>fail(Error('Native test interrupted'));
  process.once('SIGINT',interrupted);process.once('SIGTERM',interrupted);
  const timer=setTimeout(()=>fail(Error('Native test active deadline reached; cleanup reserve retained')),timeout-120000);
  let rejectStop;const stopped=new Promise((_,bad)=>{rejectStop=bad;});stopped.catch(()=>{});
  controller.signal.addEventListener('abort',()=>rejectStop(controller.signal.reason),{once:true});
  const check=()=>controller.signal.throwIfAborted();
  const bounded=async(name,work)=>{check();phase=name;report.phase=name;await save();check();const t=performance.now();try{return await Promise.race([work(),stopped]);}finally{report.phases[name]=(report.phases[name]??0)+Math.round(performance.now()-t);}};
  const log=async name=>{const f=await open(join(out,name),'a',0o600);files.push(f);return f.fd;};
  const launch=(name,command,args,stdio,env=process.env)=>{
    check();const row=startChild(command,args,{cwd:root,stdio,env},controller.signal);row.name=name;children.push(row);register();
    row.started.then(()=>{if(!row.identity){row.identity=identity(row.child.pid);register();}},fail);
    row.closed.then(value=>{row.done=true;if(!row.stopping)fail(Error(name+' exited before cleanup: '+JSON.stringify(value)));});return row;
  };
  const waitFor=async(work,description,limit=30000)=>{
    const end=performance.now()+limit;
    while(true){check();const value=await work();if(value)return value;assert(performance.now()<end,description);await delay(30,undefined,{signal:controller.signal});}
  };
  const api=async command=>{
    check();const token=(await readFile(join(out,'state/control.token'),'utf8')).trim();
    const response=await fetch(`http://127.0.0.1:${port}/dev/testing`,{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:'case-'+randomUUID(),command}),signal:AbortSignal.any([controller.signal,AbortSignal.timeout(10000)])});
    const value=await response.json();assert(response.ok&&value.status!=='failed','Test API failed: '+JSON.stringify(value));return value;
  };
  const call=(method,params={})=>{check();return connection.call(method,params);};
  const evaluate=expression=>{check();return connection.eval(expression);};
  async function driver(command) {
    check();const id='case-'+randomUUID(),request=join(out,id+'.request.json');await writeFile(request,JSON.stringify({id,command}),{mode:0o600});check();
    const row=launch('controller',process.execPath,[join(root,'scripts/game.mjs'),join(out,'native.json'),request],['ignore','pipe','pipe']);
    const fd=await log('controller.stderr');let text='',errors='';row.child.stdout.on('data',b=>text+=b);
    row.child.stderr.on('data',b=>{writeSync(fd,b);errors+=b;let n;while((n=errors.indexOf('\n'))>=0){const line=errors.slice(0,n);errors=errors.slice(n+1);if(line.startsWith('{'))try{const value=JSON.parse(line);if(value.id===id&&value.status==='failed')fail(Error('Controller failed: '+value.error));}catch(error){fail(error);}}});
    row.stopping=true;const deadline=setTimeout(()=>fail(Error('Controller command exceeded 90 seconds')),90000);let status;
    try{await row.started;status=await Promise.race([row.closed,stopped]);}finally{clearTimeout(deadline);}
    await writeFile(join(out,id+'.result.json'),text,{mode:0o600});assert.equal(status.code,0,'Controller failed; see controller.stderr');check();return JSON.parse(text);
  }
  async function capture(name) {
    assert(/^[A-Za-z0-9_-]+$/.test(name),'Capture name must be a filename');
    const image=await call('Page.captureScreenshot',{format:'png'});const path=join(out,name+'.png');await writeFile(path,Buffer.from(image.data,'base64'),{mode:0o600});report.captures.push(path);return path;
  }
  try {
    await save();
    registryState={parent:identity(process.pid),boot:readFileSync('/proc/sys/kernel/random/boot_id','utf8'),deadline:Date.now()+timeout,children:[],done:false};register();
    guardian=startChild(process.execPath,[join(root,'scripts/native-test-process.mjs'),out],{stdio:['ignore','ignore',await log('guardian.log')]},controller.signal);await guardian.started;
    guardian.closed.then(value=>{if(!closing)fail(Error('Cleanup guardian exited early: '+JSON.stringify(value)));});
    report.guardian=guardian.child.pid;
    report.inputs={};for(const [name,path] of [['case',resolve(scenario)],['runner',fileURLToPath(import.meta.url)],['controller',join(root,'scripts/game.mjs')]])report.inputs[name]=createHash('sha256').update(await readFile(path)).digest('hex');
    const fixtureModule=await import(pathToFileURL(resolve(scenario)).href);assert(typeof fixtureModule.run==='function'&&fixtureModule.fixture,'Case must export fixture and run(game)');
    const spec=fixtureModule.fixture;assert(Number.isInteger(spec.world),'Fixture needs an explicit world ID');
    port=await freePort();chromePort=await freePort();assert.notEqual(port,chromePort,'Ports collided; no launch attempted');
    owner='native-test:'+randomUUID();process.env.PI_BROWSER_OWNER=owner;
    report.source={head:(await exec('git',['rev-parse','HEAD'],{cwd:root})).stdout.trim(),dirty:Boolean((await exec('git',['status','--porcelain'],{cwd:root})).stdout.trim())};report.origin=`http://127.0.0.1:${port}`;
    await bounded('prepareBuildStart',async()=>{
      const began=performance.now();let buildStart,buildEnd,ready;
      const args=[String(port),'--test-state',join(out,'state'),'--no-shore-bots','--release'];if(options.client)args.push('--client',options.client);
      const server=launch('server',join(root,'run.sh'),args,['ignore','pipe','pipe'],{...process.env,CARGO_TERM_COLOR:'never'});
      for(const [stream,name] of [[server.child.stdout,'server.log'],[server.child.stderr,'build.log']]){
        const fd=await log(name);let pending='';
        stream.on('data',bytes=>{
          try{
            writeSync(fd,bytes);pending+=bytes;
            if(pending.includes('Building and starting your local game.'))buildStart??=performance.now();
            if(pending.includes('Finished `release` profile'))buildEnd??=performance.now();
            let newline;while((newline=pending.indexOf('\n'))>=0){const line=pending.slice(0,newline);pending=pending.slice(newline+1);if(line.startsWith('{')){const event=JSON.parse(line);if(event.level==='ERROR')fail(Error('Backend ERROR: '+line));if(event.fields?.message==='game frontdoor listening')ready=performance.now();}}
          }catch(error){fail(error);}
        });
        stream.on('end',()=>{if(pending.includes('"level":"ERROR"'))fail(Error('Backend ERROR at EOF: '+pending));});
      }
      await server.started;await waitFor(()=>ready,'Server did not become ready',timeout-125000);
      assert(buildStart&&buildEnd&&buildStart<=buildEnd&&buildEnd<=ready,'Missing or inconsistent launcher/Cargo timing boundaries');
      report.phases.clientPreparation=Math.round(buildStart-began);report.phases.build=Math.round(buildEnd-buildStart);report.phases.serverStart=Math.round(ready-buildEnd);
      report.client=options.client?{mode:'local',path:options.client,lock:JSON.parse(await readFile(join(options.client,'ruffle/lock.json'),'utf8'))}:{mode:'deployed',selection:JSON.parse(await readFile(join(out,'state/client-selection.json'),'utf8'))};
    });
    delete report.phases.prepareBuildStart;
    await bounded('browserAndFixture',async()=>{
      const auth=join(out,'Xauthority');await writeFile(auth,'',{mode:0o600});const cookie=randomBytes(16).toString('hex');
      await exec('xauth',['-f',auth,'add',':99','MIT-MAGIC-COOKIE-1',cookie]);
      const xvfb=launch('Xvfb','Xvfb',['-displayfd','3','-screen','0','1400x1000x24','-nolisten','tcp','-auth',auth],['ignore','ignore',await log('display.log'),'pipe']);
      let display='';xvfb.child.stdio[3].on('data',b=>display+=b);await waitFor(()=>display.includes('\n'),'Private display did not start',10000);display=':'+display.trim();
      await exec('xauth',['-f',auth,'add',display,'MIT-MAGIC-COOKIE-1',cookie]);
      const env={...process.env,DISPLAY:display,XAUTHORITY:auth,PI_BROWSER_OWNER:owner,PI_BROWSER_TAB_POLICY:'agent_only',CHROME_REAL_BINARY:options.chrome};delete env.WAYLAND_DISPLAY;
      const box=launch('openbox','openbox',['--sm-disable'],['ignore',await log('window-manager.log'),await log('window-manager.log')],env);await box.started;
      const chrome=launch('Chrome',process.execPath,[join(options.browserTools,'launch-browser.mjs'),`--remote-debugging-port=${chromePort}`,`--user-data-dir=${join(out,'profile')}`,'--ozone-platform=x11','--window-size=1280,960','--no-first-run','--no-default-browser-check',...options.chromeArgs,'about:blank'],['ignore','pipe','pipe'],env);
      const browserLog=await log('browser.log');let browserLines='';
      const inspectLine=line=>{const kind=browserDiagnostic(line,closing);if(kind==='fatal')fail(Error('Browser ERROR: '+line));else if(kind)(report.browserDiagnostics??=[]).push({kind,line});};
      chrome.child.stderr.on('data',bytes=>{try{writeSync(browserLog,bytes);browserLines+=bytes;let n;while((n=browserLines.indexOf('\n'))>=0){inspectLine(browserLines.slice(0,n));browserLines=browserLines.slice(n+1);}}catch(error){fail(error);}});
      chrome.child.stderr.on('end',()=>{if(browserLines)inspectLine(browserLines);});
      let receipt='';chrome.child.stdout.on('data',b=>receipt+=b);await waitFor(()=>receipt.includes('\n'),'Managed browser did not return its receipt',30000);browser=JSON.parse(receipt.split('\n')[0]);assert(browser.target&&browser.owner===owner&&browser.pid===chrome.child.pid,'Invalid managed browser receipt');
      report.browser={owner,port:chromePort,target:browser.target,pid:browser.pid};
      let resolveGuest,rejectGuest;const guest=new Promise((ok,bad)=>{resolveGuest=ok;rejectGuest=bad;});guest.catch(()=>{});let requests=0,guestDone=false;
      connection=await connect({port:chromePort,target:browser.target,url:'about:blank',helper:join(options.browserTools,'direct-cdp.mjs'),onEvent:event=>{
        if(event.method==='Runtime.exceptionThrown'){fail(Error('Browser exception: '+JSON.stringify(event.params.exceptionDetails)));return;}
        if(event.method==='Runtime.consoleAPICalled'&&event.params.type==='error'){fail(Error('Browser console error: '+JSON.stringify(event.params.args)));return;}
        if(event.method!=='Fetch.requestPaused')return;
        (async()=>{
          assert.equal(++requests,1,'Only one normal fresh guest is permitted');const e=event.params;
          assert.equal(e.request.url,report.origin+'/legacy-api/char/list');assert.equal(e.request.method,'POST');
          const form=new URLSearchParams(e.request.postData),guid=form.get('guid');assert(guid&&/^[A-Za-z0-9_.-]{1,128}$/.test(guid)&&!form.get('password'),'Expected a UI-generated disposable guest');
          const world=await api({op:'world_info',world:spec.world});const point=spec.position==='spawn'?world.spawn_points[0]:null;
          if(spec.position==='spawn')assert(Array.isArray(point)&&point.length===2&&point.every(Number.isFinite),'Invalid observed spawn tuple');
          const position=spec.position==='spawn'?{x:point[0],y:point[1]}:spec.position;
          const result=await api({op:'spawn',spec:{...spec,guid,position,controller:'native',profile:null}});actor=result.result.actor;account=result.result.account;
          await writeFile(join(out,'fixture.json'),JSON.stringify({actor,account,spec,prepared:true}),{mode:0o600});
          await call('Fetch.continueRequest',{requestId:e.requestId});guestDone=true;resolveGuest();
        })().catch(error=>{rejectGuest(error);fail(error);});
      }});
      await call('Runtime.enable');
      await call('Fetch.enable',{patterns:[{urlPattern:report.origin+'/legacy-api/char/list',requestStage:'Request'}]});
      await call('Page.navigate',{url:report.origin});
      await Promise.race([guest,waitFor(()=>guestDone,'Normal fresh guest char/list missing',60000),stopped]);await call('Fetch.disable');
    });
    await bounded('clientUI',async()=>{
      await waitFor(()=>evaluate('document.querySelector("ruffle-player")?.ruffle().isPlaying===true'),'Ruffle did not start');
      await evaluate('(()=>{const p=document.querySelector("ruffle-player");const modal=p.shadowRoot.querySelector("#hardware-acceleration-modal");if(modal&&!modal.classList.contains("hidden"))modal.querySelector(".close-modal").click();})()');
      const click=async()=>{
        const r=await evaluate('(()=>{const r=document.querySelector("ruffle-player").getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height,vw:innerWidth,vh:innerHeight};})()');assert(r.width===800&&r.height===600&&r.x>=0&&r.y>=0&&r.x+800<=r.vw&&r.y+600<=r.vh,'Native menu requires the unclipped 800x600 client');
        const p={x:r.x+400,y:r.y+550,button:'left',clickCount:1};
        try{await call('Input.dispatchMouseEvent',{type:'mousePressed',buttons:1,...p});}finally{await connection.call('Input.dispatchMouseEvent',{type:'mouseReleased',buttons:0,...p});}
        await evaluate('new Promise(ok=>requestAnimationFrame(()=>requestAnimationFrame(ok)))');
      };
      await call('Emulation.setFocusEmulationEnabled',{enabled:true});await click();await click();
      await waitFor(async()=>{const s=await api({op:'native_state',actors:[actor]});assert(!s.stopped,'Native session stopped');assert(!s.actors.some(a=>['dead','dying','lost'].includes(a.stage)),'Native participant terminal');return s.actors.length===1&&s.actors[0].stage==='ready';},'Normal character LOAD did not complete');
      await writeFile(join(out,'native.json'),JSON.stringify({origin:report.origin,state:join(out,'state'),browserTools:join(options.browserTools,'direct-cdp.mjs'),native:[{port:chromePort,target:browser.target,actor}]}),{mode:0o600});
      await driver({op:'pause'});report.actor=actor;report.account=account;
    });
    await bounded('scenario',async()=>{
      const game={actor,observe:()=>api({op:'observe',actor}),check(name,condition){assert(condition,name);report.assertions.push(name);},
        async act(input,captureSpec){
          check();let complete=false;const action=driver({op:'act',actor,input,pauseAfter:true}).finally(()=>complete=true);
          if(!captureSpec)return await action;
          const shot=(async()=>{
            await waitFor(async()=>{assert(!complete,'Action ended before screenshot condition');const view=await api({op:'observe',actor});return !view.paused&&captureSpec.when(view);},'Screenshot condition not observed',input.duration_ms+10000);
            assert(await evaluate('document.querySelector("ruffle-player").ruffle().isPlaying'),'Client paused before capture');await capture(captureSpec.name);
          })();
          const results=await Promise.allSettled([action,shot]);for(const r of results)if(r.status==='rejected')throw r.reason;return results[0].value;
        }};
      report.before=await game.observe();await fixtureModule.run(game);report.after=await game.observe();report.activeGameMs=report.after.game_ms-report.before.game_ms;
    });
    check();report.status='passed';
  } catch(error) {
    primary=error;report.status='failed';report.failure={phase,error:String(error),stack:error.stack};
    await save();
    if(connection)try{const image=await connection.call('Page.captureScreenshot',{format:'png'});await writeFile(join(out,'failure.png'),Buffer.from(image.data,'base64'),{mode:0o600});report.failure.capture=join(out,'failure.png');}catch(error){report.failure.captureError=String(error);}
  } finally {
    clearTimeout(timer);closing=true;report.cleanupErrors=[];controller.abort(Error('Native test closing'));const cleanup=performance.now();
    for(const row of children)row.stopping=true;
    let managed,tabs=[];
    if(browser)try{
      managed=await import(pathToFileURL(join(options.browserTools,'tabs.mjs')).href);
      for(const page of (await managed.liveTargets(chromePort)).targets.filter(p=>p.type==='page')){
        const tab={...browser,target:page.targetId};await managed.ensureTarget(tab);managed.assertOwner(tab,owner);tabs.push(tab);
      }
    }catch(error){report.cleanupErrors.push(String(error));}
    if(connection)try{connection.close();}catch(error){report.cleanupErrors.push(String(error));}
    // One graceful browser shutdown drains renderer work before retiring its owned targets.
    for(const row of children.reverse()){
      row.stopping=true;
      try{report.children.push({name:row.name,...await stopChild(row)});if(row.name==='Chrome')for(const tab of tabs)managed.released(tab);}catch(error){report.cleanupErrors.push(String(error));}
    }
    if(registryState){registryState.done=true;register();}
    if(guardian)try{const end=setTimeout(()=>guardian.child.kill('SIGTERM'),5000);try{const value=await guardian.closed;report.children.push({name:'guardian',...value});assert.equal(value.code,0,'Cleanup guardian failed');}finally{clearTimeout(end);}}catch(error){report.cleanupErrors.push(String(error));}
    for(const f of files)try{await f.close();}catch(error){report.cleanupErrors.push(String(error));}
    report.phases.cleanup=Math.round(performance.now()-cleanup);if(report.cleanupErrors.length)report.status='failed';
    try{await save();}finally{process.removeListener('SIGINT',interrupted);process.removeListener('SIGTERM',interrupted);activeCase=false;if(previousOwner===undefined)delete process.env.PI_BROWSER_OWNER;else process.env.PI_BROWSER_OWNER=previousOwner;}
  }
  if(primary)throw Error('Native test failed during '+report.failure.phase+': '+String(primary)+'; '+join(out,'result.json'));
  if(report.cleanupErrors.length)throw Error('Native test cleanup failed: '+report.cleanupErrors.join('; ')+'; '+join(out,'result.json'));
  return {...report,wallMs:elapsed(),report:join(out,'result.json')};
}
if(process.argv[1]&&resolve(process.argv[1])===fileURLToPath(import.meta.url)){
  if(process.argv.length===3&&process.argv[2]==='--help'){
    console.log('Usage: node scripts/native-test.mjs CASE.mjs --run-dir NEW_ABSOLUTE_DIRECTORY\nLinux: Node22.15+, Xvfb, xauth, Openbox and managed browser helpers.\nSet PI_BROWSER_TOOLS and PI_NATIVE_TEST_CHROME, or --browser-tools and --chrome.\nOptional --chrome-args JSON, --client ABSOLUTE_CHECKOUT, --timeout-ms (default900000; includes120000 cleanup reserve).\nFresh prepared fixtures only. Existing directories are refused; state/logs are preserved.');
  }else try {
    const {values,positionals}=parseArgs({allowPositionals:true,options:{'run-dir':{type:'string'},'browser-tools':{type:'string'},chrome:{type:'string'},'chrome-args':{type:'string'},client:{type:'string'},'timeout-ms':{type:'string',default:'900000'}}});
    assert.equal(positionals.length,1,'Usage: native-test.mjs CASE.mjs --run-dir NEW_ABSOLUTE_DIRECTORY');
    const result=await runCase(positionals[0],{runDir:values['run-dir'],browserTools:values['browser-tools']??process.env.PI_BROWSER_TOOLS,chrome:values.chrome??process.env.PI_NATIVE_TEST_CHROME,chromeArgs:JSON.parse(values['chrome-args']??process.env.PI_NATIVE_TEST_CHROME_ARGS??'[]'),client:values.client,timeoutMs:Number(values['timeout-ms'])});
    console.log(JSON.stringify({status:result.status,assertions:result.assertions,phases:result.phases,wallMs:result.wallMs,browserDiagnostics:result.browserDiagnostics,report:result.report}));
  }catch(error){console.error(String(error));process.exitCode=1;}
}
