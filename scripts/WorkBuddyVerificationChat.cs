using System;
using System.Linq;
using System.Runtime.InteropServices;
using System.Threading;
using System.Windows.Automation;

// Diagnostic navigation only. The caller must explicitly select this mode.
// One guarded click opens the existing authorized chat; no typing or send.
public static partial class WorkBuddyLayoutMetadata {
    static string CachedId(AutomationElement e) {return String.Join(".",(int[])e.GetCachedPropertyValue(AutomationElement.RuntimeIdProperty));}
    static AutomationElement[] Children(AutomationElement e) {return e.FindAll(TreeScope.Children,Automation.ControlViewCondition).Cast<AutomationElement>().ToArray();}
    static AutomationElement AtPath(AutomationElement root,string target) {
        var current=root;
        for(int i=0;i<24;i++) {
            if(Id(current)==target)return current;
            var next=Children(current).Where(n=>CachedId(n)==target || target.StartsWith(CachedId(n)+".",StringComparison.Ordinal)).ToArray();
            if(next.Length!=1)throw new InvalidOperationException("structure_path_unavailable");
            current=next[0];
        }
        throw new InvalidOperationException("structure_depth_limit");
    }
    static object InspectChatStructure(IntPtr hwnd,int pid) {
        using(var process=System.Diagnostics.Process.GetProcessById(pid)) {
            int owner;GetWindowThreadProcessId(hwnd,out owner);
            var started=process.StartTime.ToUniversalTime().Ticks;
            if(owner!=pid || !String.Equals(process.MainModule.FileName,ExpectedPath,StringComparison.OrdinalIgnoreCase) ||
                process.MainModule.FileVersionInfo.ProductVersion!=ExpectedVersion || IsIconic(hwnd))throw new InvalidOperationException("structure_identity_changed");
            var cache=new CacheRequest{TreeScope=TreeScope.Element};
            foreach(var property in new[]{AutomationElement.RuntimeIdProperty,AutomationElement.ControlTypeProperty,
                AutomationElement.IsEnabledProperty,AutomationElement.IsOffscreenProperty,AutomationElement.IsPasswordProperty})cache.Add(property);
            using(cache.Activate()) {
                var root=AutomationElement.FromHandle(hwnd);
                var documents=root.FindAll(TreeScope.Children,new PropertyCondition(AutomationElement.ControlTypeProperty,ControlType.Document));
                if(documents.Count!=1)throw new InvalidOperationException("document_not_unique");
                var document=documents[0];var docId=Id(document);
                Console.Error.WriteLine("structure-stage=resolve-content");
                var panel=AtPath(document,docId+".4.0.1.0.0.0.1");
                Console.Error.WriteLine("structure-stage=confirm-authorized-label");
                var labels=panel.FindAll(TreeScope.Descendants,new AndCondition(
                    new PropertyCondition(AutomationElement.ControlTypeProperty,ControlType.Text),
                    new PropertyCondition(AutomationElement.NameProperty,"工作台接入验证"))).Cast<AutomationElement>().ToArray();
                if(labels.Length!=1 || labels[0].Cached.IsOffscreen)throw new InvalidOperationException("authorized_content_label_unconfirmed");
                var rows=new System.Collections.Generic.List<object>();
                var queue=new System.Collections.Generic.Queue<Tuple<AutomationElement,int>>();queue.Enqueue(Tuple.Create(panel,0));
                while(queue.Count>0) {
                    var item=queue.Dequeue();var e=item.Item1;var p=e.Cached;
                    if(rows.Count>=180)throw new InvalidOperationException("structure_node_limit");
                    rows.Add(new {id=CachedId(e),type=p.ControlType.ProgrammaticName,depth=item.Item2,offscreen=p.IsOffscreen,enabled=p.IsEnabled});
                    if(item.Item2<4)foreach(var child in Children(e))queue.Enqueue(Tuple.Create(child,item.Item2+1));
                }
                Console.Error.WriteLine("structure-stage=composer-branches");
                var controls=new System.Collections.Generic.List<object>();
                var body=AtPath(panel,docId+".4.0.1.0.0.0.1.1.0.0");
                foreach(var branch in Children(body).Where(n=>CachedId(n)!=CachedId(body)+".0")) {
                    var selection=branch.FindAll(TreeScope.Subtree,new OrCondition(
                        new PropertyCondition(AutomationElement.ControlTypeProperty,ControlType.Edit),
                        new PropertyCondition(AutomationElement.ControlTypeProperty,ControlType.ComboBox),
                        new PropertyCondition(AutomationElement.ControlTypeProperty,ControlType.Button)));
                    if(selection.Count>40)throw new InvalidOperationException("composer_metadata_limit");
                    foreach(AutomationElement n in selection)controls.Add(new {id=CachedId(n),type=n.Cached.ControlType.ProgrammaticName,
                        enabled=n.Cached.IsEnabled,offscreen=n.Cached.IsOffscreen,category=Category(n,n.Cached.ControlType,n.Cached.IsPassword)});
                }
                Console.Error.WriteLine("structure-stage=history-suffix");
                var list=AtPath(panel,docId+".4.0.1.0.0.0.1.1.0.0.0.0.0.0.0");
                var turns=Children(list);
                Console.Error.WriteLine("history-direct-children="+turns.Length);
                if(turns.Length<4 || turns.Length>259) {
                    var shape=new System.Collections.Generic.List<object>();
                    foreach(var n in turns.Take(12)) {
                        shape.Add(new{id=CachedId(n),type=n.Cached.ControlType.ProgrammaticName,depth=1});
                        foreach(var child in Children(n).Take(12))shape.Add(new{id=CachedId(child),type=child.Cached.ControlType.ProgrammaticName,depth=2});
                    }
                    return new {status="authorized-chat-layout-diff",pid=pid,version=ExpectedVersion,document=docId,
                        contentLabel=CachedId(labels[0]),nodes=rows,composerControls=controls,
                        history=new{parsed=false,error="history_turn_limit",directChildren=turns.Length,shape=shape},
                        readOnly=true,sendActions=0,focusActions=0,valueReads=0,chatTextExported=false};
                }
                var suffix=turns.Skip(Math.Max(0,turns.Length-6)).ToArray();
                var history=new System.Collections.Generic.List<AutomationElement>();
                // The installed parser receives a bounded suffix, plus only its
                // required container/header nodes. Names stay in process memory.
                var historyCache=cache.Clone();historyCache.Add(AutomationElement.NameProperty);
                using(historyCache.Activate()) {
                    foreach(var e in new[]{document,labels[0],list})history.Add(e.GetUpdatedCache(historyCache));
                    foreach(var turn in suffix) {
                        var elements=turn.FindAll(TreeScope.Subtree,Automation.ControlViewCondition).Cast<AutomationElement>().ToArray();
                        if(elements.Length+history.Count>1000)throw new InvalidOperationException("history_node_limit");
                        history.AddRange(elements);
                    }
                }
                var historyResult=DesktopObserver.InspectWorkBuddyHistorySuffix(docId,CachedId(list),history.ToArray());
                GetWindowThreadProcessId(hwnd,out owner);
                if(owner!=pid || process.HasExited || process.StartTime.ToUniversalTime().Ticks!=started)throw new InvalidOperationException("structure_identity_changed");
                return new {status="authorized-chat-structure-observed",pid=pid,version=ExpectedVersion,document=docId,
                    contentLabel=CachedId(labels[0]),nodes=rows,composerControls=controls,history=historyResult,
                    readOnly=true,sendActions=0,focusActions=0,valueReads=0,chatTextExported=false};
            }
        }
    }
    [StructLayout(LayoutKind.Sequential)] struct NavPoint {public int x,y;}
    [StructLayout(LayoutKind.Sequential)] struct MouseInput {public int x,y;public uint data,flags,time;public UIntPtr extra;}
    [StructLayout(LayoutKind.Explicit)] struct InputData {[FieldOffset(0)] public MouseInput mouse;}
    [StructLayout(LayoutKind.Sequential)] struct Input {public uint type;public InputData data;}
    [DllImport("user32.dll")] static extern bool SetForegroundWindow(IntPtr hwnd);
    [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] static extern bool SetCursorPos(int x,int y);
    [DllImport("user32.dll")] static extern IntPtr WindowFromPoint(NavPoint point);
    [DllImport("user32.dll")] static extern IntPtr GetAncestor(IntPtr hwnd,uint flags);
    [DllImport("user32.dll")] static extern uint SendInput(uint count,Input[] inputs,int size);
    class NavigationFrame {
        public AutomationElement link;
        public string identity,document,model,editor,linkId;
    }
    static NavigationFrame NavigationReady(IntPtr hwnd,int pid) {
        using(var p=System.Diagnostics.Process.GetProcessById(pid)) {
            int owner;GetWindowThreadProcessId(hwnd,out owner);
            if(owner!=pid || !IsWindowVisible(hwnd) || IsIconic(hwnd) ||
                !String.Equals(p.MainModule.FileName,ExpectedPath,StringComparison.OrdinalIgnoreCase) ||
                p.MainModule.FileVersionInfo.ProductVersion!=ExpectedVersion)
                throw new InvalidOperationException("navigation_identity_changed");
            var root=AutomationElement.FromHandle(hwnd);
            var documents=root.FindAll(TreeScope.Children,new PropertyCondition(AutomationElement.ControlTypeProperty,ControlType.Document));
            if(documents.Count!=1)throw new InvalidOperationException("document_not_unique");
            var document=documents[0];var documentId=Id(document);
            var cache=new CacheRequest{TreeScope=TreeScope.Element};
            foreach(var property in new[]{AutomationElement.RuntimeIdProperty,AutomationElement.ControlTypeProperty,
                AutomationElement.NameProperty,AutomationElement.IsEnabledProperty,AutomationElement.IsOffscreenProperty,
                AutomationElement.IsPasswordProperty})cache.Add(property);
            using(cache.Activate()) {
                var selected=document.FindAll(TreeScope.Descendants,new AndCondition(Automation.ControlViewCondition,new OrCondition(
                    new PropertyCondition(AutomationElement.ControlTypeProperty,ControlType.Edit),
                    new PropertyCondition(AutomationElement.ControlTypeProperty,ControlType.Button),
                    new PropertyCondition(AutomationElement.ControlTypeProperty,ControlType.ComboBox)))).Cast<AutomationElement>().ToArray();
                if(selected.Length>1000 || selected.Any(n=>n.Cached.IsPassword))throw new InvalidOperationException("navigation_controls_ambiguous");
                if(selected.Any(n=>n.Cached.ControlType==ControlType.Button && !n.Cached.IsOffscreen && n.Cached.IsEnabled &&
                    new[]{"stop","approval"}.Contains(Category(n,n.Cached.ControlType,false))))throw new InvalidOperationException("navigation_busy_or_approval");
                var editors=selected.Where(n=>n.Cached.ControlType==ControlType.Edit && !n.Cached.IsOffscreen).ToArray();
                string expectedEditor=documentId+".4.0.1.0.0.0.1.0.0.1.2.1.0.0.1.0";
                if(editors.Length!=1 || Id(editors[0])!=expectedEditor || !editors[0].Cached.IsEnabled)
                    throw new InvalidOperationException("navigation_home_composer_required");
                object pattern;
                if(!editors[0].TryGetCurrentPattern(ValuePattern.Pattern,out pattern))throw new InvalidOperationException("navigation_draft_unavailable");
                var draft=((ValuePattern)pattern).Current.Value;
                // Do not output even a rejected draft. Only known blank forms pass.
                var normalized=draft==null?null:draft.Trim('\ufeff','\r','\n');
                Console.Error.WriteLine("draft-classification: "+(normalized==null?"unavailable":normalized==""?"empty":normalized=="今天帮你做些什么？@ 添加上下文，/调用技能与指令"?"known-placeholder":"other-do-not-navigate"));
                if(draft==null || draft.Length>1024 || (normalized!="" && normalized!="今天帮你做些什么？@ 添加上下文，/调用技能与指令"))
                    throw new InvalidOperationException("navigation_draft_present");
                var links=selected.Where(n=>n.Cached.ControlType==ControlType.Button && !n.Cached.IsOffscreen && n.Cached.IsEnabled &&
                    Category(n,n.Cached.ControlType,false)=="verification-chat-link" &&
                    Id(n).StartsWith(documentId+".4.0.1.0.0.0.0.",StringComparison.Ordinal)).ToArray();
                var models=selected.Where(n=>n.Cached.ControlType==ControlType.ComboBox && !n.Cached.IsOffscreen && n.Cached.IsEnabled &&
                    Category(n,n.Cached.ControlType,false)=="model-selector").ToArray();
                var sends=selected.Where(n=>n.Cached.ControlType==ControlType.Button && !n.Cached.IsOffscreen && Category(n,n.Cached.ControlType,false)=="send").ToArray();
                if(links.Length!=1 || models.Length!=1 || sends.Length!=1 || sends[0].Cached.IsEnabled)
                    throw new InvalidOperationException("navigation_controls_ambiguous");
                return new NavigationFrame{link=links[0],document=documentId,linkId=Id(links[0]),editor=expectedEditor,
                    model=Id(models[0])+":"+models[0].Cached.Name,identity=pid+":"+p.StartTime.ToUniversalTime().Ticks};
            }
        }
    }
    static object OpenVerificationChat(IntPtr hwnd,int pid) {
        var before=NavigationReady(hwnd,pid);
        SetForegroundWindow(hwnd);Thread.Sleep(150);
        if(GetForegroundWindow()!=hwnd)throw new InvalidOperationException("navigation_foreground_changed");
        var current=NavigationReady(hwnd,pid);
        if(before.identity!=current.identity || before.document!=current.document || before.editor!=current.editor || before.model!=current.model || before.linkId!=current.linkId)
            throw new InvalidOperationException("navigation_view_changed");
        var rect=current.link.Current.BoundingRectangle;
        if(rect.IsEmpty || rect.Width<20 || rect.Height<10)throw new InvalidOperationException("navigation_link_unavailable");
        var point=new NavPoint{x=(int)(rect.Left+rect.Width/3),y=(int)(rect.Top+rect.Height/2)};
        if(GetAncestor(WindowFromPoint(point),2)!=hwnd || !SetCursorPos(point.x,point.y) || GetForegroundWindow()!=hwnd)
            throw new InvalidOperationException("navigation_overlay_or_focus_changed");
        var hit=AutomationElement.FromPoint(new System.Windows.Point(point.x,point.y));
        if(Id(hit)!=current.linkId && !Id(hit).StartsWith(current.linkId+".",StringComparison.Ordinal))throw new InvalidOperationException("navigation_hit_changed");
        Console.Error.WriteLine("navigation-intent: one click, no message");
        var inputs=new[]{new Input{data=new InputData{mouse=new MouseInput{flags=2}}},new Input{data=new InputData{mouse=new MouseInput{flags=4}}}};
        if(SendInput(2,inputs,Marshal.SizeOf(typeof(Input)))!=2)throw new InvalidOperationException("navigation_click_unknown");
        Thread.Sleep(800);
        return new {status="navigation-clicked-observe-result",navigationClicks=1,messagesSent=0,keyboardEvents=0,
            credentialRead=false,documentValueRead=false,draftValuesRead=true,draftTextExported=false,
            observation=Inspect(hwnd,pid)};
    }
}

// Call the unchanged production parser only after the diagnostic has confirmed
// the authorized content header. No runtime bridge operations are exposed here.
public partial class DesktopObserver {
    public static object InspectAuthorizedWorkBuddy(IntPtr hwnd,int pid,bool prepare=false) {
        var identity=Identity(hwnd);
        if(identity.pid!=pid || identity.app!="workbuddy" || identity.version!="5.8.3.0" ||
            !String.Equals(identity.exe,@"D:\workbuddy\WorkBuddy.exe",StringComparison.OrdinalIgnoreCase))throw new InvalidOperationException("window_changed");
        // Check the exact authorized content header before allowing the product
        // observer to read any historical message names.
        var cache=new CacheRequest{TreeScope=TreeScope.Element};
        foreach(var p in new[]{AutomationElement.RuntimeIdProperty,AutomationElement.ControlTypeProperty,AutomationElement.NameProperty,AutomationElement.IsOffscreenProperty})cache.Add(p);
        using(cache.Activate()) {
            var documents=AutomationElement.FromHandle(hwnd).FindAll(TreeScope.Children,new PropertyCondition(AutomationElement.ControlTypeProperty,ControlType.Document));
            if(documents.Count!=1)throw new InvalidOperationException("document_not_unique");
            var header=WorkBuddyPath(documents[0],CachedRuntime(documents[0])+".4.0.1.0.0.0.1.0.0.0");
            if(header.Cached.ControlType!=ControlType.Text || header.Cached.IsOffscreen || header.Cached.Name!="工作台接入验证")throw new InvalidOperationException("authorized_content_label_unconfirmed");
        }
        if(prepare)return new {status="production-task-prepared-read-only",native=PrepareTask(identity,null),
            messagesSent=0,focusActions=0,credentialRead=false,documentValueRead=false,draftTextExported=false};
        var view=ReadView(identity,true);
        var rounds=WorkBuddyRounds(view.observation.documentRuntimeId,view.nodes.Select(n=>new WorkBuddyNode{id=CachedRuntime(n),type=n.Cached.ControlType.ProgrammaticName,name=n.Cached.Name??""}).ToArray(),"5.8.3.0");
        return new {status="production-read-only-observed",observation=view.observation,rounds=rounds.Length,baseline=Baseline(rounds),
            messagesSent=0,focusActions=0,credentialRead=false,documentValueRead=false,draftTextExported=false};
    }
    public static object InspectWorkBuddyHistorySuffix(string document,string actualList,AutomationElement[] nodes) {
        var input=nodes.Select(n=>new WorkBuddyNode{id=String.Join(".",(int[])n.GetCachedPropertyValue(AutomationElement.RuntimeIdProperty)),
            type=n.Cached.ControlType.ProgrammaticName,name=n.Cached.Name??""}).ToArray();
        if(actualList!=document+".4.0.1.0.0.0.1.1.0.0.0.0.0.0.0")throw new InvalidOperationException("structure_path_unavailable");
        try {
            var rounds=WorkBuddyRounds(document,input,"5.8.3.0");
            return new {parsed=true,parserVersion="5.8.3.0",rounds=rounds.Length,baseline=Baseline(rounds),error=(string)null};
        }catch(InvalidOperationException e) {
            if(e.Message!="trial_history_unavailable")throw;
            // Preserve only structural categories for adapting the parser.
            return (object)new {parsed=false,error=e.Message,nodes=input.Select(n=>new {id=n.id,type=n.type,
                category=new[]{"复制","点赞","点踩","朗读","点击将重新生成内容，此会话后的内容都将被覆盖，请谨慎操作","分享","更多操作"}.Contains(n.name)?n.name:
                    System.Text.RegularExpressions.Regex.IsMatch(n.name,@"^[0-2][0-9]:[0-5][0-9]$")?"time":n.name=="WorkBuddy"?"assistant-role":n.name=="工作台接入验证"?"verification-title":
                    System.Text.RegularExpressions.Regex.IsMatch(n.name,@"^已完成\s+(?:[0-9]+m)?[0-9]+(?:\.[0-9]+)?s$")?"complete-status":
                    String.IsNullOrWhiteSpace(n.name)?"empty":"other-redacted"}).ToArray()};
        }
    }
}
