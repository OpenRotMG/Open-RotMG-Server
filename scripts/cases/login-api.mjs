import assert from 'node:assert/strict';
import {spawn,execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {open,readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {createServer} from 'node:net';
import {setTimeout as delay} from 'node:timers/promises';

const [native,client]=process.argv.slice(2),out=process.env.QA_RESULTS_DIR;
assert(native&&client&&out,'Run through qa.py exec with a closed login native run and its exact client directory');
const receipt=JSON.parse(await readFile(join(native,'result.json'),'utf8'));
assert(receipt.status==='passed'&&receipt.cleanupErrors.length===0,'Native run must have passed and closed');
const state=join(native,'state'),exec=promisify(execFile);
const query=async sql=>JSON.parse((await exec('python3',['-c','import sqlite3,json,sys; c=sqlite3.connect(sys.argv[1]); print(json.dumps(c.execute(sys.argv[2]).fetchall()))',join(state,'game.db'),sql])).stdout);
const snapshot=()=>query('SELECT a.id,a.name,a.credits,f.balance,f.earned,r.email,r.password_hash,c.* FROM accounts a JOIN account_fame f ON f.account=a.id LEFT JOIN credentials r ON r.account=a.id LEFT JOIN characters c ON c.account=a.id ORDER BY a.id,c.id');
const before=await snapshot(),account=receipt.account;
const listener=createServer();await new Promise(ok=>listener.listen(0,'127.0.0.1',ok));const port=listener.address().port;await new Promise(ok=>listener.close(ok));
const origin='http://127.0.0.1:'+port,log=await open(join(out,'api-server.log'),'a',0o600);
const child=spawn(join(state,'server-build/openrotmg'),['--state',state,'--dev-control','--no-shore-bots','--client',client,'--port',String(port)],{stdio:['ignore',log.fd,log.fd]});
const closed=new Promise((ok,bad)=>{child.once('error',bad);child.once('close',(code,signal)=>ok({code,signal}));});
const checks=[],check=(name,value)=>{assert(value,name);checks.push(name);};
try {
  const deadline=Date.now()+30000;
  while(true) {
    assert(child.exitCode===null,'Cold server exited during startup');
    try {if((await fetch(origin+'/',{signal:AbortSignal.timeout(1000)})).ok)break;}catch(error){assert(Date.now()<deadline,'Cold server did not start: '+error);}
    assert(Date.now()<deadline,'Cold server did not become ready');await delay(100);
  }
  let requests=0;
  const request=async(path,guid='login-921@example.test',password='fixture-password')=>{
    requests++;
    const r=await fetch(origin+'/legacy-api/'+path,{method:'POST',body:new URLSearchParams({guid,password}),signal:AbortSignal.timeout(10000)});
    return {body:await r.text(),status:r.status,type:r.headers.get('content-type'),cache:r.headers.get('cache-control'),retry:r.headers.get('retry-after')};
  };
  const valid=await request('account/verify');
  check('cold credential verification does not falsely mark email verified',valid.status===200&&valid.body==='<Account/>'&&valid.cache==='no-store'&&valid.type==='application/xml');
  for(let i=0;i<5;i++)check('wrong password rejection '+(i+1),(await request('account/verify','login-921@example.test','wrong-password')).body.includes('<Error>invalid email or password</Error>'));
  check('five failures do not impose an undocumented account lockout',(await request('account/verify')).body==='<Account/>');
  check('unknown email uses the same semantic refusal',(await request('account/verify','missing@example.test')).body.includes('<Error>invalid email or password</Error>'));
  check('empty registered password cannot become a guest',(await request('char/list','login-921@example.test','')).body.includes('<Error>'));
  check('wrong password cannot read saved characters',(await request('char/list','login-921@example.test','wrong-password')).body==='<Error>Account credentials not valid</Error>');
  const chars=(await request('char/list','LOGIN-921@EXAMPLE.TEST')).body;
  check('case-normalized credentials load the original account and living character after restart',chars.includes(`<AccountId>${account}</AccountId>`)&&chars.includes(`<Char id="${receipt.after.session.character}"`));
  assert.deepEqual(await snapshot(),before,'Authentication changed account or character state');
  check('successes and refusals preserve every saved character and wallet',true);
  while(requests<20)assert.equal((await request('account/verify')).status,200);
  const refused=await request('account/verify');
  check('shared IP admission refuses request 21 explicitly',refused.status===429&&refused.retry==='60'&&refused.body.includes('Too many guest entry requests'));
  await delay(61000);
  check('IP admission recovers after its one-minute window',(await request('account/verify')).body==='<Account/>');
  assert.deepEqual(await snapshot(),before,'Rate admission changed persisted progress');
} finally {
  child.kill('SIGINT');const timer=setTimeout(()=>child.kill('SIGKILL'),10000);
  try {assert.deepEqual(await closed,{code:0,signal:null},'Cold server did not shut down cleanly');}finally{clearTimeout(timer);await log.close();}
}
await writeFile(join(out,'login-api.json'),JSON.stringify({status:'passed',native,revision:receipt.source.head,checks},null,2));
