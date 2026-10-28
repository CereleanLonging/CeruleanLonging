using System;
using System.Linq;
using System.Threading;
using System.Windows.Automation;

// Test-only incident operation. Not exposed by the production request protocol.
public partial class DesktopObserver {
    static View CheckedIncidentDraft(TaskReview review) {
        ReviewTask(review,true);
        if(review.observation.window.app!="workbuddy"||review.observation.window.version!="5.7.7.0"||review.observation.modelLabel!="Hy3")throw new InvalidOperationException("trial_review_invalid");
        var view=ReadView(review.observation.window,false);Continuity(view,review);
        string header=view.observation.documentRuntimeId+".4.0.1.0.0.0.1.0.0.0";
        if(view.nodes.Count(n=>Runtime(n)==header&&n.Cached.ControlType==ControlType.Text&&n.Cached.Name=="工作台接入验证"&&!n.Cached.IsOffscreen)!=1)throw new InvalidOperationException("trial_view_changed");
        var rounds=TaskRounds(view);
        if(view.draft!=review.prompt||view.observation.busy||view.observation.modelLabel!=review.observation.modelLabel||!Baseline(rounds).SequenceEqual(review.baseline)||rounds.Any(r=>r.user.Contains(review.prompt)))throw new InvalidOperationException("trial_draft_changed");
        WorkBuddySendButton(view,true);return view;
    }
    static object PrepareIncidentDraft(TaskReview review) {
        var first=CheckedIncidentDraft(review);var second=CheckedIncidentDraft(review);
        if(!SameView(first.observation,second.observation))throw new InvalidOperationException("trial_view_changed");
        return new{draftObservation=second.observation,draftMatchesApproved=true,historyUnchanged=true};
    }
    static object SubmitIncidentDraft(TaskReview review,Observation expected) {
        if(expected==null)throw new InvalidOperationException("trial_review_invalid");
        DateTime at;if(!DateTime.TryParse(expected.capturedAt,null,System.Globalization.DateTimeStyles.RoundtripKind,out at))throw new InvalidOperationException("trial_review_invalid");
        var age=DateTime.UtcNow-at.ToUniversalTime();if(age.TotalSeconds<0||age.TotalSeconds>30)throw new InvalidOperationException("trial_expired");
        using(var mutex=new Mutex(false,@"Local\CeruleanDesktopTrialActionV1")){
            bool acquired=false;
            try {
                try{acquired=mutex.WaitOne(0);}catch(AbandonedMutexException){acquired=true;}
                if(!acquired)throw new InvalidOperationException("trial_lease_busy");
                var view=CheckedIncidentDraft(review);if(!SameView(view.observation,expected))throw new InvalidOperationException("trial_view_changed");
                var hwnd=new IntPtr(Int64.Parse(review.observation.window.hwnd));SetForegroundWindow(hwnd);Thread.Sleep(100);Foreground(hwnd,review.observation.window);
                view=CheckedIncidentDraft(review);if(!SameView(view.observation,expected))throw new InvalidOperationException("trial_view_changed");
                ClickWorkBuddySend(review,WorkBuddySendButton(view,true));
                return WaitSentReply(review,true);
            } finally {if(acquired)mutex.ReleaseMutex();}
        }
    }
}
