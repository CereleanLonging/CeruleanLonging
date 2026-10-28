using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Runtime.InteropServices;
using System.Web.Script.Serialization;
using System.Windows;
using System.Windows.Automation;
using System.Windows.Automation.Provider;
using System.Windows.Controls;
using System.Windows.Interop;
using System.Windows.Threading;
[assembly: AssemblyVersion("5.7.7.0")]
[assembly: AssemblyFileVersion("5.7.7.0")]
[assembly: AssemblyInformationalVersion("5.7.7.0")]

// AutomationPeer.GetRuntimeId is not virtual. This owned WPF window instead
// hosts an explicit UIA fragment provider so the *real cross-process UIA API*
// exposes the positional runtime IDs of WorkBuddy 5.7.7. No observer is mocked.
[ComVisible(true)]
class BuddyNode:IRawElementProviderFragment, IValueProvider, IInvokeProvider {
    public BuddyRoot Root;public BuddyNode Parent;public List<BuddyNode> Children=new List<BuddyNode>();
    public int[] Path;public int Type;public string Name="",ValueText="";public bool Enabled=true;
    public BuddyNode(BuddyRoot root,BuddyNode parent,int[] path,int type){Root=root;Parent=parent;Path=path;Type=type;}
    public virtual ProviderOptions ProviderOptions{get{return ProviderOptions.ServerSideProvider;}}
    public virtual IRawElementProviderSimple HostRawElementProvider{get{return null;}}
    public object GetPatternProvider(int pattern){if(Type==ControlType.Edit.Id&&pattern==ValuePatternIdentifiers.Pattern.Id)return this;if(Type==ControlType.Button.Id&&pattern==InvokePatternIdentifiers.Pattern.Id)return this;return null;}
    public object GetPropertyValue(int property){
        if(property==AutomationElementIdentifiers.ControlTypeProperty.Id)return Type;
        if(property==AutomationElementIdentifiers.NameProperty.Id)return Name;
        if(property==AutomationElementIdentifiers.IsEnabledProperty.Id)return Enabled;
        if(property==AutomationElementIdentifiers.IsOffscreenProperty.Id)return false;
        if(property==AutomationElementIdentifiers.IsControlElementProperty.Id||property==AutomationElementIdentifiers.IsContentElementProperty.Id)return true;
        if(property==AutomationElementIdentifiers.IsPasswordProperty.Id)return false;
        if(property==AutomationElementIdentifiers.IsKeyboardFocusableProperty.Id)return Root.FocusAction!=null&&!Root.ClickFocus&&Type==ControlType.Edit.Id;
        if(property==AutomationElementIdentifiers.HasKeyboardFocusProperty.Id)return Root.Focused==this;
        if(property==AutomationElementIdentifiers.ProcessIdProperty.Id)return Process.GetCurrentProcess().Id;
        if(property==AutomationElementIdentifiers.FrameworkIdProperty.Id)return "OwnedWorkBuddyProvider";
        if(property==AutomationElementIdentifiers.AutomationIdProperty.Id)return "owned-"+String.Join(".",Path);
        return null;
    }
    public IRawElementProviderFragment Navigate(NavigateDirection direction){
        if(direction==NavigateDirection.Parent)return Parent;
        if(direction==NavigateDirection.FirstChild)return Children.FirstOrDefault();
        if(direction==NavigateDirection.LastChild)return Children.LastOrDefault();
        if(Parent==null)return null;int index=Parent.Children.IndexOf(this);
        if(direction==NavigateDirection.NextSibling)return index>=0&&index+1<Parent.Children.Count?Parent.Children[index+1]:null;
        if(direction==NavigateDirection.PreviousSibling)return index>0?Parent.Children[index-1]:null;
        return null;
    }
    public virtual int[] GetRuntimeId(){return new[]{AutomationInteropProvider.AppendRuntimeId,Root.DocumentSeed}.Concat(Path).ToArray();}
    public Rect BoundingRectangle{get{return Root.SendButton==this?Root.SendBounds:Root.Bounds;}}
    public IRawElementProviderSimple[] GetEmbeddedFragmentRoots(){return null;}
    public void SetFocus(){if(Root.FocusAction==null||Type!=ControlType.Edit.Id)throw new InvalidOperationException("read-only owned fixture");Root.FocusAction(this);}
    public IRawElementProviderFragmentRoot FragmentRoot{get{return Root;}}
    public bool IsReadOnly{get{return false;}}
    public string Value{get{if(Root.SlowRead){Root.SlowReads++;File.WriteAllText(System.IO.Path.Combine(Root.Directory,"slow-reads.json"),new JavaScriptSerializer().Serialize(new{reads=Root.SlowReads}));System.Threading.Thread.Sleep(1200);}return ValueText;}}
    public void SetValue(string value){Root.CountMutation("value");throw new InvalidOperationException("unexpected input");}
    public void Invoke(){Root.CountMutation("invoke");if(!Enabled)throw new InvalidOperationException("disabled invocation");if(Name=="发送"&&Root.SendAction!=null){Root.SendAction();return;}if(Root.StopAction==null||Name!="停止")throw new InvalidOperationException("unexpected invocation");Root.StopAction();}
}
[ComVisible(true)]
class BuddyRoot:BuddyNode,IRawElementProviderFragmentRoot {
    public IntPtr Hwnd;public Rect Bounds;public int DocumentSeed=5707;public string Directory;public int ValueChanges,Invokes;
    public Action StopAction,SendAction;public Action<BuddyNode> FocusAction;public BuddyNode Focused,Editor,SendButton;public bool ClickFocus,HitMismatch,SendHitMismatch,SlowRead;public Rect SendBounds;public int SendClicks,SlowReads;
    public BuddyRoot():base(null,null,new int[0],ControlType.Window.Id){Root=this;}
    public override IRawElementProviderSimple HostRawElementProvider{get{return AutomationInteropProvider.HostProviderFromHandle(Hwnd);}}
    public override int[] GetRuntimeId(){return null;}
    public IRawElementProviderFragment ElementProviderFromPoint(double x,double y){if(SendBounds.Contains(x,y)&&SendButton!=null)return SendHitMismatch?Editor:SendButton;return ClickFocus&&!HitMismatch&&Editor!=null?Editor:Children.FirstOrDefault();}
    public IRawElementProviderFragment GetFocus(){return Focused;}
    public void CountMutation(string kind){if(kind=="value")ValueChanges++;else Invokes++;WriteCounts();}
    public void WriteCounts(){File.WriteAllText(System.IO.Path.Combine(Directory,"mutations.json"),new JavaScriptSerializer().Serialize(new{valueChanges=ValueChanges,invokes=Invokes}));File.WriteAllText(System.IO.Path.Combine(Directory,"send-clicks.json"),new JavaScriptSerializer().Serialize(new{sendClicks=SendClicks}));}
}
class WorkBuddyStopWindow {
    [DllImport("user32.dll")] static extern IntPtr SetFocus(IntPtr hwnd);
    class Command {public int sequence;public string mode,prompt,status;}
    static readonly string Panel="4.0.1.0.0.0.1",List=Panel+".1.0.0.0.0.0.0",Composer=Panel+".1.0.1.0.1.0";
    static BuddyNode Add(BuddyRoot root,BuddyNode document,string path,int type,string name=""){
        BuddyNode parent=document;string[] pieces=path.Split('.');var ids=new List<int>();
        for(int i=0;i<pieces.Length;i++){
            int part=Int32.Parse(pieces[i]);ids.Add(part);var child=parent.Children.FirstOrDefault(n=>n.Path.Last()==part);
            if(child==null){child=new BuddyNode(root,parent,ids.ToArray(),ControlType.Custom.Id);parent.Children.Add(child);parent.Children.Sort((a,b)=>a.Path.Last().CompareTo(b.Path.Last()));}
            parent=child;
        }
        parent.Type=type;parent.Name=name;return parent;
    }
    static void Round(BuddyRoot root,BuddyNode doc,int index,string prompt,string reply,string status){
        string user=List+"."+(index*2),answer=List+"."+(index*2+1);
        Add(root,doc,user+".0.0.0.0.0.0.0",ControlType.Text.Id,prompt);
        Add(root,doc,answer+".0.1",ControlType.Text.Id,"WorkBuddy");
        if(status!=null)Add(root,doc,answer+".0.2.0.0",ControlType.Text.Id,status);
        Add(root,doc,answer+".0.3.0",ControlType.Text.Id,reply);
    }
    static void Populate(BuddyRoot root,Command command){
        string mode=command.mode;root.DocumentSeed=mode=="document-changed"?5708:5707;
        var doc=new BuddyNode(root,root,new int[0],ControlType.Document.Id){Name="Owned 工作台接入验证"};
        Add(root,doc,Panel+".0.0.0",ControlType.Text.Id,"工作台接入验证");
        if(!mode.StartsWith("clipped-",StringComparison.Ordinal))Round(root,doc,0,mode=="history-changed"?"changed history":"history-1","reply-1","已完成 1s");
        if(mode!="clipped-too-short")Round(root,doc,1,"history-2","reply-2","已完成 2s");
        Round(root,doc,2,"history-3","reply-3","已完成 3s");
        if(mode!="initial"&&!mode.StartsWith("send-",StringComparison.Ordinal)){
            string status=mode=="cancelled"||mode=="cancelled-empty-reply"||mode=="terminal-busy"||mode=="clipped-cancelled"||mode=="clipped-too-short"?(command.status??"已取消"):mode=="completed"?"已完成 4s":mode=="failed"?"已失败":mode=="missing-status"?null:mode=="malformed-terminal"?(command.status??"已取消（引用文字）"):"生成中";
            Round(root,doc,3,mode=="prompt-changed"?"foreign task":command.prompt,mode=="quoted-terminal"?"已取消":mode=="cancelled-empty-reply"?"":"owned partial reply",status);
            if(mode=="wrong-role")Add(root,doc,List+".7.0.1",ControlType.Text.Id,"ForeignAgent");
            if(mode=="ambiguous-status")Add(root,doc,List+".7.0.2.1",ControlType.Text.Id,"已取消");
            if(mode=="body-stop")Add(root,doc,List+".7.0.3.1",ControlType.Button.Id,"停止");
            if(mode=="body-stop-task")Add(root,doc,List+".7.0.3.1",ControlType.Button.Id,"停止任务");
        }
        var editor=Add(root,doc,Composer+".1.0",ControlType.Edit.Id);editor.ValueText=mode=="draft"?"unrelated unsent draft":mode.StartsWith("send-existing",StringComparison.Ordinal)?(mode=="send-existing-altered"?"user changed draft":command.prompt):"";
        root.Editor=editor;root.ClickFocus=mode.StartsWith("send-click",StringComparison.Ordinal);root.HitMismatch=mode=="send-click-wrong-target";
        Add(root,doc,Composer+".2.2.0",ControlType.ComboBox.Id,"Select model: "+(mode=="model-changed"||mode=="send-existing-model"?"Different":"Hy3"));
        bool running=!mode.StartsWith("send-",StringComparison.Ordinal)&&mode!="initial"&&mode!="cancelled"&&mode!="cancelled-empty-reply"&&mode!="completed"&&mode!="failed"&&mode!="body-stop"&&mode!="clipped-cancelled"&&mode!="clipped-too-short";
        var stop=Add(root,doc,Composer+".2.2.3",ControlType.Button.Id,running?"停止":"发送");stop.Enabled=running||mode.StartsWith("send-existing",StringComparison.Ordinal);
        root.SendButton=stop;root.SendHitMismatch=mode=="send-button-wrong-target";
        if(mode=="duplicate-stop")Add(root,doc,Composer+".2.2.4",ControlType.Button.Id,"停止");
        if(mode=="send-duplicate") { var duplicate=Add(root,doc,Composer+".2.2.4",ControlType.Button.Id,"发送");duplicate.Enabled=false; }
        if(mode=="send-body-button") { stop.Name="";Add(root,doc,List+".5.0.3.1",ControlType.Button.Id,"发送"); }
        root.Children=new List<BuddyNode>{doc};
    }
    [STAThread]static void Main(string[] args){
        if(args.Length!=1||Environment.GetEnvironmentVariable("CERULEAN_NATIVE_OWNED_JOB")!="1")return;
        string directory=System.IO.Path.GetFullPath(args[0]);var json=new JavaScriptSerializer();
        var app=new Application();var window=new Window{Title="Owned WorkBuddy stop UIA fixture",Width=940,Height=620};
        var label=new TextBlock{Margin=new Thickness(24),FontSize=20,TextWrapping=TextWrapping.Wrap};window.Content=label;
        var root=new BuddyRoot{Directory=directory};int sequence=0,characters=0,completedSends=0;string activeMode=null;
        Action writeSend=()=>{
            string text=root.Editor==null?"":root.Editor.ValueText;
            // Unicode SendInput arrives as UTF-16 code units. Wait for the low
            // surrogate before serializing a character outside the BMP.
            if(text.Length>0&&Char.IsHighSurrogate(text[text.Length-1]))return;
            File.WriteAllText(System.IO.Path.Combine(directory,"send-input.json"),json.Serialize(new{characters=characters,text=text,invokes=root.Invokes}));
        };
        Action<Command> apply=null;
        apply=command=>{
            Populate(root,command);
            activeMode=command.mode;root.Focused=null;characters=0;completedSends=0;root.SlowRead=false;root.SlowReads=0;
            if(command.mode.StartsWith("send-",StringComparison.Ordinal))writeSend();
            root.FocusAction=command.mode.StartsWith("send-",StringComparison.Ordinal)?(Action<BuddyNode>)(node=>window.Dispatcher.Invoke(new Action(()=>{
                window.Activate();SetFocus(root.Hwnd);root.Focused=node;
                AutomationInteropProvider.RaiseAutomationEvent(AutomationElementIdentifiers.AutomationFocusChangedEvent,node,new AutomationEventArgs(AutomationElementIdentifiers.AutomationFocusChangedEvent));
            }))):null;
            root.SendAction=root.FocusAction==null?null:(Action)(()=>window.Dispatcher.Invoke(new Action(()=>{
                writeSend();if(command.mode=="send-unconfirmed")return;
                if(command.mode=="send-slow-unconfirmed"){root.SlowRead=true;return;}
                string sent=root.Editor.ValueText;
                string marker=System.Text.RegularExpressions.Regex.Match(sent,@"CL_TASK_[0-9a-f]{32}").Value;
                var doc=root.Children[0];Round(root,doc,3+completedSends++,sent,"owned reply "+marker, "已完成 4s");
                root.Editor.ValueText="";Add(root,doc,Composer+".2.2.3",ControlType.Button.Id,"发送").Enabled=false;
            })));
            root.StopAction=command.mode.StartsWith("action-",StringComparison.Ordinal)?(Action)(()=>{
                if(command.mode=="action-unconfirmed")return;
                string next=command.mode=="action-completed"?"completed":command.mode=="action-failed"?"failed":"cancelled";
                window.Dispatcher.BeginInvoke(new Action(()=>apply(new Command{mode=next,prompt=command.prompt})));
            }):null;
            if(command.mode.StartsWith("action-",StringComparison.Ordinal))window.Activate();
            label.Text="Owned WorkBuddy 5.7.7 UIA fixture\n\nState: "+command.mode+"\n\nThree completed history rounds; optional current task.\nNo external application or model is contacted.";
        };
        window.SourceInitialized+=(s,e)=>{
            root.Hwnd=new WindowInteropHelper(window).Handle;root.Bounds=new Rect(window.Left,window.Top,900,560);
            HwndSource.FromHwnd(root.Hwnd).AddHook((IntPtr hwnd,int message,IntPtr wParam,IntPtr lParam,ref bool handled)=>{
                if(message==0x003D&&lParam.ToInt64()==AutomationInteropProvider.RootObjectId){handled=true;return AutomationInteropProvider.ReturnRawElementProvider(hwnd,wParam,lParam,root);}
                if(message==0x0201||message==0x0202){
                    int packed=lParam.ToInt32();var screen=window.PointToScreen(new Point((short)(packed&65535),(short)((packed>>16)&65535)));
                    if(root.SendBounds.Contains(screen)&&root.SendButton!=null&&root.SendButton.Enabled&&root.SendAction!=null){
                        handled=true;if(message==0x0202){root.SendClicks++;root.WriteCounts();root.SendAction();}
                    }else if(message==0x0201&&root.ClickFocus&&root.FocusAction!=null){root.FocusAction(root.Editor);if(activeMode=="send-click-draft")root.Editor.ValueText="user draft appeared";handled=true;}
                }
                if(message==0x0102&&root.Focused!=null&&activeMode.StartsWith("send-",StringComparison.Ordinal)){
                    handled=true;root.Focused.ValueText+=(char)wParam.ToInt32();characters++;
                    var doc=root.Children[0];Add(root,doc,Composer+".2.2.3",ControlType.Button.Id,"发送").Enabled=true;
                    if(activeMode=="send-model-changed")Add(root,doc,Composer+".2.2.0",ControlType.ComboBox.Id,"Select model: Different");
                    if(activeMode=="send-disabled")Add(root,doc,Composer+".2.2.3",ControlType.Button.Id,"发送").Enabled=false;
                    writeSend();
                }
                return IntPtr.Zero;
            });
        };
        window.Loaded+=(s,e)=>{
            root.Bounds=new Rect(window.PointToScreen(new Point(0,0)),new Size(window.ActualWidth,window.ActualHeight));
            root.SendBounds=new Rect(window.PointToScreen(new Point(760,470)),new Size(70,45));
            apply(new Command{mode="initial"});root.WriteCounts();
            File.WriteAllText(System.IO.Path.Combine(directory,"window.json"),json.Serialize(new{app="workbuddy",hwnd=root.Hwnd.ToInt64().ToString(),pid=Process.GetCurrentProcess().Id,startTicks=Process.GetCurrentProcess().StartTime.ToUniversalTime().Ticks.ToString(),exe=Process.GetCurrentProcess().MainModule.FileName,title=window.Title,version="5.7.7.0"}));
        };
        var timer=new DispatcherTimer{Interval=TimeSpan.FromMilliseconds(40)};
        timer.Tick+=(s,e)=>{
            // Each immutable command is published by rename only after its write
            // handle closes. Consume exactly the next sequence, never reopen or
            // overwrite a file another process might still be reading.
            int next=sequence+1;
            string file=System.IO.Path.Combine(directory,"command-"+next+".json");if(!File.Exists(file))return;
            Command command;try{command=json.Deserialize<Command>(File.ReadAllText(file));}catch(IOException){return;}catch(ArgumentException){return;}
            if(command.sequence!=next)return;
            if(command.mode!="shutdown")apply(command);sequence=command.sequence;
            string ack=System.IO.Path.Combine(directory,"ack-"+sequence+".json");
            File.WriteAllText(ack+".tmp",json.Serialize(new{sequence=sequence,mode=command.mode}));
            File.Move(ack+".tmp",ack);
            if(command.mode=="shutdown"){timer.Stop();app.Shutdown();}
        };
        timer.Start();app.Run(window);
    }
}
