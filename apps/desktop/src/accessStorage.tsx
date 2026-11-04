import { useSyncExternalStore } from "react";
import { invoke } from "@tauri-apps/api/core";

let native=false, blocked=false, blockedEdits=false, pending=0;
let tail:Promise<void>=Promise.resolve();
const failed=new Map<string,{value:string;message:string}>();
const listeners=new Set<()=>void>();
let state={pending:0,error:""};
const emit=()=>{state={pending,error:blocked?"无法读取本机接入配置，请重新打开工作台后重试。":failed.values().next().value?.message??""};listeners.forEach(fn=>fn());};
export function useAccessPersistence(){return useSyncExternalStore(callback=>{listeners.add(callback);return()=>{listeners.delete(callback);};},()=>state);}

export async function initializeAccessStorage(){
  if(!("__TAURI_INTERNALS__" in window))return;
  native=true;
  try {
    const entries=await invoke<Record<string,string>>("access_settings_load");
    for(const [key,value] of Object.entries(entries))localStorage.setItem(key,value);
    blocked=false;
  }catch{blocked=true;}
  emit();
}
function enqueue(key:string,value:string){
  if(blocked)return;
  pending++;emit();
  tail=tail.then(async()=>{
    try {await invoke("access_setting_put",{key,value});failed.delete(key);}
    catch {failed.set(key,{value,message:"接入设置尚未写入本机文件，输入仍在当前窗口，请重试保存。"});}
    finally{pending--;emit();}
  });
}
export function setAccessItem(key:string,value:string):boolean{
  try {localStorage.setItem(key,value);if(blocked)blockedEdits=true;else if(native)enqueue(key,value);return !blocked;}
  catch{return false;}
}
export async function flushAccessStorage(retry=false):Promise<boolean>{
  let observed:Promise<void>;
  do{observed=tail;await observed;}while(observed!==tail);
  if(retry&&!blocked){for(const [key,{value}] of failed)enqueue(key,value);await tail;}
  return !blockedEdits&&failed.size===0;
}
export function AccessStorageStatus(){
  const current=useAccessPersistence();
  if(current.error)return <div className="banner error" role="alert">{current.error} <button type="button" onClick={()=>blocked?window.location.reload():void flushAccessStorage(true)}>{blocked?"重新读取接入配置":"重试保存接入设置"}</button></div>;
  return current.pending>0?<div className="banner" role="status">正在保存接入设置到本机…</div>:null;
}
