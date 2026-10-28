using System;
using System.Linq;
using System.Text.RegularExpressions;
using System.Threading;
using System.Windows.Automation;

public partial class DesktopObserver {
    public class StopObservation {
        public string taskReviewDigest,turnRuntimeId,promptRuntimeId,stopRuntimeId;
        public Observation observation;
        public string[] baseline;
    }
    class StopView {
        public View view;public string turn,prompt,status;public string[] baseline;
        public AutomationElement button;
    }
    static string Runtime(AutomationElement node){return String.Join(".",node.GetRuntimeId());}
    static bool CancelledStatus(string status){return status!=null&&Regex.IsMatch(status,@"^状态：(已取消|取消|已停止|停止|已中断|中断)(?:，|$)");}
    static bool FinishedStatus(string status){return status!=null&&Regex.IsMatch(status,@"^状态：(成功|失败)(?:，|$)");}
    static bool WorkBuddyCancelled(string status){return new[]{"已取消","已停止","已中断","任务已停止","任务已取消"}.Contains(status);}
    static bool StopCancelled(StopView current){return current.view.observation.window.app=="workbuddy"?WorkBuddyCancelled(current.status):CancelledStatus(current.status);}
    static bool StopFinished(StopView current){return current.view.observation.window.app=="workbuddy"?current.status!=null&&(Regex.IsMatch(current.status,@"^已完成\s+(?:[0-9]+m)?[0-9]+(?:\.[0-9]+)?s$")||current.status=="已失败"):FinishedStatus(current.status);}
    class WorkBuddyStopFrame {public string turn,prompt,status;public string[] baseline;}
    static WorkBuddyStopFrame WorkBuddyStopHistory(TaskReview task,string document,WorkBuddyNode[] nodes) {
        if(nodes==null||nodes.Length==0||nodes.Length>1000||nodes.Any(n=>n==null||n.id==null||n.type==null||n.name==null||n.name.Length>65536)||nodes.Select(n=>n.id).Distinct().Count()!=nodes.Length)throw new InvalidOperationException("stop_history_changed");
        var byId=nodes.ToDictionary(n=>n.id);
        Func<string,string,WorkBuddyNode> require=(id,type)=>{WorkBuddyNode n;if(!byId.TryGetValue(id,out n)||n.type!="ControlType."+type)throw new InvalidOperationException("stop_turn_changed");return n;};
        string list=WorkBuddyListPath(document,task.observation.window.version);
        var turns=nodes.Where(n=>n.id.StartsWith(list+".",StringComparison.Ordinal)&&!n.id.Substring(list.Length+1).Contains('.')).ToArray();
        if(turns.Length<6||turns.Length>259||turns.Any(n=>n.type!="ControlType.Custom"))throw new InvalidOperationException("stop_history_changed");
        int previous=-1;
        foreach(var t in turns){int index;if(!Int32.TryParse(t.id.Substring(list.Length+1),out index)||index<0||(previous>=0&&index!=previous+1))throw new InvalidOperationException("stop_history_changed");previous=index;}
        var user=turns[turns.Length-2];var reply=turns.Last();
        WorkBuddyNode role;
        if(byId.TryGetValue(user.id+".0.1",out role)&&role.type=="ControlType.Text"&&role.name=="WorkBuddy")throw new InvalidOperationException("stop_turn_changed");
        if(require(reply.id+".0.1","Text").name!="WorkBuddy")throw new InvalidOperationException("stop_turn_changed");
        var prior=nodes.Where(n=>n.id!=user.id&&!n.id.StartsWith(user.id+".",StringComparison.Ordinal)&&n.id!=reply.id&&!n.id.StartsWith(reply.id+".",StringComparison.Ordinal)).ToArray();
        Round[] history;try{history=WorkBuddyRounds(document,prior,task.observation.window.version);}catch(InvalidOperationException){throw new InvalidOperationException("stop_history_changed");}
        var baseline=Baseline(history);
        if(baseline.Length<2||baseline.Length>task.baseline.Length||!baseline.SequenceEqual(task.baseline.Skip(task.baseline.Length-baseline.Length))||history.Any(r=>r.user.Contains(task.prompt)))throw new InvalidOperationException("stop_history_changed");
        string body=user.id+".0.0.0";require(body,"Custom");
        var userNodes=nodes.Where(n=>n.id.StartsWith(body+".",StringComparison.Ordinal)).ToArray();
        var prompts=userNodes.Where(n=>n.type=="ControlType.Text"&&!String.IsNullOrWhiteSpace(n.name)).ToArray();
        if(userNodes.Any(n=>n.type!="ControlType.Custom"&&n.type!="ControlType.Text")||prompts.Length!=1||prompts[0].name!=task.prompt)throw new InvalidOperationException("stop_turn_changed");
        string statusId=reply.id+".0.2",status=null;WorkBuddyNode statusRoot;
        var states=nodes.Where(n=>(n.id==statusId||n.id.StartsWith(statusId+".",StringComparison.Ordinal))&&n.type=="ControlType.Text"&&!String.IsNullOrWhiteSpace(n.name)).ToArray();
        if(byId.TryGetValue(statusId,out statusRoot)) {
            if((statusRoot.type!="ControlType.Custom"&&statusRoot.type!="ControlType.Text")||states.Length!=1||states[0].name.Length>512)throw new InvalidOperationException("stop_turn_changed");
            status=states[0].name;
        } else if(states.Length!=0)throw new InvalidOperationException("stop_turn_changed");
        // A malformed terminal-like label must never grant another stop action.
        if(status!=null) {
            bool terminalPrefix=new[]{"已取消","已停止","已中断","任务已停止","任务已取消","已完成","已失败"}.Any(s=>status.StartsWith(s,StringComparison.Ordinal));
            bool terminal=WorkBuddyCancelled(status)||Regex.IsMatch(status,@"^已完成 (?:[0-9]+m)?[0-9]+(?:\.[0-9]+)?s$")||status=="已失败";
            if(terminalPrefix&&!terminal)throw new InvalidOperationException("stop_turn_changed");
        }
        return new WorkBuddyStopFrame{turn=reply.id,prompt=prompts[0].id,status=status,baseline=baseline};
    }
    static StopView WorkBuddyStopView(TaskReview task,View view) {
        string document=view.observation.documentRuntimeId;
        var frame=WorkBuddyStopHistory(task,document,view.nodes.Select(n=>new WorkBuddyNode{id=Runtime(n),type=n.Cached.ControlType.ProgrammaticName,name=n.Cached.Name??""}).ToArray());
        string toolbar=WorkBuddyToolbar(view,"stop_control_unavailable");
        var controls=view.nodes.Where(n=>Runtime(n).StartsWith(toolbar+".",StringComparison.Ordinal)&&!Runtime(n).Substring(toolbar.Length+1).Contains('.')&&n.Cached.ControlType==ControlType.Button&&(n.Cached.Name=="发送"||n.Cached.Name=="停止")&&!n.Cached.IsOffscreen).ToArray();
        if(controls.Length!=1)throw new InvalidOperationException("stop_control_ambiguous");
        var control=controls[0];
        if(control.Cached.Name=="发送"&&control.Cached.IsEnabled)throw new InvalidOperationException("stop_identity_changed");
        if(control.Cached.Name=="停止"&&!control.Cached.IsEnabled)throw new InvalidOperationException("stop_control_unavailable");
        // Stop-looking controls anywhere else are ambiguity, never substitutes.
        if(view.nodes.Any(n=>n.Cached.ControlType==ControlType.Button&&n.Cached.IsEnabled&&!n.Cached.IsOffscreen&&(n.Cached.Name=="停止"||n.Cached.Name=="停止生成"||n.Cached.Name=="停止任务")&&Runtime(n)!=Runtime(control)))throw new InvalidOperationException("stop_control_ambiguous");
        return new StopView{view=view,turn=frame.turn,prompt=frame.prompt,status=frame.status,baseline=frame.baseline,button=control.Cached.Name=="停止"?control:null};
    }
    static StopView BoundStopView(TaskReview task) {
        ReviewTask(task,true);
        var view=ReadView(task.observation.window,false);Continuity(view,task);
        if(view.observation.modelLabel!=task.observation.modelLabel||!view.observation.draftEmpty)throw new InvalidOperationException("stop_identity_changed");
        if(task.observation.window.app=="workbuddy")return WorkBuddyStopView(task,view);
        var nodes=view.nodes;
        var begins=Enumerable.Range(0,nodes.Length).Where(i=>Group(nodes[i],"消息列表")).ToArray();
        var ends=Enumerable.Range(0,nodes.Length).Where(i=>Group(nodes[i],"消息锚点")).ToArray();
        if(begins.Length!=1||ends.Length!=1||ends[0]<=begins[0])throw new InvalidOperationException("stop_history_changed");
        int begin=begins[0],end=ends[0];
        var turns=Enumerable.Range(begin+1,end-begin-1).Where(i=>Group(nodes[i],"一轮任务")).ToArray();
        if(turns.Length<3||turns.Length>129)throw new InvalidOperationException("stop_history_changed");
        int last=turns.Last();
        // Parse only completed history with the existing exact round serializer.
        // The running final round may not expose reply actions or a terminal yet.
        var prior=new View{nodes=nodes.Take(last).Concat(new[]{nodes[end]}).ToArray()};
        var rounds=Rounds(prior);var baseline=Baseline(rounds);
        if(rounds.Any(r=>!r.completed&&!r.cancelled)||baseline.Length<2||baseline.Length>task.baseline.Length||!baseline.SequenceEqual(task.baseline.Skip(task.baseline.Length-baseline.Length)))throw new InvalidOperationException("stop_history_changed");
        var part=nodes.Skip(last+1).Take(end-last-1).ToArray();
        var agents=Enumerable.Range(0,part.Length).Where(i=>Group(part[i],"Agent 回复")).ToArray();
        if(agents.Length!=1)throw new InvalidOperationException("stop_turn_changed");
        var prompts=part.Take(agents[0]).Where(n=>n.Cached.ControlType==ControlType.Text&&n.Cached.Name==task.prompt).ToArray();
        if(prompts.Length!=1)throw new InvalidOperationException("stop_turn_changed");
        var statuses=part.Where(n=>n.Cached.ControlType==ControlType.Group&&(n.Cached.Name??"").StartsWith("状态：")).ToArray();
        if(statuses.Length>1||(statuses.Length==1&&statuses[0].Cached.Name.Length>512))throw new InvalidOperationException("stop_turn_changed");
        // Never use a stop-looking button inside old messages, tool output or a reply.
        var buttons=Enumerable.Range(0,nodes.Length).Where(i=>(i<begin||i>end)&&nodes[i].Cached.ControlType==ControlType.Button&&nodes[i].Cached.Name=="停止生成"&&nodes[i].Cached.IsEnabled&&!nodes[i].Cached.IsOffscreen).Select(i=>nodes[i]).ToArray();
        if(buttons.Length>1)throw new InvalidOperationException("stop_control_ambiguous");
        return new StopView{view=view,turn=Runtime(nodes[last]),prompt=Runtime(prompts[0]),status=statuses.Length==1?statuses[0].Cached.Name:null,baseline=baseline,button=buttons.SingleOrDefault()};
    }
    static StopObservation StopObserved(TaskReview task,StopView current) {
        if(current.button==null||StopCancelled(current)||StopFinished(current)||!current.view.observation.busy)throw new InvalidOperationException("stop_not_running");
        return new StopObservation{taskReviewDigest=task.id,observation=current.view.observation,baseline=current.baseline,turnRuntimeId=current.turn,promptRuntimeId=current.prompt,stopRuntimeId=Runtime(current.button)};
    }
    static object PrepareStop(TaskReview task){return new{stopObservation=StopObserved(task,BoundStopView(task))};}
    // Observation only: no focus, editor access, Invoke, send, or terminal claim.
    // Reuse original-task/history binding, but never expose a stop capability.
    static object ReadTaskProgress(TaskReview task) {
        var current=BoundStopView(task);
        var nodes=current.view.nodes;
        AutomationElement[] turn;
        if(current.view.observation.window.app=="workbuddy") {
            turn=nodes.Where(n=>Runtime(n)==current.turn||Runtime(n).StartsWith(current.turn+".",StringComparison.Ordinal)).ToArray();
        } else {
            int start=Array.FindIndex(nodes,n=>Runtime(n)==current.turn);
            if(start<0)throw new InvalidOperationException("stop_turn_changed");
            int end=Array.FindIndex(nodes,start+1,n=>Group(n,"一轮任务")||Group(n,"消息锚点"));
            if(end<=start)throw new InvalidOperationException("stop_turn_changed");
            turn=nodes.Skip(start).Take(end-start).ToArray();
        }
        if(turn.Length==0||turn.Length>1000)throw new InvalidOperationException("tree_limit");
        // Hash current visible nodes; never return reply, tool-output or reasoning text.
        string visible=Hash(json.Serialize(turn.Select(n=>new{type=n.Cached.ControlType.ProgrammaticName,name=n.Cached.Name??""}).ToArray()));
        return new{taskProgress=new{schemaVersion=1,taskReviewDigest=task.id,observation=current.view.observation,baseline=current.baseline,turnRuntimeId=current.turn,promptRuntimeId=current.prompt,visibleDigest=visible,statusText=current.status}};
    }
    static void StopReview(TaskReview task,StopObservation expected,bool fresh) {
        ReviewTask(task,true);
        if(expected==null||expected.observation==null||expected.taskReviewDigest!=task.id||expected.taskReviewDigest==null||!Regex.IsMatch(expected.taskReviewDigest,@"^[0-9a-f]{64}$")||expected.baseline==null||expected.baseline.Length<2||expected.baseline.Length>task.baseline.Length||!expected.baseline.SequenceEqual(task.baseline.Skip(task.baseline.Length-expected.baseline.Length))||String.IsNullOrWhiteSpace(expected.turnRuntimeId)||String.IsNullOrWhiteSpace(expected.promptRuntimeId)||String.IsNullOrWhiteSpace(expected.stopRuntimeId))throw new InvalidOperationException("stop_review_invalid");
        if(!Same(expected.observation.window,task.observation.window)||expected.observation.sessionId!=task.observation.sessionId||expected.observation.documentRuntimeId!=task.observation.documentRuntimeId||expected.observation.modelLabel!=task.observation.modelLabel||!expected.observation.draftEmpty||!expected.observation.busy)throw new InvalidOperationException("stop_review_invalid");
        DateTime at;if(!DateTime.TryParse(expected.observation.capturedAt,null,System.Globalization.DateTimeStyles.RoundtripKind,out at))throw new InvalidOperationException("stop_review_invalid");
        var age=DateTime.UtcNow-at.ToUniversalTime();
        if(age.TotalSeconds<0||(fresh&&age.TotalSeconds>30))throw new InvalidOperationException("stop_review_expired");
    }
    static void MatchStop(StopView current,StopObservation expected,bool running) {
        if(current.turn!=expected.turnRuntimeId||current.prompt!=expected.promptRuntimeId||current.baseline.Length<2||current.baseline.Length>expected.baseline.Length||!current.baseline.SequenceEqual(expected.baseline.Skip(expected.baseline.Length-current.baseline.Length)))throw new InvalidOperationException("stop_turn_changed");
        if(running&&(current.button==null||Runtime(current.button)!=expected.stopRuntimeId||StopCancelled(current)||StopFinished(current)||!current.view.observation.busy))throw new InvalidOperationException("stop_not_running");
    }
    static object CancelledProof(TaskReview task,StopObservation expected,StopView first,StopView second) {
        MatchStop(first,expected,false);MatchStop(second,expected,false);
        if(!StopCancelled(first)||!StopCancelled(second)||first.status!=second.status||first.button!=null||second.button!=null||first.view.observation.busy||second.view.observation.busy)throw new InvalidOperationException("stop_terminal_unverified");
        return new{stopProof=new{taskReviewDigest=task.id,observation=second.view.observation,firstObservedAt=first.view.observation.capturedAt,turnRuntimeId=second.turn,promptRuntimeId=second.prompt,observedBaseline=second.baseline,statusText=second.status,cancelled=true}};
    }
    static object ReadStop(TaskReview task,StopObservation expected) {
        StopReview(task,expected,false);var first=BoundStopView(task);Thread.Sleep(250);var second=BoundStopView(task);
        return CancelledProof(task,expected,first,second);
    }
    static object StopTask(TaskReview task,StopObservation expected) {
        // Both versioned adapters use the same one-invocation stop path. The
        // trusted host owns the durable one-shot capability, including recovery.
        ReviewTask(task,true);
        RequireDesktopActionVersion(task.observation.window);
        StopReview(task,expected,true);
        using(var mutex=new Mutex(false,@"Local\CeruleanDesktopTrialActionV1")) {
            bool acquired=false;
            try {
                try{acquired=mutex.WaitOne(0);}catch(AbandonedMutexException){acquired=true;}
                if(!acquired)throw new InvalidOperationException("trial_lease_busy");
                var current=BoundStopView(task);MatchStop(current,expected,true);
                var hwnd=new IntPtr(Int64.Parse(task.observation.window.hwnd));
                SetForegroundWindow(hwnd);Thread.Sleep(100);Foreground(hwnd,task.observation.window);
                current=BoundStopView(task);MatchStop(current,expected,true);Foreground(hwnd,task.observation.window);
                object invoke;if(!current.button.TryGetCurrentPattern(InvokePattern.Pattern,out invoke))throw new InvalidOperationException("stop_control_unavailable");
                // One Invoke only. No keyboard, clipboard, retry, process kill or other window.
                ((InvokePattern)invoke).Invoke();
                var until=DateTime.UtcNow.AddSeconds(10);
                while(DateTime.UtcNow<until) {
                    Thread.Sleep(250);
                    try{return ReadStop(task,expected);}catch(InvalidOperationException e){if(e.Message!="stop_terminal_unverified")throw;}
                }
                throw new InvalidOperationException("stop_terminal_unverified");
            } finally {if(acquired)mutex.ReleaseMutex();}
        }
    }
}
