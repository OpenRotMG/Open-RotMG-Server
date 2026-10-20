import {spawn} from 'node:child_process';
import {readFileSync,statSync,writeFileSync,renameSync} from 'node:fs';
import {join} from 'node:path';
import {fileURLToPath} from 'node:url';
import {setTimeout as delay} from 'node:timers/promises';

export function identity(pid) {
  try {
    const path='/proc/'+pid,stat=readFileSync(path+'/stat','utf8'),fields=stat.slice(stat.lastIndexOf(')')+2).split(' ');
    return fields[0]==='Z'?null:{pid,uid:statSync(path).uid,start:fields[19]};
  }catch(error){if(['ENOENT','ESRCH'].includes(error.code))return null;throw error;}
}
export function sameProcess(row) {const live=identity(row.pid);return live&&live.uid===row.uid&&live.start===row.start;}
export function registry(path,value) {writeFileSync(path+'.tmp',JSON.stringify(value),{mode:0o600});renameSync(path+'.tmp',path);}
export function startChild(command,args,options,signal) {
  signal.throwIfAborted();const child=spawn(command,args,{...options,detached:true});
  const closed=new Promise(resolve=>child.once('close',(code,killed)=>resolve({pid:child.pid,code,signal:killed})));
  const started=new Promise((resolve,reject)=>{child.once('spawn',resolve);child.once('error',reject);});started.catch(()=>{});
  return {child,closed,started,done:false,identity:child.pid?identity(child.pid):null};
}
export async function stopChild(row,grace=3000) {
  for(const [name,ms]of [['SIGINT',grace],['SIGTERM',2000],['SIGKILL',5000]]){
    if(row.done)return await row.closed;
    if(!row.child.pid)await row.started;
    try{process.kill(-row.child.pid,name);}catch(error){if(error.code!=='ESRCH')throw error;}
    const stopped=await Promise.race([row.closed.then(value=>({value})),delay(ms,undefined,{ref:false}).then(()=>null)]);
    if(stopped){row.done=true;return stopped.value;}
  }
  throw Error('Owned child did not close after SIGKILL: '+row.child.pid);
}
export async function guardRun(directory) {
  const path=join(directory,'children.json'),boot=readFileSync('/proc/sys/kernel/random/boot_id','utf8');let state;
  while(true){
    state=JSON.parse(readFileSync(path,'utf8'));
    if(state.done)return;
    if(state.boot!==boot||state.parent.uid!==process.getuid())throw Error('Guardian owner/boot mismatch');
    if(!sameProcess(state.parent)||Date.now()>=state.deadline)break;
    await delay(250);
  }
  const reason=sameProcess(state.parent)?'Absolute run deadline reached':'Runner process disappeared';
  registry(join(directory,'guardian.json'),{reason,children:state.children});
  const resultPath=join(directory,'result.json'),result=JSON.parse(readFileSync(resultPath,'utf8'));
  if(result.status==='running'){result.status='failed';result.failure={phase:'guardian',error:reason};writeFileSync(resultPath+'.guardian',JSON.stringify(result,null,2));renameSync(resultPath+'.guardian',resultPath);}
  const children=state.children.filter(row=>row.uid===process.getuid());
  for(const [signal,ms]of [['SIGINT',1000],['SIGTERM',1000],['SIGKILL',2000]]){
    for(const row of [...children].reverse())if(sameProcess(row))try{process.kill(-row.pid,signal);}catch(error){if(error.code!=='ESRCH')throw error;}
    await delay(ms);if(children.every(row=>!sameProcess(row)))break;
  }
  if(sameProcess(state.parent))process.kill(state.parent.pid,'SIGKILL');
  const remaining=children.filter(sameProcess);registry(join(directory,'guardian.json'),{reason,remaining,children});
  if(remaining.length)throw Error('Guardian could not stop owned children');
}
if(process.argv[1]===fileURLToPath(import.meta.url))await guardRun(process.argv[2]);
