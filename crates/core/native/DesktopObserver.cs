using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Linq;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
using System.Text;
using System.Text.RegularExpressions;
using System.Web.Script.Serialization;
using System.Windows.Automation;

// Bounded UIA bridge: observations are read-only; fixed trials and approved task sends are guarded.
public partial class DesktopObserver {
    delegate bool EnumProc(IntPtr window,IntPtr arg);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc cb,IntPtr arg);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll")] static extern bool IsIconic(IntPtr window);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr window,out int pid);
    [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern int GetWindowText(IntPtr window,StringBuilder text,int length);
    static JavaScriptSerializer json=new JavaScriptSerializer{MaxJsonLength=128*1024};
    public class Window {public string app,hwnd,startTicks,exe,title,version;public uint pid;}
    public class Request {public int version;public string op;public Window window;public TrialReview review;public TaskReview taskReview;public StopObservation stopObservation;}
    public class Observation {public Window window;public string capturedAt,documentRuntimeId,sessionId,contentDigest,layoutDigest,modelLabel;public int nodeCount;public bool draftEmpty,busy;}
    class View {public Observation observation;public AutomationElement[] nodes;public AutomationElement editor;public string draft;}
    static string Hash(string text){using(var sha=SHA256.Create())return BitConverter.ToString(sha.ComputeHash(Encoding.UTF8.GetBytes(text))).Replace("-","").ToLowerInvariant();}
    static Window Identity(IntPtr hwnd) {
        int pid;GetWindowThreadProcessId(hwnd,out pid);
        using(var process=Process.GetProcessById(pid)) {
            string path=process.MainModule.FileName;
            string app=Path.GetFileNameWithoutExtension(path).ToLowerInvariant();
            if(app!="qoder"&&app!="workbuddy")throw new InvalidOperationException("app_not_allowed");
            var title=new StringBuilder(512);GetWindowText(hwnd,title,title.Capacity);
            return new Window{app=app,hwnd=hwnd.ToInt64().ToString(),pid=(uint)pid,startTicks=process.StartTime.ToUniversalTime().Ticks.ToString(),exe=path,title=title.ToString(),version=process.MainModule.FileVersionInfo.ProductVersion??"unknown"};
        }
    }
    static bool Same(Window a,Window b){return a.app==b.app&&a.hwnd==b.hwnd&&a.pid==b.pid&&a.startTicks==b.startTicks&&String.Equals(a.exe,b.exe,StringComparison.OrdinalIgnoreCase)&&a.version==b.version&&a.title==b.title;}
    static Observation Inspect(Window expected) {return ReadView(expected, true).observation;}
    static string CachedRuntime(AutomationElement element){return String.Join(".",(int[])element.GetCachedPropertyValue(AutomationElement.RuntimeIdProperty));}
    static AutomationElement WorkBuddyPath(AutomationElement root,string target) {
        var current=root;
        for(int depth=0;depth<24;depth++) {
            if(CachedRuntime(current)==target)return current;
            var next=current.FindAll(TreeScope.Children,Automation.ControlViewCondition).Cast<AutomationElement>()
                .Where(n=>CachedRuntime(n)==target||target.StartsWith(CachedRuntime(n)+".",StringComparison.Ordinal)).ToArray();
            if(next.Length!=1)throw new InvalidOperationException("trial_history_unavailable");
            current=next[0];
        }
        throw new InvalidOperationException("tree_limit");
    }
    static AutomationElement[] WorkBuddy583Nodes(AutomationElement document,CacheRequest cache) {
        // 5.8.3: avoid enumerating the sidebar and the full long conversation.
        // Keep actual runtime IDs, the content header, the last three turn pairs,
        // and the whole composer area. No values are read while locating nodes.
        string doc=CachedRuntime(document),panel=doc+".4.0.1.0.0.0.1";
        var header=WorkBuddyPath(document,panel+".0.0.0");
        if(header.Cached.ControlType!=ControlType.Text||header.Cached.IsOffscreen||String.IsNullOrWhiteSpace(header.Cached.Name))throw new InvalidOperationException("trial_history_unavailable");
        var body=WorkBuddyPath(document,panel+".1.0.0");
        var list=WorkBuddyPath(body,WorkBuddyListPath(doc,"5.8.3.0"));
        var turns=list.FindAll(TreeScope.Children,Automation.ControlViewCondition).Cast<AutomationElement>().ToArray();
        if(turns.Length<4||turns.Length>259||turns.Any(n=>n.Cached.ControlType!=ControlType.Custom))throw new InvalidOperationException("trial_history_unavailable");
        var nodes=new List<AutomationElement>{document.GetUpdatedCache(cache),header,list};
        var roots=turns.Skip(Math.Max(0,turns.Length-6)).Concat(body.FindAll(TreeScope.Children,Automation.ControlViewCondition)
            .Cast<AutomationElement>().Where(n=>CachedRuntime(n)!=CachedRuntime(body)+".0")).ToArray();
        foreach(var root in roots) {
            nodes.AddRange(root.FindAll(TreeScope.Subtree,Automation.ControlViewCondition).Cast<AutomationElement>());
            if(nodes.Count>1000)throw new InvalidOperationException("tree_limit");
        }
        if(nodes.Select(CachedRuntime).Distinct().Count()!=nodes.Count)throw new InvalidOperationException("trial_history_unavailable");
        return nodes.ToArray();
    }
    static View ReadView(Window expected,bool idle) {
        if(expected==null)throw new InvalidOperationException("window_required");
        long number;if(!Int64.TryParse(expected.hwnd,out number)||number<=0)throw new InvalidOperationException("invalid_window");
        var hwnd=new IntPtr(number);var before=Identity(hwnd);
        if(!Same(before,expected))throw new InvalidOperationException("window_changed");
        if(!IsWindowVisible(hwnd)||IsIconic(hwnd))throw new InvalidOperationException("window_not_visible");
        var cache=new CacheRequest{TreeScope=TreeScope.Element};
        foreach(var property in new[]{AutomationElement.RuntimeIdProperty,AutomationElement.NameProperty,AutomationElement.ControlTypeProperty,AutomationElement.IsEnabledProperty,AutomationElement.IsOffscreenProperty,AutomationElement.IsPasswordProperty})cache.Add(property);
        using(cache.Activate()) {
            var root=AutomationElement.FromHandle(hwnd);
            AutomationElement document;AutomationElement[] nodes;
            if(expected.app=="qoder") {
                // This Electron build exposes its document during ControlView cache
                // enumeration; a direct ControlType predicate can return no document.
                nodes=root.FindAll(TreeScope.Subtree,Automation.ControlViewCondition).Cast<AutomationElement>().ToArray();
                var documents=nodes.Where(n=>n.Cached.ControlType==ControlType.Document).ToArray();
                if(documents.Length!=1)throw new InvalidOperationException("document_not_unique");document=documents[0];
            } else {
                var documents=root.FindAll(TreeScope.Children,new PropertyCondition(AutomationElement.ControlTypeProperty,ControlType.Document));
                if(documents.Count!=1)throw new InvalidOperationException("document_not_unique");document=documents[0];
                nodes=expected.version=="5.8.3.0"?WorkBuddy583Nodes(document,cache):document.FindAll(TreeScope.Subtree,Automation.ControlViewCondition).Cast<AutomationElement>().ToArray();
            }
            if(nodes.Length==0||nodes.Length>1000)throw new InvalidOperationException("tree_limit");
            string documentId=String.Join(".",document.GetRuntimeId());
            var content=new List<string>();var layout=new List<string>();var models=new List<string>();
            int editors=0;bool draftEmpty=false,busy=false;string session=null;AutomationElement editor=null;string draftText=null;
            foreach(var node in nodes) {
                var p=node.Cached;if(p.IsPassword)throw new InvalidOperationException("sensitive_control_present");
                var id=String.Join(".",(int[])node.GetCachedPropertyValue(AutomationElement.RuntimeIdProperty)??new int[0]);
                string name=p.Name??"";if(name.Length>65536)throw new InvalidOperationException("text_limit");
                layout.Add(p.ControlType.Id+":"+id+":"+p.IsEnabled+":"+p.IsOffscreen);
                if(p.ControlType==ControlType.Text)content.Add(name);
                if(expected.app=="qoder"&&p.ControlType==ControlType.Button&&!p.IsOffscreen&&name.StartsWith("模型: ")&&name.Length<=204)models.Add(name.Substring(4));
                // Observed in WorkBuddy 5.7.7's model selector. This is a UI
                // display label, not an authenticated backend model identifier.
                if(expected.app=="workbuddy"&&(expected.version=="5.7.7.0"||expected.version=="5.8.3.0")&&p.ControlType==ControlType.ComboBox&&p.IsEnabled&&!p.IsOffscreen&&name.StartsWith("Select model: ",StringComparison.Ordinal)&&name.Length<=214) {
                    string label=name.Substring(14);
                    if(!String.IsNullOrWhiteSpace(label)&&!label.Any(Char.IsControl))models.Add(label);
                }
                // Rich replies expose inline/code blocks as Edit controls. They
                // are message data, never composer candidates or Value sources.
                bool replyEditor=expected.app=="workbuddy"&&(expected.version=="5.7.7.0"||expected.version=="5.8.3.0")&&
                    id.StartsWith(documentId+".4.0.1.0.0.0.1.1.0.0.0.0.0.0.",StringComparison.Ordinal);
                string value=null;object pattern;
                // Only the composer and Qoder's route are needed. In particular,
                // WorkBuddy's Document.Value can contain account data: never read it.
                if(((p.ControlType==ControlType.Edit&&!replyEditor)||(p.ControlType==ControlType.Document&&expected.app=="qoder"))&&node.TryGetCurrentPattern(ValuePattern.Pattern,out pattern))value=((ValuePattern)pattern).Current.Value;
                if(value!=null&&value.Length>1024*1024)throw new InvalidOperationException("value_limit");
                if(p.ControlType==ControlType.Document&&expected.app=="qoder"&&value!=null) {
                    var match=Regex.Match(value,@"^qoder-app://[^\s]+#/chat/([0-9a-fA-F-]{36})(?:\?|$)");
                    Guid parsed;if(match.Success&&Guid.TryParse(match.Groups[1].Value,out parsed))session=parsed.ToString();
                }
                if(p.ControlType==ControlType.Edit&&!replyEditor&&!p.IsOffscreen&&(expected.app!="qoder"||name=="发送任务消息")) {
                    // WorkBuddy 5.7.6 also exposes a tiny, empty auxiliary editor.
                    // Exclude only a readable, exactly empty control of this size;
                    // drafts, unavailable values and ambiguous full editors still fail closed.
                    if(expected.app=="workbuddy"&&value=="") {
                        var rect=node.Current.BoundingRectangle;
                        if(!rect.IsEmpty&&rect.Width>0&&rect.Width<=32&&rect.Height>0&&rect.Height<=32)continue;
                    }
                    editors++;editor=node;
                    string draft=value;
                    if(expected.app=="qoder"&&draft!=null&&draft.EndsWith("\n"))draft=draft.Substring(0,draft.Length-1);
                    if(expected.app=="workbuddy"&&draft!=null)draft=draft.Trim('\ufeff','\r','\n');
                    draftText=draft;draftEmpty=draft==""||(expected.app=="workbuddy"&&draft=="今天帮你做些什么？@ 添加上下文，/调用技能与指令");
                }
                if(p.ControlType==ControlType.Button&&p.IsEnabled&&!p.IsOffscreen&&Regex.IsMatch(name,@"^(停止|停止生成|停止任务|允许一次|批准|允许本次)$"))busy=true;
            }
            if(editors!=1||(idle&&!draftEmpty))throw new InvalidOperationException("draft_or_composer_unavailable");
            if(idle&&busy)throw new InvalidOperationException("session_busy_or_approval_pending");
            if(expected.app=="qoder"&&session==null)throw new InvalidOperationException("session_id_unavailable");
            if(content.Count==0||content.Sum(s=>s.Length)>1024*1024)throw new InvalidOperationException("content_unavailable_or_limit");
            if(!Same(Identity(hwnd),before))throw new InvalidOperationException("window_changed");
            // Never return chat/account text or Document.Value. WorkBuddy has no authenticated session ID.
            return new View{nodes=nodes,editor=editor,draft=draftText,observation=new Observation{window=before,capturedAt=DateTime.UtcNow.ToString("o"),documentRuntimeId=String.Join(".",document.GetRuntimeId()),sessionId=session,contentDigest=Hash(json.Serialize(content)),layoutDigest=Hash(json.Serialize(layout)),nodeCount=nodes.Length,draftEmpty=draftEmpty,busy=busy,modelLabel=models.Count==1?models[0]:null}};
        }
    }
    [STAThread] public static int Main(string[] args) {
        Console.OutputEncoding=new UTF8Encoding(false);
        try {
            if(args.Length!=1||args[0].Length>65536)throw new InvalidOperationException("request_limit");
            var request=json.Deserialize<Request>(args[0]);
            if(request.version!=1)throw new InvalidOperationException("protocol_mismatch");
            object result;
            if(request.op=="discover") {
                var windows=new List<Window>();
                EnumWindows((hwnd,arg)=>{if(IsWindowVisible(hwnd))try{var window=Identity(hwnd);if(windows.Count>=32)throw new InvalidOperationException("window_limit");windows.Add(window);}catch(InvalidOperationException e){if(e.Message=="window_limit")return false;}catch{}return true;},IntPtr.Zero);
                if(windows.Count>=32)throw new InvalidOperationException("window_limit");
                result=new{windows=windows};
            } else if(request.op=="inspect")result=new{observation=Inspect(request.window)};
            else if(request.op=="trialprepare")result=PrepareTrial(request.window);
            else if(request.op=="trialsend")result=SendTrial(request.review);
            else if(request.op=="trialread")result=ReadTrial(request.review);
            else if(request.op=="tasksend")result=SendMessage(request.taskReview,true);
            else if(request.op=="taskread")result=ReadTask(request.taskReview);
            else if(request.op=="taskprogress")result=ReadTaskProgress(request.taskReview);
            else if(request.op=="taskprepare")result=PrepareTask(request.window,request.taskReview);
            else if(request.op=="taskstopprepare")result=PrepareStop(request.taskReview);
            else if(request.op=="taskstop")result=StopTask(request.taskReview,request.stopObservation);
            else if(request.op=="taskstopread")result=ReadStop(request.taskReview,request.stopObservation);
            else throw new InvalidOperationException("unsupported_read_operation");
            Console.WriteLine(json.Serialize(new{version=1,ok=true,result=result}));return 0;
        } catch(Exception e) {
            string[] known={"app_not_allowed","window_required","invalid_window","window_changed","window_not_visible","document_not_unique","tree_limit","text_limit","value_limit","sensitive_control_present","draft_or_composer_unavailable","session_busy_or_approval_pending","session_id_unavailable","content_unavailable_or_limit","request_limit","protocol_mismatch","window_limit","unsupported_read_operation","trial_app_not_enabled","trial_review_invalid","trial_expired","trial_view_changed","trial_history_unavailable","trial_history_changed","trial_lease_busy","trial_focus_changed","trial_draft_changed","trial_send_unavailable","trial_reply_unverified","trial_keyboard_unavailable","stop_identity_changed","stop_history_changed","stop_turn_changed","stop_control_ambiguous","stop_not_running","stop_review_invalid","stop_review_expired","stop_control_unavailable","stop_terminal_unverified"};
            Console.WriteLine(json.Serialize(new{version=1,ok=false,error=known.Contains(e.Message)?e.Message:"observation_unavailable"}));return 1;
        }
    }
}
