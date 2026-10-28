using System;
using System.Collections.Generic;
using System.Linq;
using System.Runtime.InteropServices;
using System.Text.RegularExpressions;
using System.Threading;
using System.Windows.Automation;

public partial class DesktopObserver {
    public class TrialReview {public string id,bindingId,marker,prompt,bridgeSha256;public Observation observation;public string[] baseline;}
    public class TaskReview:TrialReview {public string promptSha256;public int maxWaitSeconds;}
    class Round {public string[] user;public string reply;public bool completed,cancelled;[System.Web.Script.Serialization.ScriptIgnore]public string richBodyDigest;}
    [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] static extern bool SetForegroundWindow(IntPtr window);
    [StructLayout(LayoutKind.Sequential)] struct KeyboardInput {public ushort vk,scan;public uint flags,time;public IntPtr extra;}
    [StructLayout(LayoutKind.Sequential)] struct MouseInput {public int x,y;public uint data,flags,time;public IntPtr extra;}
    [StructLayout(LayoutKind.Explicit)] struct InputData {[FieldOffset(0)]public KeyboardInput keyboard;[FieldOffset(0)]public MouseInput mouse;}
    [StructLayout(LayoutKind.Sequential)] struct Input {public uint type;public InputData data;}
    [DllImport("user32.dll",SetLastError=true)] static extern uint SendInput(uint count,Input[] inputs,int size);
    [DllImport("user32.dll")] static extern short GetAsyncKeyState(int key);
    [StructLayout(LayoutKind.Sequential)] struct FocusPoint {public int x,y;}
    [DllImport("user32.dll",EntryPoint="WindowFromPoint")] static extern IntPtr ComposerWindowFromPoint(FocusPoint point);
    [DllImport("user32.dll",EntryPoint="GetAncestor")] static extern IntPtr ComposerAncestor(IntPtr hwnd,uint flags);
    [DllImport("user32.dll",EntryPoint="SetCursorPos")] static extern bool ComposerCursor(int x,int y);
    static void FocusComposer(View view,IntPtr hwnd) {
        if(ComposerHasFocus(view))return;
        if(view.observation.window.app!="workbuddy"||view.editor.Current.IsKeyboardFocusable){view.editor.SetFocus();return;}
        // WorkBuddy's contenteditable exposes ValuePattern but rejects UIA
        // SetFocus. Click only the freshly hit-tested original composer once.
        var rect=view.editor.Current.BoundingRectangle;
        if(rect.IsEmpty||rect.Width<40||rect.Height<20)throw new InvalidOperationException("trial_focus_changed");
        var point=new FocusPoint{x=(int)(rect.Left+rect.Width/2),y=(int)(rect.Top+rect.Height/2)};
        if(new[]{0x10,0x11,0x12,0x5B,0x5C}.Any(key=>(GetAsyncKeyState(key)&0x8000)!=0))throw new InvalidOperationException("trial_focus_changed");
        Foreground(hwnd,view.observation.window);
        if(ComposerAncestor(ComposerWindowFromPoint(point),2)!=hwnd)throw new InvalidOperationException("trial_focus_changed");
        var hit=AutomationElement.FromPoint(new System.Windows.Point(point.x,point.y));
        if(hit==null||Runtime(hit)!=Runtime(view.editor))throw new InvalidOperationException("trial_focus_changed");
        if(!ComposerCursor(point.x,point.y))throw new InvalidOperationException("trial_focus_changed");
        Foreground(hwnd,view.observation.window);
        if(ComposerAncestor(ComposerWindowFromPoint(point),2)!=hwnd)throw new InvalidOperationException("trial_focus_changed");
        var click=new[]{new Input{type=0,data=new InputData{mouse=new MouseInput{flags=2}}},new Input{type=0,data=new InputData{mouse=new MouseInput{flags=4}}}};
        if(SendInput(2,click,Marshal.SizeOf(typeof(Input)))!=2)throw new InvalidOperationException("trial_focus_changed");
        Thread.Sleep(100);
    }
    static bool ComposerHasFocus(View view) {
        if(!view.editor.Current.HasKeyboardFocus)return false;
        var focused=AutomationElement.FocusedElement;if(focused==null)return false;
        if(Runtime(focused)==Runtime(view.editor))return true;
        var w=view.observation.window;
        // Observed 5.7.7 and 5.8.3 host-provider alias: UIA global focus reports the
        // window's direct Document while the content provider marks its Edit.
        if(!WorkBuddyReadable(w)||focused.Current.ControlType!=ControlType.Document||focused.Current.ProcessId!=w.pid||Runtime(focused)!="42."+w.hwnd+".4.3")return false;
        var parent=TreeWalker.RawViewWalker.GetParent(focused);
        if(parent==null||parent.Current.ControlType!=ControlType.Window||parent.Current.ProcessId!=w.pid||parent.Current.NativeWindowHandle!=Int64.Parse(w.hwnd))return false;
        var editors=view.nodes.Where(n=>n.Cached.ControlType==ControlType.Edit&&!n.Current.IsOffscreen&&n.Current.HasKeyboardFocus).ToArray();
        return editors.Length==1&&Runtime(editors[0])==Runtime(view.editor);
    }
    static void SendStage(string stage){Console.Error.WriteLine("desktop-send-stage:"+stage);}
    static string Prompt(string marker){return "这是一次独立的工作台接入验证。请仅回复本条新标记："+marker+"。不添加解释，不读取或修改文件，不调用工具，不运行命令。";}
    static bool Group(AutomationElement node,string name){return node.Cached.ControlType==ControlType.Group&&node.Cached.Name==name;}
    static int QoderHistoryEnd(string[] snapshot,int begin,int[] anchors,string[] descendants) {
        if(begin<0||begin>=snapshot.Length||anchors.Length>1)throw new InvalidOperationException("trial_history_unavailable");
        if(anchors.Length==1) {
            if(anchors[0]<=begin||anchors[0]>=snapshot.Length)throw new InvalidOperationException("trial_history_unavailable");
            return anchors[0];
        }
        // New 0.4.3 chats may omit the old sibling anchor. Bound history to
        // the actual message-list subtree, never to the remainder of the page.
        if(descendants==null||descendants.Length==0||descendants.Length>snapshot.Length-begin-1||
            descendants.Distinct().Count()!=descendants.Length||
            !snapshot.Skip(begin+1).Take(descendants.Length).SequenceEqual(descendants))
            throw new InvalidOperationException("trial_history_unavailable");
        return begin+1+descendants.Length;
    }
    static Round[] Rounds(View view) {
        var nodes=view.nodes;
        var begin=Enumerable.Range(0,nodes.Length).Where(i=>Group(nodes[i],"消息列表")).ToArray();
        var end=Enumerable.Range(0,nodes.Length).Where(i=>Group(nodes[i],"消息锚点")).ToArray();
        if(begin.Length!=1)throw new InvalidOperationException("trial_history_unavailable");
        string[] descendants=null;
        if(end.Length==0)descendants=nodes[begin[0]].FindAll(TreeScope.Descendants,Automation.ControlViewCondition).Cast<AutomationElement>().Select(Runtime).ToArray();
        int boundary=QoderHistoryEnd(nodes.Select(CachedRuntime).ToArray(),begin[0],end,descendants);
        var starts=Enumerable.Range(begin[0]+1,boundary-begin[0]-1).Where(i=>Group(nodes[i],"一轮任务")).ToArray();
        var rounds=new List<Round>();
        for(int k=0;k<starts.Length;k++) {
            var part=nodes.Skip(starts[k]+1).Take((k+1<starts.Length?starts[k+1]:boundary)-starts[k]-1).ToArray();
            var agent=Enumerable.Range(0,part.Length).Where(i=>Group(part[i],"Agent 回复")).ToArray();
            var actions=Enumerable.Range(0,part.Length).Where(i=>Group(part[i],"回复操作")).ToArray();
            var states=part.Where(n=>n.Cached.ControlType==ControlType.Group&&(n.Cached.Name??"").StartsWith("状态：")).ToArray();
            if(agent.Length!=1||actions.Length!=1||actions[0]<=agent[0]||states.Length!=1)throw new InvalidOperationException("trial_history_unavailable");
            rounds.Add(new Round{user=part.Take(agent[0]).Where(n=>n.Cached.ControlType==ControlType.Text).Select(n=>n.Cached.Name).ToArray(),
                reply=String.Concat(part.Skip(agent[0]+1).Take(actions[0]-agent[0]-1).Where(n=>n.Cached.ControlType==ControlType.Text).Select(n=>n.Cached.Name)).Trim(),
                completed=Regex.IsMatch(states[0].Cached.Name,@"^状态：成功(?:，|$)"),cancelled=Regex.IsMatch(states[0].Cached.Name,@"^状态：(已取消|取消|已停止|停止|已中断|中断)(?:，|$)")});
        }
        return rounds.ToArray();
    }
    static string[] Baseline(Round[] rounds){return rounds.Select(r=>Hash(r.richBodyDigest==null?json.Serialize(r):json.Serialize(new{user=r.user,reply=r.reply,completed=r.completed,cancelled=r.cancelled,richBodyDigest=r.richBodyDigest}))).ToArray();}
    static void Enabled(Window window){if(window==null||window.app!="qoder"||window.version!="0.4.3.0")throw new InvalidOperationException("trial_app_not_enabled");}
    static bool WorkBuddyReadable(Window window){return window!=null&&window.app=="workbuddy"&&(window.version=="5.7.7.0"||window.version=="5.8.3.0");}
    static void RequireDesktopActionVersion(Window window){if(window!=null&&window.app=="workbuddy"&&!WorkBuddyReadable(window))throw new InvalidOperationException("trial_app_not_enabled");}
    static string WorkBuddyListPath(string document,string version) {
        if(version!="5.7.7.0"&&version!="5.8.3.0")throw new InvalidOperationException("trial_history_unavailable");
        return document+".4.0.1.0.0.0.1.1.0.0.0.0.0.0"+(version=="5.8.3.0"?".0":"");
    }
    static string WorkBuddyToolbar(View view,string error) {
        string panel=view.observation.documentRuntimeId+".4.0.1.0.0.0.1";
        string version=view.observation.window.version;
        string[] candidates=version=="5.8.3.0"?new[]{panel+".1.0.0.1"}:version=="5.7.7.0"?new[]{panel+".1.0.1",panel+".1.0.2"}:new string[0];
        var matched=candidates.Where(p=>Runtime(view.editor)==p+".0.1.0.1.0").ToArray();
        if(matched.Length!=1)throw new InvalidOperationException(error);
        return matched[0]+".0.1.0.2.2";
    }
    static object PrepareTrial(Window window) {
        Enabled(window);var view=ReadView(window,true);var rounds=Rounds(view);
        if(rounds.Length<2||rounds.Length>128||rounds.Any(r=>!r.completed&&!r.cancelled))throw new InvalidOperationException("trial_history_unavailable");
        return new{observation=view.observation,baseline=Baseline(rounds)};
    }
    static object PrepareTask(Window window,TaskReview previous) {
        // RuntimeHost binds this native contract to the persisted approved attempt.
        if(WorkBuddyReadable(window)) {
            if(previous!=null)ReviewTask(previous,true);
            var buddy=ReadView(window,true);
            if(String.IsNullOrWhiteSpace(buddy.observation.modelLabel))throw new InvalidOperationException("trial_history_unavailable");
            var history=TaskRounds(buddy);
            if(previous!=null)TaskResult(previous,buddy,history);
            return new{observation=buddy.observation,baseline=Baseline(history)};
        }
        Enabled(window);if(previous!=null)ReviewTask(previous);
        var view=ReadView(window,true);var rounds=Rounds(view);
        if(rounds.Length<2||rounds.Length>128||rounds.Any(r=>!r.completed&&!r.cancelled))throw new InvalidOperationException("trial_history_unavailable");
        if(previous!=null) {
            if(!Same(previous.observation.window,window))throw new InvalidOperationException("trial_view_changed");
            TaskResult(previous,view,rounds);
        }
        return new{observation=view.observation,baseline=Baseline(rounds)};
    }
    // Versioned structural paths resolved anew from the current document runtime ID.
    // Runtime IDs are never treated as a provider conversation/turn identifier.
    class WorkBuddyNode {public string id,type,name;}
    static string WorkBuddyReply(string body,WorkBuddyNode[] all,out string richBodyDigest,bool rendered=false,bool lists=false) {
        richBodyDigest=null;
        var nodes=all.Where(n=>n.id.StartsWith(body+".",StringComparison.Ordinal)).ToArray();
        if(nodes.Length==0)throw new InvalidOperationException("trial_history_unavailable");
        var byId=nodes.ToDictionary(n=>n.id);
        var children=nodes.GroupBy(n=>n.id.Substring(0,n.id.LastIndexOf('.'))).ToDictionary(g=>g.Key,g=>g.ToArray());
        foreach(var group in children) {
            if(group.Key!=body) {
                WorkBuddyNode parent;if(!byId.TryGetValue(group.Key,out parent))throw new InvalidOperationException("trial_history_unavailable");
                bool container=parent.type=="ControlType.Custom";
                bool code=rendered&&parent.type=="ControlType.Edit"&&group.Value.All(n=>n.type=="ControlType.Text");
                bool icon=rendered&&parent.type=="ControlType.Button"&&parent.name=="复制"&&group.Value.Length==1&&group.Value[0].type=="ControlType.Image"&&group.Value[0].name=="";
                bool list=rendered&&lists&&parent.type=="ControlType.List"&&group.Value.All(n=>n.type=="ControlType.ListItem");
                bool item=rendered&&lists&&parent.type=="ControlType.ListItem"&&group.Value.All(n=>n.type=="ControlType.Text"||n.type=="ControlType.Edit");
                if(!container&&!code&&!icon&&!list&&!item)throw new InvalidOperationException("trial_history_unavailable");
            }
            for(int i=0;i<group.Value.Length;i++)if(group.Value[i].id!=group.Key+"."+i)throw new InvalidOperationException("trial_history_unavailable");
        }
        if(nodes.Any(n=>n.id.Substring(body.Length+1).Split('.').Length>16||
            (n.type!="ControlType.Custom"&&n.type!="ControlType.Text"&&n.type!="ControlType.Button"&&!(rendered&&(n.type=="ControlType.Edit"||n.type=="ControlType.Image"||lists&&(n.type=="ControlType.List"||n.type=="ControlType.ListItem"))))))throw new InvalidOperationException("trial_history_unavailable");
        foreach(var n in nodes.Where(n=>n.type=="ControlType.List"||n.type=="ControlType.ListItem")) {
            WorkBuddyNode[] contents;WorkBuddyNode parent;
            if(!children.TryGetValue(n.id,out contents)||contents.Length==0)throw new InvalidOperationException("trial_history_unavailable");
            if(n.type=="ControlType.ListItem"&&(!byId.TryGetValue(n.id.Substring(0,n.id.LastIndexOf('.')),out parent)||parent.type!="ControlType.List"))throw new InvalidOperationException("trial_history_unavailable");
        }
        var direct=children[body];
        bool plain=nodes.Length==direct.Length&&direct.All(n=>n.type=="ControlType.Text");
        if(plain)return String.Concat(direct.Select(n=>n.name)).Trim('\ufeff','\r','\n',' ');
        // Code/tool cards are evidence, not terminal narrative. Only direct text
        // after the final card may carry the requested completion marker.
        int start=direct.Length;
        while(start>0&&direct[start-1].type=="ControlType.Text")start--;
        string tail=String.Concat(direct.Skip(start).Select(n=>n.name)).Trim('\ufeff','\r','\n',' ');
        if(rendered) {
            // Real 5.7.7 Markdown uses sibling paragraphs and an Edit code block.
            // Only the final paragraph with direct Text leaves is terminal prose.
            WorkBuddyNode[] paragraph;
            if(direct.Last().type!="ControlType.Custom"||!children.TryGetValue(direct.Last().id,out paragraph)||paragraph.Any(n=>n.type!="ControlType.Text"))throw new InvalidOperationException("trial_history_unavailable");
            tail=String.Concat(paragraph.Select(n=>n.name)).Trim('\ufeff','\r','\n',' ');
        }
        if(String.IsNullOrWhiteSpace(tail))throw new InvalidOperationException("trial_history_unavailable");
        // Use relative paths so moving/clipping earlier turns preserves evidence.
        // Preserve legacy plain-text bytes; never drop nested output from hashes.
        string evidence=json.Serialize(nodes.Select(n=>new{id=n.id.Substring(body.Length),type=n.type,name=n.name}).ToArray());
        if(evidence.Length>1024*1024)throw new InvalidOperationException("trial_history_unavailable");
        richBodyDigest=Hash(evidence);
        return tail;
    }
    static string WorkBuddyRenderedReply(string root,WorkBuddyNode[] all,out string digest,bool modern=false) {
        var direct=all.Where(n=>n.id.StartsWith(root+".",StringComparison.Ordinal)&&!n.id.Substring(root.Length+1).Contains('.')).ToArray();
        for(int i=0;i<direct.Length;i++)if(direct[i].id!=root+"."+i)throw new InvalidOperationException("trial_history_unavailable");
        string[] actions={"复制","点赞","点踩","朗读","点击将重新生成内容，此会话后的内容都将被覆盖，请谨慎操作","分享"};
        if(modern)actions[4]="点击将重新生成会话和产物，请谨慎操作";
        // 5.8.3 removes regenerate from historical replies. Match the entire
        // observed action profile; never infer a footer from a lone copy button.
        var profiles=new List<string[]>{actions};
        if(modern)profiles.Add(new[]{"复制","点赞","点踩","朗读","分享"});
        var matches=(from profile in profiles from i in Enumerable.Range(3,Math.Max(0,direct.Length-3))
            where i+profile.Length<=direct.Length&&profile.Select((a,j)=>direct[i+j].type=="ControlType.Button"&&direct[i+j].name==a).All(v=>v)
            select new{index=i,profile=profile}).ToArray();
        var footers=matches.Select(m=>m.index).ToArray();
        if(footers.Length!=1||footers[0]<=4)throw new InvalidOperationException("trial_history_unavailable");
        actions=matches[0].profile;
        int end=footers[0];var blocks=new List<WorkBuddyNode>();
        // Do not silently ignore another content block after the action bar.
        // Existing expanded footer retains its original bytes/digest. The new
        // compact footer is allowed only with all six actions and an HH:mm tail.
        int count=actions.Length,extra=modern&&direct.Length==end+count+6?2:0;
        int moreIndex=end+count,footerStart=moreIndex+1;
        if(modern&&count==5&&extra!=2)throw new InvalidOperationException("trial_history_unavailable");
        if(direct.Length!=end+count+4+extra||direct[moreIndex].type!="ControlType.Custom"||direct[footerStart+extra].type!="ControlType.Image"||direct[footerStart+extra].name!=""||direct[footerStart+extra+1].type!="ControlType.Text"||String.IsNullOrWhiteSpace(direct[footerStart+extra+1].name)||direct[footerStart+extra+2].type!="ControlType.Text"||(extra==2?String.IsNullOrWhiteSpace(direct[footerStart+extra+2].name):!Regex.IsMatch(direct[footerStart+extra+2].name,@"^[0-2][0-9]:[0-5][0-9]$")))throw new InvalidOperationException("trial_history_unavailable");
        if(extra==2&&Enumerable.Range(footerStart,2).Any(i=>direct[i].type!="ControlType.Text"||String.IsNullOrWhiteSpace(direct[i].name)))throw new InvalidOperationException("trial_history_unavailable");
        for(int i=0;i<actions.Length;i++) {
            string prefix=direct[end+i].id;var icons=all.Where(n=>n.id.StartsWith(prefix+".",StringComparison.Ordinal)).ToArray();
            if(icons.Length!=1||icons[0].id!=prefix+".0"||icons[0].type!="ControlType.Image"||icons[0].name!="")throw new InvalidOperationException("trial_history_unavailable");
        }
        var more=all.Where(n=>n.id.StartsWith(direct[moreIndex].id+".",StringComparison.Ordinal)).ToArray();
        string[] moreTypes={"Custom","Custom","Button","Image"};
        for(int i=0;i<moreTypes.Length;i++)if(more.Length!=moreTypes.Length||more[i].id!=direct[moreIndex].id+String.Concat(Enumerable.Repeat(".0",i+1))||more[i].type!="ControlType."+moreTypes[i]||more[i].name!=(i==2?"更多操作":""))throw new InvalidOperationException("trial_history_unavailable");
        if(all.Any(n=>Enumerable.Range(footerStart,3+extra).Any(i=>n.id.StartsWith(direct[i].id+".",StringComparison.Ordinal))))throw new InvalidOperationException("trial_history_unavailable");
        for(int i=3;i<end;i++) {
            string prefix=direct[i].id;
            foreach(var n in all.Where(n=>n.id==prefix||n.id.StartsWith(prefix+".",StringComparison.Ordinal)))
                blocks.Add(new WorkBuddyNode{id=root+".body."+(i-3)+n.id.Substring(prefix.Length),type=n.type,name=n.name});
        }
        var reply=WorkBuddyReply(root+".body",blocks.ToArray(),out digest,true,modern);
        // New footer fields are not completion prose. Bind them to evidence
        // instead of silently discarding potentially meaningful extra text.
        // Legacy 5.7.7 hashes and serialization remain byte-for-byte unchanged.
        // 5.8.3 removes regenerate when a reply becomes historical. Canonicalize
        // only that validated UI shift to the original six-action coordinates;
        // footer values/types and all body content remain bound to the hash.
        if(modern)digest=Hash(json.Serialize(new {bodyDigest=digest,footer=direct.Skip(footerStart).Select((n,i)=>new{id="."+(end+7+i).ToString(System.Globalization.CultureInfo.InvariantCulture),type=n.type,name=n.name}).ToArray()}));
        return reply;
    }
    static Round[] WorkBuddyRounds(string document,WorkBuddyNode[] nodes,string version="5.7.7.0") {
        if(version!="5.7.7.0"&&version!="5.8.3.0")throw new InvalidOperationException("trial_history_unavailable");
        bool modern=version=="5.8.3.0";
        if(String.IsNullOrEmpty(document)||nodes==null||nodes.Length==0||nodes.Length>1000||nodes.Any(n=>n==null||n.id==null||n.type==null||n.name==null||n.name.Length>65536)||nodes.Select(n=>n.id).Distinct().Count()!=nodes.Length)throw new InvalidOperationException("trial_history_unavailable");
        if(nodes.Sum(n=>n.name.Length)>1024*1024)throw new InvalidOperationException("trial_history_unavailable");
        var byId=nodes.ToDictionary(n=>n.id);
        Func<string,string,WorkBuddyNode> require=(id,type)=>{WorkBuddyNode n;if(!byId.TryGetValue(id,out n)||n.type!="ControlType."+type)throw new InvalidOperationException("trial_history_unavailable");return n;};
        require(document,"Document");
        string panel=document+".4.0.1.0.0.0.1";
        if(String.IsNullOrWhiteSpace(require(panel+".0.0.0","Text").name))throw new InvalidOperationException("trial_history_unavailable");
        string list=WorkBuddyListPath(document,version);require(list,"Custom");
        Func<string,WorkBuddyNode[]> children=id=>nodes.Where(n=>n.id.StartsWith(id+".",StringComparison.Ordinal)&&!n.id.Substring(id.Length+1).Contains('.')).ToArray();
        Func<string,WorkBuddyNode[]> descendants=id=>nodes.Where(n=>n.id.StartsWith(id+".",StringComparison.Ordinal)).ToArray();
        var turns=children(list);
        if(turns.Length<4||turns.Length>257||turns.Any(n=>n.type!="ControlType.Custom"))throw new InvalidOperationException("trial_history_unavailable");
        int lastIndex=-1;
        foreach(var t in turns){int index;if(!Int32.TryParse(t.id.Substring(list.Length+1),out index)||index<0||(lastIndex>=0&&index!=lastIndex+1))throw new InvalidOperationException("trial_history_unavailable");lastIndex=index;}
        Func<WorkBuddyNode,bool> assistant=t=>{WorkBuddyNode role;return byId.TryGetValue(t.id+".0.1",out role)&&role.type=="ControlType.Text"&&role.name=="WorkBuddy";};
        int first=assistant(turns[0])?1:0; // Only one clipped leading assistant may be discarded.
        if((turns.Length-first)%2!=0)throw new InvalidOperationException("trial_history_unavailable");
        var result=new List<Round>();int total=0;
        for(int i=first;i<turns.Length;i+=2) {
            var user=turns[i];var reply=turns[i+1];
            if(assistant(user)||!assistant(reply))throw new InvalidOperationException("trial_history_unavailable");
            string userBody=user.id+".0.0.0";require(userBody,"Custom");
            var userNodes=descendants(userBody);
            if(userNodes.Any(n=>n.type!="ControlType.Custom"&&n.type!="ControlType.Text"))throw new InvalidOperationException("trial_history_unavailable");
            var prompt=userNodes.Where(n=>n.type=="ControlType.Text"&&!String.IsNullOrWhiteSpace(n.name)).ToArray();
            if(prompt.Length!=1)throw new InvalidOperationException("trial_history_unavailable");
            string statusRoot=reply.id+".0.2";WorkBuddyNode stateRoot;
            if(!byId.TryGetValue(statusRoot,out stateRoot)||(stateRoot.type!="ControlType.Custom"&&stateRoot.type!="ControlType.Text"))throw new InvalidOperationException("trial_history_unavailable");
            var states=nodes.Where(n=>(n.id==statusRoot||n.id.StartsWith(statusRoot+".",StringComparison.Ordinal))&&n.type=="ControlType.Text"&&!String.IsNullOrWhiteSpace(n.name)).ToArray();
            if(states.Length!=1)throw new InvalidOperationException("trial_history_unavailable");
            string state=states[0].name;
            bool completed=Regex.IsMatch(state,@"^已完成\s+(?:[0-9]+m)?[0-9]+(?:\.[0-9]+)?s$");
            bool cancelled=Regex.IsMatch(state,@"^(已取消|已停止|已中断|任务已停止|任务已取消)(?:\s.*)?$");
            if(!completed&&!cancelled)throw new InvalidOperationException("trial_history_unavailable");
            string replyBody=reply.id+".0.3";require(replyBody,"Custom");
            // Short legacy replies keep their original bytes and hashes. Multiple
            // Markdown blocks occupy siblings of .3 before the reply action bar.
            WorkBuddyNode next;bool rendered=byId.TryGetValue(reply.id+".0.4",out next)&&next.type!="ControlType.Button";
            string richBodyDigest;string text=rendered?WorkBuddyRenderedReply(reply.id+".0",nodes,out richBodyDigest,modern):WorkBuddyReply(replyBody,nodes,out richBodyDigest);
            if(String.IsNullOrWhiteSpace(text))throw new InvalidOperationException("trial_history_unavailable");
            total+=prompt[0].name.Length+text.Length;if(total>1024*1024)throw new InvalidOperationException("trial_history_unavailable");
            result.Add(new Round{user=new[]{prompt[0].name},reply=text,completed=completed,cancelled=cancelled,richBodyDigest=richBodyDigest});
        }
        if(result.Count<2||result.Count>128)throw new InvalidOperationException("trial_history_unavailable");
        return result.ToArray();
    }
    static void Review(TrialReview review) {
        if(review==null||review.observation==null||review.marker==null||!Regex.IsMatch(review.marker,@"^CL_DESKTOP_[0-9a-f]{32}$")||review.prompt!=Prompt(review.marker)||review.baseline==null||review.baseline.Length<2||review.baseline.Length>128||review.baseline.Any(s=>s==null||!Regex.IsMatch(s,@"^[0-9a-f]{64}$")))throw new InvalidOperationException("trial_review_invalid");
        Enabled(review.observation.window);
    }
    static bool SameView(Observation a,Observation b) {
        return a.modelLabel==b.modelLabel&&Same(a.window,b.window)&&a.documentRuntimeId==b.documentRuntimeId&&a.sessionId==b.sessionId&&a.contentDigest==b.contentDigest&&a.layoutDigest==b.layoutDigest&&a.nodeCount==b.nodeCount&&a.draftEmpty==b.draftEmpty&&a.busy==b.busy;
    }
    static void Continuity(View view,TrialReview review) {
        if(view.observation.documentRuntimeId!=review.observation.documentRuntimeId||view.observation.sessionId!=review.observation.sessionId)throw new InvalidOperationException("trial_view_changed");
    }
    static void Foreground(IntPtr hwnd,Window expected) {
        if(GetForegroundWindow()!=hwnd||!IsWindowVisible(hwnd)||IsIconic(hwnd)||!Same(Identity(hwnd),expected))throw new InvalidOperationException("trial_focus_changed");
    }
    static object SendTrial(TrialReview review) {return SendMessage(review,false);}
    static void ReviewTask(TaskReview review,bool allowWorkBuddy=false) {
        if(review==null||review.observation==null||review.marker==null||!Regex.IsMatch(review.marker,@"^CL_TASK_[0-9a-f]{32}$")||review.prompt==null||review.prompt.Length>16000||review.prompt.Any(Char.IsControl)||Hash(review.prompt)!=review.promptSha256||!review.prompt.Contains(review.marker)||review.baseline==null||review.baseline.Length<2||review.baseline.Length>128||review.baseline.Any(s=>s==null||!Regex.IsMatch(s,@"^[0-9a-f]{64}$"))||review.maxWaitSeconds<1||review.maxWaitSeconds>50||String.IsNullOrWhiteSpace(review.observation.modelLabel))throw new InvalidOperationException("trial_review_invalid");
        if(allowWorkBuddy&&WorkBuddyReadable(review.observation.window)) {
            if(review.observation.sessionId!=null||String.IsNullOrWhiteSpace(review.observation.documentRuntimeId)||review.bindingId==null||!Regex.IsMatch(review.bindingId,@"^[0-9a-f]{64}$")||review.id==null||!Regex.IsMatch(review.id,@"^[0-9a-f]{64}$"))throw new InvalidOperationException("trial_review_invalid");
        } else Enabled(review.observation.window);
    }
    static Round[] TaskRounds(View view) {
        return view.observation.window.app=="workbuddy"?WorkBuddyRounds(view.observation.documentRuntimeId,view.nodes.Select(n=>new WorkBuddyNode{id=Runtime(n),type=n.Cached.ControlType.ProgrammaticName,name=n.Cached.Name??""}).ToArray(),view.observation.window.version):Rounds(view);
    }
    static AutomationElement WorkBuddySendButton(View view,bool hasDraft) {
        string toolbar=WorkBuddyToolbar(view,"trial_send_unavailable");
        var buttons=view.nodes.Where(n=>Runtime(n).StartsWith(toolbar+".",StringComparison.Ordinal)&&!Runtime(n).Substring(toolbar.Length+1).Contains('.')&&n.Cached.ControlType==ControlType.Button&&(n.Cached.Name=="发送"||n.Cached.Name=="停止")&&!n.Cached.IsOffscreen).ToArray();
        if(buttons.Length!=1||buttons[0].Cached.Name!="发送"||buttons[0].Cached.IsEnabled!=hasDraft)throw new InvalidOperationException("trial_send_unavailable");
        return buttons[0];
    }
    static void ClickWorkBuddySend(TaskReview review,AutomationElement original) {
        // This is the primary WorkBuddy send action, never a fallback after
        // Invoke or a retry after uncertainty. The caller owns one-shot intent.
        var hwnd=new IntPtr(Int64.Parse(review.observation.window.hwnd));
        var view=ReadView(review.observation.window,false);Continuity(view,review);
        if(view.draft!=review.prompt||view.observation.busy||view.observation.modelLabel!=review.observation.modelLabel||!Baseline(TaskRounds(view)).SequenceEqual(review.baseline))throw new InvalidOperationException("trial_draft_changed");
        var button=WorkBuddySendButton(view,true);
        if(Runtime(button)!=Runtime(original))throw new InvalidOperationException("trial_send_unavailable");
        var rect=button.Current.BoundingRectangle;
        if(rect.IsEmpty||rect.Width<10||rect.Height<10)throw new InvalidOperationException("trial_send_unavailable");
        var point=new FocusPoint{x=(int)(rect.Left+rect.Width/2),y=(int)(rect.Top+rect.Height/2)};
        if(new[]{0x10,0x11,0x12,0x5B,0x5C}.Any(key=>(GetAsyncKeyState(key)&0x8000)!=0))throw new InvalidOperationException("trial_focus_changed");
        Foreground(hwnd,review.observation.window);
        if(ComposerAncestor(ComposerWindowFromPoint(point),2)!=hwnd)throw new InvalidOperationException("trial_focus_changed");
        var hit=AutomationElement.FromPoint(new System.Windows.Point(point.x,point.y));
        bool exact=false;
        for(int i=0;i<6&&hit!=null;i++) {
            if(Runtime(hit)==Runtime(button)){exact=true;break;}
            if(hit.Current.ProcessId!=review.observation.window.pid)break;
            hit=TreeWalker.RawViewWalker.GetParent(hit);
        }
        if(!exact||!ComposerCursor(point.x,point.y))throw new InvalidOperationException("trial_focus_changed");
        Foreground(hwnd,review.observation.window);
        if(ComposerAncestor(ComposerWindowFromPoint(point),2)!=hwnd||!button.Current.IsEnabled||button.Current.IsOffscreen||button.Current.BoundingRectangle!=rect)throw new InvalidOperationException("trial_focus_changed");
        object value;if(!view.editor.TryGetCurrentPattern(ValuePattern.Pattern,out value)||((ValuePattern)value).Current.Value.Trim('\ufeff','\r','\n')!=review.prompt)throw new InvalidOperationException("trial_draft_changed");
        SendStage("send-click-attempt");
        var click=new[]{new Input{type=0,data=new InputData{mouse=new MouseInput{flags=2}}},new Input{type=0,data=new InputData{mouse=new MouseInput{flags=4}}}};
        if(SendInput(2,click,Marshal.SizeOf(typeof(Input)))!=2)throw new InvalidOperationException("trial_send_unavailable");
    }
    static object WaitSentReply(TrialReview review,bool task) {
        SendStage("read-reply");
        // Monotonic elapsed time includes UIA reads. An individual blocked COM
        // call is still bounded by the outer owned process timeout.
        var watch=System.Diagnostics.Stopwatch.StartNew();
        long budget=(task?((TaskReview)review).maxWaitSeconds:50)*1000L;
        while(watch.ElapsedMilliseconds<budget) {
            Thread.Sleep((int)Math.Max(1,Math.Min(500,budget-watch.ElapsedMilliseconds)));
            if(watch.ElapsedMilliseconds>=budget)break;
            try {return task?ReadTask((TaskReview)review):ReadTrial(review);}
            catch(ElementNotAvailableException){}
            catch(COMException){}
            catch(InvalidOperationException e){if(e.Message!="trial_reply_unverified"&&e.Message!="trial_history_unavailable")throw;}
        }
        throw new InvalidOperationException("trial_reply_unverified");
    }
    // Waiting only: callers supply read-only samples. Never types or retries a send.
    static T WaitForDraft<T>(Func<T> read,Func<T,bool> ready,int timeoutMs,int intervalMs) where T:class {
        if(read==null||ready==null||timeoutMs<=0||intervalMs<=0)throw new InvalidOperationException("draft_wait_invalid");
        var timer=System.Diagnostics.Stopwatch.StartNew();T last=null;
        while(timer.ElapsedMilliseconds<timeoutMs) {
            last=read();
            if(ready(last))return last;
            int pause=(int)Math.Min(intervalMs,timeoutMs-timer.ElapsedMilliseconds);
            if(pause>0)Thread.Sleep(pause);
        }
        return last;
    }
    static object SendMessage(TrialReview review,bool task) {
        SendStage("review");
        if(task)ReviewTask((TaskReview)review,true);else Review(review);
        RequireDesktopActionVersion(review.observation.window);
        bool buddy=review.observation.window.app=="workbuddy";
        var age=DateTime.UtcNow-DateTime.Parse(review.observation.capturedAt,null,System.Globalization.DateTimeStyles.RoundtripKind).ToUniversalTime();
        if(age.TotalSeconds<0||age.TotalSeconds>120)throw new InvalidOperationException("trial_expired");
        using(var mutex=new Mutex(false,@"Local\CeruleanDesktopTrialActionV1")) {
            bool acquired=false;
            try {
                try{acquired=mutex.WaitOne(0);}catch(AbandonedMutexException){acquired=true;}
                if(!acquired)throw new InvalidOperationException("trial_lease_busy");
                SendStage("observe-before-input");var view=ReadView(review.observation.window,true);
                if(!SameView(view.observation,review.observation))throw new InvalidOperationException("trial_view_changed");
                if(!Baseline(TaskRounds(view)).SequenceEqual(review.baseline)||view.nodes.Any(n=>(n.Cached.Name??"").Contains(review.marker)))throw new InvalidOperationException("trial_history_changed");
                if(buddy)WorkBuddySendButton(view,false);
                var hwnd=new IntPtr(Int64.Parse(review.observation.window.hwnd));
                SetForegroundWindow(hwnd);Thread.Sleep(100);Foreground(hwnd,review.observation.window);
                // This editor exposes ValuePattern but SetValue does not update
                // its React composer. Type the exact reviewed Unicode burst only
                // after verifying exact foreground + UIA editor focus. No Enter,
                // shortcuts, clipboard, unreviewed text or retry. WorkBuddy may
                // require a guarded composer click before the exact focus check.
                SendStage("focus-composer");FocusComposer(view,hwnd);
                SendStage("verify-focus");
                view=ReadView(review.observation.window,true);
                if(!SameView(view.observation,review.observation))throw new InvalidOperationException("trial_view_changed");
                if(buddy)WorkBuddySendButton(view,false);
                Foreground(hwnd,review.observation.window);
                object value;if(!view.editor.TryGetCurrentPattern(ValuePattern.Pattern,out value)||((ValuePattern)value).Current.IsReadOnly)throw new InvalidOperationException("trial_draft_changed");
                if(!ComposerHasFocus(view)||new[]{0x10,0x11,0x12,0x5B,0x5C}.Any(key=>(GetAsyncKeyState(key)&0x8000)!=0))throw new InvalidOperationException("trial_focus_changed");
                if(review.prompt.Any(Char.IsControl))throw new InvalidOperationException("trial_review_invalid");
                var inputs=new List<Input>();
                foreach(char c in review.prompt){inputs.Add(new Input{type=1,data=new InputData{keyboard=new KeyboardInput{scan=c,flags=4}}});inputs.Add(new Input{type=1,data=new InputData{keyboard=new KeyboardInput{scan=c,flags=6}}});}
                Foreground(hwnd,review.observation.window);
                SendStage("keyboard-attempt");
                if(SendInput((uint)inputs.Count,inputs.ToArray(),Marshal.SizeOf(typeof(Input)))!=inputs.Count)throw new InvalidOperationException("trial_keyboard_unavailable");
                SendStage("verify-draft");
                View draft=null;AutomationElement send=null;
                if(buddy) {
                    for(int i=0;i<10;i++) {
                        Thread.Sleep(100);draft=ReadView(review.observation.window,false);Continuity(draft,review);Foreground(hwnd,review.observation.window);
                        if(draft.draft==review.prompt){send=WorkBuddySendButton(draft,true);break;}
                    }
                } else {
                    // A single Unicode burst may take React longer than one second
                    // to expose all characters through UIA (observed in 228 and 231).
                    draft=WaitForDraft(()=>{
                        var v=ReadView(review.observation.window,false);
                        Continuity(v,review);Foreground(hwnd,review.observation.window);return v;
                    },v=>{
                        var buttons=v.nodes.Where(n=>n.Cached.ControlType==ControlType.Button&&n.Cached.Name=="发送消息"&&!n.Cached.IsOffscreen&&n.Cached.IsEnabled).ToArray();
                        if(v.draft!=review.prompt||buttons.Length!=1)return false;
                        send=buttons[0];return true;
                    },8000,100);
                }
                if(draft==null||draft.draft!=review.prompt||draft.observation.busy||(task&&draft.observation.modelLabel!=review.observation.modelLabel)||!Baseline(TaskRounds(draft)).SequenceEqual(review.baseline))throw new InvalidOperationException("trial_draft_changed");
                object invoke=null;if(send==null||(!buddy&&!send.TryGetCurrentPattern(InvokePattern.Pattern,out invoke)))throw new InvalidOperationException("trial_send_unavailable");
                Foreground(hwnd,review.observation.window);
                // Re-read the live editor immediately before the only Invoke. No retry path.
                object current;if(!draft.editor.TryGetCurrentPattern(ValuePattern.Pattern,out current))throw new InvalidOperationException("trial_draft_changed");
                var text=((ValuePattern)current).Current.Value;
                if(buddy)text=text.Trim('\ufeff','\r','\n');else if(text.EndsWith("\n"))text=text.Substring(0,text.Length-1);
                if(text!=review.prompt)throw new InvalidOperationException("trial_draft_changed");
                if(buddy)ClickWorkBuddySend((TaskReview)review,send);
                else {SendStage("invoke-attempt");((InvokePattern)invoke).Invoke();}
                return WaitSentReply(review,task);
            } finally {if(acquired)mutex.ReleaseMutex();}
        }
    }
    static object ReadTask(TaskReview review) {
        ReviewTask(review,true);var view=ReadView(review.observation.window,false);
        var rounds=TaskRounds(view);
        return TaskResult(review,view,rounds);
    }
    static object TaskResult(TaskReview review,View view,Round[] rounds) {
        Continuity(view,review);
        if(!Same(view.observation.window,review.observation.window))throw new InvalidOperationException("trial_view_changed");
        if(view.observation.modelLabel!=review.observation.modelLabel)throw new InvalidOperationException("trial_view_changed");
        var retained=Baseline(rounds.Take(Math.Max(0,rounds.Length-1)).ToArray());
        if(retained.Length<2||retained.Length>review.baseline.Length||!retained.SequenceEqual(review.baseline.Skip(review.baseline.Length-retained.Length)))throw new InvalidOperationException("trial_reply_unverified");
        var last=rounds.Last();
        if(!last.completed||last.cancelled||!last.user.Contains(review.prompt)||!last.reply.EndsWith(review.marker,StringComparison.Ordinal)||!view.observation.draftEmpty||view.observation.busy)throw new InvalidOperationException("trial_reply_unverified");
        if(review.observation.window.app=="workbuddy") {
            ReviewTask(review,true);
            if(last.user.Length!=1||rounds.Count(r=>r.user.Contains(review.prompt))!=1)throw new InvalidOperationException("trial_reply_unverified");
            string replyDigest=Hash(last.richBodyDigest==null?last.reply:json.Serialize(new{reply=last.reply,richBodyDigest=last.richBodyDigest}));
            return new{taskProof=new{marker=review.marker,sessionId=(string)null,localBinding=new{bindingId=review.bindingId,taskReviewDigest=review.id,observation=view.observation},promptSha256=review.promptSha256,observedBaseline=retained,replySha256=replyDigest,observedAt=view.observation.capturedAt,completed=true,roundSha256=Baseline(new[]{last})[0]}};
        }
        return new{taskProof=new{marker=review.marker,sessionId=view.observation.sessionId,promptSha256=review.promptSha256,observedBaseline=retained,replySha256=Hash(last.reply),observedAt=view.observation.capturedAt,completed=true,roundSha256=Baseline(new[]{last})[0]}};
    }
    static object ReadTrial(TrialReview review) {
        Review(review);var view=ReadView(review.observation.window,false);Continuity(view,review);var rounds=Rounds(view);
        var retained=Baseline(rounds.Take(Math.Max(0,rounds.Length-1)).ToArray());
        if(retained.Length<2||retained.Length>review.baseline.Length||!retained.SequenceEqual(review.baseline.Skip(review.baseline.Length-retained.Length)))throw new InvalidOperationException("trial_reply_unverified");
        var last=rounds.Last();
        if(!last.completed||last.cancelled||!last.user.Contains(review.prompt)||last.reply!=review.marker||!view.observation.draftEmpty||view.observation.busy)throw new InvalidOperationException("trial_reply_unverified");
        return new{trialProof=new{marker=review.marker,sessionId=view.observation.sessionId,baseline=review.baseline,observedBaseline=retained,promptSeen=true,completed=true,replyMatches=true,observedAt=view.observation.capturedAt}};
    }
}
