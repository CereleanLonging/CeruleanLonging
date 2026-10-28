using System;
using System.Diagnostics;
using System.IO;
using System.Reflection;
using System.Runtime.InteropServices;
using System.Web.Script.Serialization;
using System.Windows;
using System.Windows.Automation;
using System.Windows.Automation.Peers;
using System.Windows.Automation.Provider;
using System.Windows.Controls;
using System.Windows.Interop;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using System.Windows.Threading;
[assembly: AssemblyVersion("0.4.3.0")]
[assembly: AssemblyFileVersion("0.4.3.0")]
[assembly: AssemblyInformationalVersion("0.4.3.0")]

class FixtureDocument:GroupBox {
    public string Route="qoder-app://fixture#/chat/12345678-1234-1234-1234-123456789abc?fixture=1";
    protected override AutomationPeer OnCreateAutomationPeer(){return new FixtureDocumentPeer(this);}
}
class FixtureDocumentPeer:GroupBoxAutomationPeer,IValueProvider {
    public FixtureDocumentPeer(FixtureDocument owner):base(owner){}
    protected override AutomationControlType GetAutomationControlTypeCore(){return AutomationControlType.Document;}
    public override object GetPattern(PatternInterface pattern){return pattern==PatternInterface.Value?this:base.GetPattern(pattern);}
    public bool IsReadOnly{get{return true;}}
    public string Value{get{return ((FixtureDocument)Owner).Route;}}
    public void SetValue(string value){throw new InvalidOperationException();}
}
class DesktopJointWindow {
    [DllImport("user32.dll")]static extern bool AllowSetForegroundWindow(uint process);
    [DllImport("user32.dll")]static extern bool SetForegroundWindow(IntPtr window);
    static JavaScriptSerializer json=new JavaScriptSerializer();
    static GroupBox Group(string name,UIElement child=null){var g=new GroupBox{Content=child,Margin=new Thickness(1),Padding=new Thickness(1)};AutomationProperties.SetName(g,name);return g;}
    static GroupBox Round(string prompt,string reply,string state,bool actions){var p=new StackPanel();p.Children.Add(new TextBlock{Text=prompt,TextWrapping=TextWrapping.Wrap});p.Children.Add(Group("Agent 回复",new TextBlock{Text=reply}));if(actions)p.Children.Add(Group("回复操作"));p.Children.Add(Group(state));return Group("一轮任务",p);}
    static void Capture(Window window,string path){window.UpdateLayout();var bitmap=new RenderTargetBitmap((int)window.ActualWidth,(int)window.ActualHeight,96,96,PixelFormats.Pbgra32);bitmap.Render(window);var encoder=new PngBitmapEncoder();encoder.Frames.Add(BitmapFrame.Create(bitmap));using(var output=File.Create(path))encoder.Save(output);}
    [STAThread]static void Main(string[] args){
        if(args.Length!=3||Environment.GetEnvironmentVariable("CERULEAN_NATIVE_OWNED_JOB")!="1")return;
        string root=Path.GetFullPath(args[0]),mode=args[1];uint controller=UInt32.Parse(args[2]);
        var app=new Application();var window=new Window{Title="Owned desktop joint cancellation fixture",Width=950,Height=820};
        var doc=new FixtureDocument();AutomationProperties.SetName(doc,"fixture chat");
        var body=new StackPanel();doc.Content=body;window.Content=doc;
        var model=new Button{Content="模型: fixture-model"};body.Children.Add(model);
        var history=new StackPanel();body.Children.Add(Group("消息列表",history));
        history.Children.Add(Round("history-1","reply-1","状态：成功",true));history.Children.Add(Round("history-2","reply-2","状态：成功",true));history.Children.Add(Round("history-3","reply-3","状态：成功",true));
        body.Children.Add(Group("消息锚点"));
        var editor=new TextBox();AutomationProperties.SetName(editor,"发送任务消息");body.Children.Add(editor);
        var stop=new Button{Content="停止生成",Visibility=Visibility.Collapsed};body.Children.Add(stop);
        var send=new Button{Content="发送消息"};body.Children.Add(send);
        var state=Group("状态：运行中");int count=0,sends=0;bool started=false;
        send.Click+=(s,e)=>{if(started||editor.Text.Length==0)return;sends++;File.WriteAllText(Path.Combine(root,"send-count.json"),sends.ToString());File.WriteAllText(Path.Combine(root,"start.json"),json.Serialize(editor.Text));editor.Text="";send.IsEnabled=false;};
        stop.Click+=(s,e)=>{count++;File.WriteAllText(Path.Combine(root,"stop-count.json"),count.ToString());stop.Visibility=Visibility.Collapsed;AutomationProperties.SetName(state,mode=="completed-on-stop"?"状态：成功":mode=="failed-on-stop"?"状态：失败":mode=="unconfirmed"?"状态：运行中":"状态：已中断，耗时：1s");window.Dispatcher.BeginInvoke(new Action(()=>Capture(window,Path.Combine(root,"after.png"))),DispatcherPriority.ApplicationIdle);};
        window.Loaded+=(s,e)=>{var h=new WindowInteropHelper(window).Handle;window.Activate();SetForegroundWindow(h);bool granted=AllowSetForegroundWindow(controller);File.WriteAllText(Path.Combine(root,"window.json"),json.Serialize(new{hwnd=h.ToInt64().ToString(),pid=Process.GetCurrentProcess().Id,startTicks=Process.GetCurrentProcess().StartTime.ToUniversalTime().Ticks.ToString(),exe=Process.GetCurrentProcess().MainModule.FileName,title=window.Title,version="0.4.3.0",app="qoder",foregroundGranted=granted}));};
        var timer=new DispatcherTimer{Interval=TimeSpan.FromMilliseconds(50)};
        timer.Tick+=(s,e)=>{
            var start=Path.Combine(root,"start.json");
            if(!started&&File.Exists(start)){
                string prompt=json.Deserialize<string>(File.ReadAllText(start));started=true;
                var part=new StackPanel();part.Children.Add(new TextBlock{Text=(mode=="foreign-prompt"||mode=="quoted-prompt")?"unrelated task":prompt,TextWrapping=TextWrapping.Wrap,MaxHeight=120});part.Children.Add(Group("Agent 回复",new TextBlock{Text=mode=="quoted-prompt"?prompt:"generating fixture reply"}));part.Children.Add(Group("回复操作"));part.Children.Add(state);
                if(mode=="history-button")part.Children.Add(new Button{Content="停止生成"});
                history.Children.Add(Group("一轮任务",part));
                if(mode=="history-changed")history.Children.RemoveAt(1);
                if(mode=="clipped-history")history.Children.RemoveAt(0);
                if(mode=="changed-model")model.Content="模型: different-model";
                if(mode=="changed-session")doc.Route="qoder-app://fixture#/chat/00000000-0000-0000-0000-000000000001?fixture=1";
                stop.Visibility=mode=="history-button"?Visibility.Collapsed:Visibility.Visible;
                if(mode=="duplicate-stop")body.Children.Add(new Button{Content="停止生成"});
                if(mode=="already-completed")AutomationProperties.SetName(state,"状态：成功");
                if(mode=="already-failed")AutomationProperties.SetName(state,"状态：失败");
                if(mode=="draft")editor.Text="unrelated unsent draft";
                Capture(window,Path.Combine(root,"before.png"));File.WriteAllText(Path.Combine(root,"started.json"),"true");
            }
        };timer.Start();app.Run(window);
    }
}
