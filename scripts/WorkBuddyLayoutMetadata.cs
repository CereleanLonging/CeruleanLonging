using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Linq;
using System.Runtime.InteropServices;
using System.Text;
using System.Web.Script.Serialization;
using System.Windows.Automation;

// Default: read-only version metadata, no values or input. Explicit navigation
// mode delegates to WorkBuddyVerificationChat; neither mode sends messages.
public static partial class WorkBuddyLayoutMetadata {
    delegate bool EnumProc(IntPtr hwnd, IntPtr arg);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc callback, IntPtr arg);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr hwnd, out int pid);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr hwnd);
    [DllImport("user32.dll")] static extern bool IsIconic(IntPtr hwnd);
    static readonly string ExpectedPath = @"D:\workbuddy\WorkBuddy.exe";
    static string ExpectedVersion;
    static string Id(AutomationElement element) { return String.Join(".", element.GetRuntimeId()); }
    static string Category(AutomationElement element, ControlType type, bool password) {
        if(password || (type != ControlType.Button && type != ControlType.ComboBox)) return null;
        var name=element.Current.Name ?? "";
        if(name.Length>256)return null;
        if(type==ControlType.Button && (name=="工作台接入验证" || name.StartsWith("工作台接入验证 ",StringComparison.Ordinal)))return "verification-chat-link";
        if(type==ControlType.ComboBox && name.StartsWith("Select model: ",StringComparison.Ordinal))return "model-selector";
        if(type==ControlType.ComboBox && (name.StartsWith("模型",StringComparison.Ordinal) || name.StartsWith("Model",StringComparison.Ordinal)))return "possible-model-selector";
        if(new[]{"发送","发送消息","Send","Send message"}.Contains(name))return "send";
        if(new[]{"停止","停止生成","停止任务","Stop","Stop generating","Stop generation"}.Contains(name))return "stop";
        if(new[]{"批准","允许一次","允许本次","Approve","Allow once"}.Contains(name))return "approval";
        return null;
    }
    static object Inspect(IntPtr hwnd, int pid) {
        using(var process=Process.GetProcessById(pid)) {
            var path=process.MainModule.FileName;
            var version=process.MainModule.FileVersionInfo.ProductVersion;
            var started=process.StartTime.ToUniversalTime().Ticks;
            if(!String.Equals(path,ExpectedPath,StringComparison.OrdinalIgnoreCase) || version!=ExpectedVersion)
                throw new InvalidOperationException("unexpected_application_identity");
            var root=AutomationElement.FromHandle(hwnd);
            Console.Error.WriteLine("stage=document-children");
            var documents=root.FindAll(TreeScope.Children,new PropertyCondition(AutomationElement.ControlTypeProperty,ControlType.Document));
            if(documents.Count!=1)throw new InvalidOperationException("document_not_unique");
            var document=documents[0];
            var rows=new List<object>();
            var cache=new CacheRequest{TreeScope=TreeScope.Element};
            foreach(var property in new[]{AutomationElement.RuntimeIdProperty,AutomationElement.ControlTypeProperty,
                AutomationElement.IsEnabledProperty,AutomationElement.IsOffscreenProperty,AutomationElement.IsPasswordProperty,
                AutomationElement.BoundingRectangleProperty,AutomationElement.IsValuePatternAvailableProperty})cache.Add(property);
            using(cache.Activate()) {
                Console.Error.WriteLine("stage=selected-controls");
                var types=new OrCondition(
                    new PropertyCondition(AutomationElement.ControlTypeProperty,ControlType.Document),
                    new PropertyCondition(AutomationElement.ControlTypeProperty,ControlType.Edit),
                    new PropertyCondition(AutomationElement.ControlTypeProperty,ControlType.ComboBox),
                    new PropertyCondition(AutomationElement.ControlTypeProperty,ControlType.Button));
                var elements=document.FindAll(TreeScope.Subtree,new AndCondition(Automation.ControlViewCondition,types));
                if(elements.Count>1000)throw new InvalidOperationException("bounded_metadata_limit");
                Console.Error.WriteLine("stage=metadata count="+elements.Count);
                foreach(AutomationElement element in elements) {
                    var current=element.Cached;var type=current.ControlType;var rect=current.BoundingRectangle;
                    var id=String.Join(".",(int[])element.GetCachedPropertyValue(AutomationElement.RuntimeIdProperty));
                    rows.Add(new {id=id,type=type.ProgrammaticName,
                        enabled=current.IsEnabled,offscreen=current.IsOffscreen,password=current.IsPassword,
                        category=Category(element,type,current.IsPassword),
                        hasValuePattern=(bool)element.GetCachedPropertyValue(AutomationElement.IsValuePatternAvailableProperty),
                        width=rect.IsEmpty?0:Math.Round(rect.Width),height=rect.IsEmpty?0:Math.Round(rect.Height)});
                }
            }
            // Match only the user-authorized chat label. Never export other
            // conversation names or read the Document's route/account value.
            var markers=new List<object>();
            using(cache.Activate()) {
                Console.Error.WriteLine("stage=verification-label");
                var matches=document.FindAll(TreeScope.Descendants,new AndCondition(
                    Automation.ControlViewCondition,
                    new PropertyCondition(AutomationElement.ControlTypeProperty,ControlType.Text),
                    new PropertyCondition(AutomationElement.NameProperty,"工作台接入验证")));
                if(matches.Count>16)throw new InvalidOperationException("verification_label_ambiguous");
                foreach(AutomationElement element in matches) {
                    var p=element.Cached;
                    markers.Add(new {id=String.Join(".",(int[])element.GetCachedPropertyValue(AutomationElement.RuntimeIdProperty)),
                        type=p.ControlType.ProgrammaticName,enabled=p.IsEnabled,offscreen=p.IsOffscreen});
                }
            }
            int afterPid;GetWindowThreadProcessId(hwnd,out afterPid);
            if(afterPid!=pid || process.HasExited || process.StartTime.ToUniversalTime().Ticks!=started)
                throw new InvalidOperationException("window_changed");
            return new {hwnd=hwnd.ToInt64().ToString(),pid=pid,startTicks=started.ToString(),path=path,version=version,
                visible=IsWindowVisible(hwnd),minimized=IsIconic(hwnd),controls=rows,verificationChatMarkers=markers};
        }
    }
    [STAThread] public static int Main(string[] args) {
        Console.OutputEncoding=new UTF8Encoding(false);
        var found=new List<Tuple<IntPtr,int>>();var windows=new List<object>();
        try {
            if(args.Length<1 || args.Length>2 || !new[]{"5.8.2.0","5.8.3.0"}.Contains(args[0]) || (args.Length==2 && !new[]{"--open-verification-chat","--chat-structure","--production-observe","--production-prepare"}.Contains(args[1])))throw new InvalidOperationException("explicit_supported_metadata_version_required");
            ExpectedVersion=args[0];
            EnumWindows((hwnd,arg)=>{
                int pid;GetWindowThreadProcessId(hwnd,out pid);
                try {using(var process=Process.GetProcessById(pid)) {
                    if(String.Equals(process.MainModule.FileName,ExpectedPath,StringComparison.OrdinalIgnoreCase) && IsWindowVisible(hwnd) && process.MainWindowHandle==hwnd)
                        found.Add(Tuple.Create(hwnd,pid));
                }}catch{}
                return found.Count<8;
            },IntPtr.Zero);
            if(args.Length==2) {
                if(found.Count!=1 || ExpectedVersion!="5.8.3.0")throw new InvalidOperationException("navigation_window_not_unique_or_supported");
                var result=args[1]=="--chat-structure"?InspectChatStructure(found[0].Item1,found[0].Item2):args[1]=="--production-observe"||args[1]=="--production-prepare"?DesktopObserver.InspectAuthorizedWorkBuddy(found[0].Item1,found[0].Item2,args[1]=="--production-prepare"):OpenVerificationChat(found[0].Item1,found[0].Item2);
                Console.WriteLine(new JavaScriptSerializer{MaxJsonLength=1024*1024}.Serialize(result));
                return 0;
            }
            foreach(var window in found)windows.Add(Inspect(window.Item1,window.Item2));
            Console.WriteLine(new JavaScriptSerializer{MaxJsonLength=1024*1024}.Serialize(new {
                status=windows.Count>0?"observed":"no-visible-window",capturedAt=DateTime.UtcNow.ToString("o"),windows=windows,
                readOnly=true,focusActions=0,inputEvents=0,sendActions=0,chatTextRead=false,valueReads=0,credentialRead=false}));
            return 0;
        }catch(Exception e) {
            Console.WriteLine(new JavaScriptSerializer().Serialize(new {status="unavailable",errorType=e.GetType().Name,
                errorCode=e is InvalidOperationException?e.Message:"metadata_observation_failed",readOnly=!args.Contains("--open-verification-chat"),
                navigationOutcome=args.Contains("--open-verification-chat")?"unconfirmed-see-intent-log":"not-requested",sendActions=0}));
            return 1;
        }
    }
}
