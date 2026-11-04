import type { ProjectSetupDraft, WorkerProfileDraft } from "./contracts";
import { apiTargetKey } from "./apiModels";
import { resourceError, resourceNames } from "./ResourceFields";

export const automaticTaskDescription = "由副指挥根据已批准方案、任务依赖和当前执行进度自动分配任务。";
const automaticResource = /^executor-[a-f0-9]{64}$/;

export function workerConfigurationIdentity(worker: Pick<ProjectSetupDraft,"workerAdapter"|"workerModel"|"codexExecutable"|"apiWorker"|"desktopWorker">): string {
  const adapter = worker.workerAdapter ?? "codex";
  if (worker.apiWorker) return JSON.stringify([adapter,apiTargetKey(worker.apiWorker.target),worker.workerModel.trim()]);
  if (worker.desktopWorker) return JSON.stringify([adapter,worker.desktopWorker.modelLabel,worker.desktopWorker.binding.observation.sessionId]);
  return JSON.stringify([adapter,worker.codexExecutable.trim().replaceAll("/","\\").toLowerCase(),worker.workerModel.trim()]);
}

export async function suggestedWorkerResource(profile: WorkerProfileDraft): Promise<string> {
  const bytes = new TextEncoder().encode(workerConfigurationIdentity(profile));
  const digest = new Uint8Array(await crypto.subtle.digest("SHA-256",bytes));
  const name = `executor-${Array.from(digest,byte=>byte.toString(16).padStart(2,"0")).join("")}`;
  if (resourceError([name])) throw Error("自动生成的资源名称未通过校验");
  return name;
}

export function withAutomaticResource(profile: WorkerProfileDraft, name: string, common?: string[]): WorkerProfileDraft {
  const keys = resourceNames(profile.resourceKeys);
  if (keys.length && !keys.some(key=>automaticResource.test(key))) return profile;
  const next = [...new Set([...keys.filter(key=>!automaticResource.test(key)),name])];
  if (resourceError(next,common)) return profile;
  return keys.join("\n") === next.join("\n") ? profile : {...profile,resourceKeys:next};
}

export function artifactOverlap(left: string, right: string): boolean {
  const normalize = (path: string) => path.trim().replaceAll("\\","/").toUpperCase().replace(/\/$/,"");
  const a = normalize(left), b = normalize(right);
  return !!a && !!b && (a === b || a.startsWith(`${b}/`) || b.startsWith(`${a}/`));
}

// All changes are draft edits. Existing approved revisions retain their bindings.
export function synchronizeWorkerProfiles(profiles: WorkerProfileDraft[], editedIndex: number, common?:string[], options={files:false,resources:false}): {profiles:WorkerProfileDraft[]; notices:string[]} {
  const selected = profiles[editedIndex];
  if (!selected) return {profiles,notices:[]};
  const notices: string[] = [];
  const identity = workerConfigurationIdentity(selected);
  const resources = options.resources ? resourceNames(selected.resourceKeys) : [...new Set(profiles.filter(profile=>workerConfigurationIdentity(profile)===identity).flatMap(profile=>resourceNames(profile.resourceKeys)))];
  const error=resourceError(resources,common);
  if(error)throw Error(`同一执行端的资源同步未写入。${error}`);
  const next = profiles.map((profile,index) => {
    let result = profile;
    if (profile.apiWorker && profile.apiWorker.model !== profile.workerModel) result = {...result,apiWorker:{...profile.apiWorker,model:profile.workerModel}};
    if (workerConfigurationIdentity(profile) === identity) {
      if (resources.join("\n") !== resourceNames(profile.resourceKeys).join("\n")) {
        result = {...result,resourceKeys:resources};
        if(index!==editedIndex)notices.push(`${profile.id} 已同步同一执行端的共享资源`);
      }
      return result;
    }
    if(!options.files)return result;
    const paths = profile.artifactPaths.filter(path=>!selected.artifactPaths.some(other=>artifactOverlap(path,other)));
    if (paths.length !== profile.artifactPaths.length) {
      result = {...result,artifactPaths:paths};
      notices.push(`${profile.id} 的重复文件已交由 ${selected.id} 负责${paths.length ? "" : "，请补充它的其他交付文件或移除该配置"}`);
    }
    return result;
  });
  return {profiles:next,notices};
}
