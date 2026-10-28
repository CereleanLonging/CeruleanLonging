using System;
using System.Diagnostics;
using System.IO;
using System.Reflection;
using System.Web.Script.Serialization;
using System.Windows;
using System.Windows.Automation;
using System.Windows.Automation.Peers;
using System.Windows.Controls;
using System.Windows.Interop;
[assembly: AssemblyVersion("5.7.7.0")]
[assembly: AssemblyFileVersion("5.7.7.0")]
[assembly: AssemblyInformationalVersion("5.7.7.0")]
class ChatDocument:GroupBox {
    protected override AutomationPeer OnCreateAutomationPeer(){return new ChatDocumentPeer(this);}
}
class ChatDocumentPeer:GroupBoxAutomationPeer {
    public ChatDocumentPeer(ChatDocument owner):base(owner){}
    protected override AutomationControlType GetAutomationControlTypeCore(){return AutomationControlType.Document;}
}
class WorkBuddyComposerWindow {
    [STAThread] static void Main(string[] args) {
        if(args.Length!=2||Environment.GetEnvironmentVariable("CERULEAN_NATIVE_OWNED_JOB")!="1")return;
        string dir=args[0],mode=args[1];
        var json=new JavaScriptSerializer();var app=new Application();
        var window=new Window{Title="Owned WorkBuddy composer fixture",Width=900,Height=500};
        var body=new StackPanel();window.Content=new ChatDocument{Content=body};
        body.Children.Add(new TextBlock{Text="Owned test chat history"});
        string placeholder="今天帮你做些什么？@ 添加上下文，/调用技能与指令";
        var auxiliary=new TextBox{Width=mode=="large-auxiliary"?100:22,Height=22,Text=mode=="auxiliary-draft"?"keep auxiliary draft":""};body.Children.Add(auxiliary);
        var editor=new TextBox{Width=740,Height=64,Text=mode=="draft"?"keep real draft":mode=="wrapped-draft"?"\ufeffkeep real draft\n":mode=="space"?" ":mode=="empty"?"":mode=="format-empty"?"\ufeff\n":"\ufeff"+placeholder+"\n"};
        if(mode!="missing")body.Children.Add(editor);
        if(mode=="duplicate")body.Children.Add(new TextBox{Width=740,Height=64,Text=""});
        body.Children.Add(new Button{Content=mode=="busy"?"停止":"发送",IsEnabled=mode=="busy"});
        if(mode.StartsWith("model-")) {
            if(mode=="model-text")body.Children.Add(new TextBlock{Text="Select model: Hy3"});
            else {
                var model=new ComboBox{Width=200,IsEnabled=mode!="model-disabled",Visibility=mode=="model-hidden"?Visibility.Collapsed:Visibility.Visible};
                model.Items.Add("Hy3");model.SelectedIndex=0;
                AutomationProperties.SetName(model,"Select model: "+(mode=="model-empty"?" ":mode=="model-control"?"Hy3\n":mode=="model-long"?new string('x',201):"Hy3"));body.Children.Add(model);
                if(mode=="model-duplicate"){var another=new ComboBox{Width=200};AutomationProperties.SetName(another,"Select model: Different");body.Children.Add(another);}
            }
        }
        int changes=0;auxiliary.TextChanged+=(s,e)=>File.WriteAllText(Path.Combine(dir,"changes.json"),(++changes).ToString());editor.TextChanged+=(s,e)=>File.WriteAllText(Path.Combine(dir,"changes.json"),(++changes).ToString());
        window.Loaded+=(s,e)=>{
            File.WriteAllText(Path.Combine(dir,"changes.json"),"0");
            File.WriteAllText(Path.Combine(dir,"window.json"),json.Serialize(new{app="workbuddy",hwnd=new WindowInteropHelper(window).Handle.ToInt64().ToString(),pid=Process.GetCurrentProcess().Id,startTicks=Process.GetCurrentProcess().StartTime.ToUniversalTime().Ticks.ToString(),exe=Process.GetCurrentProcess().MainModule.FileName,title=window.Title,version="5.7.7.0"}));
        };
        app.Run(window);
    }
}
