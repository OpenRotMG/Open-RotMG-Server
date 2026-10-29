import assert from 'node:assert/strict';
import {execFile,spawn} from 'node:child_process';
import {promisify} from 'node:util';
import {open,readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {createServer} from 'node:net';
import {createHash} from 'node:crypto';
import {setTimeout as delay} from 'node:timers/promises';

const [native,client]=process.argv.slice(2),out=process.env.QA_RESULTS_DIR;
assert(native&&client&&out,'Use qa.py exec with the closed native run and its exact client directory');
const receipt=JSON.parse(await readFile(join(native,'result.json'),'utf8'));
assert(receipt.status==='passed'&&receipt.cleanupErrors.length===0,'Native lifecycle must have passed and closed');
const saved=JSON.parse(await readFile(join(native,'../gallery-receipt.json'),'utf8')),{state}=JSON.parse(await readFile(join(native,'native.json'),'utf8'));
const accounts=JSON.parse((await promisify(execFile)('python3',['-c','import sqlite3,json,sys; print(json.dumps(sqlite3.connect(sys.argv[1]).execute("SELECT guid FROM accounts ORDER BY id").fetchall()))',join(state,'game.db')])).stdout);
assert.equal(accounts.length,2);const guid=accounts[0][0],foreign=accounts[1][0];
const listener=createServer();await new Promise(ok=>listener.listen(0,'127.0.0.1',ok));const port=listener.address().port;await new Promise(ok=>listener.close(ok));
const origin='http://127.0.0.1:'+port,log=await open(join(out,'gallery-api-server.log'),'a',0o600);
// Expected HTTP refusals log ERROR, which the native runner correctly treats as fatal; keep them in this closed-run API check.
const child=spawn(join(state,'server-build/openrotmg'),['--state',state,'--dev-control','--no-shore-bots','--client',client,'--port',String(port)],{stdio:['ignore',log.fd,log.fd]});
const closed=new Promise((ok,bad)=>{child.once('error',bad);child.once('close',(code,signal)=>ok({code,signal}));});
const checks=[],check=(name,ok)=>{assert(ok,name);checks.push(name);};
const request=async(op,form,status=200)=>{const r=await fetch(origin+'/legacy-api/picture/'+op,{method:'POST',body:new URLSearchParams(form),signal:AbortSignal.timeout(10000)});const body=Buffer.from(await r.arrayBuffer());assert.equal(r.status,status,body.toString());return body;};
const digest=png=>createHash('sha256').update(png).digest('hex');
try{
  const deadline=Date.now()+30000;
  while(true){assert(child.exitCode===null,'API server exited during startup');try{if((await fetch(origin+'/',{signal:AbortSignal.timeout(1000)})).ok)break;}catch(e){assert(Date.now()<deadline,String(e));}assert(Date.now()<deadline,'API startup deadline');await delay(100);}
  const first=saved.owned[0],ownerList=(await request('list',{myGUID:guid,guid})).toString();
  check('cold server restart retains 24 owned artworks',([...ownerList.matchAll(/<Pic id=/g)]).length===24&&ownerList.includes(`<Pic id="${first}">`));
  check('cold restart retains the exact native-saved PNG',digest(await request('get',{id:first}))===saved.pngSha256);
  check('cold restart retains foreign artwork',digest(await request('get',{id:saved.foreignId}))===saved.pngSha256);
  check('confirmed native deletion survives cold restart',(await request('get',{id:saved.rare},400)).toString()==='<Error>picture not found</Error>');
  check('public Mine marker is not deletion authority',ownerList.includes('<Mine/>')&&(await request('delete',{guid:foreign,password:'',id:first},400)).toString()==='<Error>picture not found or not owned by account</Error>');
  check('wrong password cannot delete artwork',(await request('delete',{guid,password:'invalid',id:first},400)).toString().includes('<Error>'));
  for(const invalid of [{offset:'-1'},{num:'25'},{dataType:'8'},{tags:'bad!'}])check('invalid query refused: '+JSON.stringify(invalid),(await request('list',{myGUID:guid,...invalid},400)).toString().startsWith('<Error>'));
  check('repeated confirmed deletion cannot succeed',(await request('delete',{guid,password:'',id:saved.rare},400)).toString()==='<Error>picture not found or not owned by account</Error>');
  check('all refusals preserve original and foreign PNGs',digest(await request('get',{id:first}))===saved.pngSha256&&digest(await request('get',{id:saved.foreignId}))===saved.pngSha256);
  await writeFile(join(out,'gallery-api.json'),JSON.stringify({status:'passed',native,revision:receipt.source.head,checks},null,2));
}finally{
  child.kill('SIGINT');const timer=setTimeout(()=>child.kill('SIGKILL'),10000);
  try{const result=await closed;assert.equal(result.code,0,JSON.stringify(result));}finally{clearTimeout(timer);await log.close();await writeFile(join(out,'gallery-api-checks.json'),JSON.stringify(checks,null,2));}
}
