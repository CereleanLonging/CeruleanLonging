//! Narrow in-memory requests to the original Host account owner.
//! Neither archived receipts nor worker messages create a second money writer.
use crate::{
    api_call_journal::{ApiCallJournal, CallIdentity},
    api_execution::ApiRunIdentity,
    async_api::Update,
    currency_budget::{ApiFeePermit, ApiPreparationPermit},
};
use cerulean_adapters::api_generation::{
    scoped::{CountedScopedApi, PreparedScopedApi, ScopedApiDraft},
    ApiClient, ApiFailure, ApiReply, ApiRequest,
};
use cerulean_domain::{currency_budget::scoped::ScopedPriceQuote, execution::require};
use std::{path::Path, sync::mpsc, time::Duration};
use tokio::sync::oneshot;

/// Not serializable/cloneable: one Host authorization for original dispatch.
pub(crate) struct DispatchPermit {
    run: ApiRunIdentity,
}
impl DispatchPermit {
    pub(super) fn new(run: ApiRunIdentity) -> Self {
        Self { run }
    }
    pub(crate) fn check(self, run: &ApiRunIdentity) -> Result<(), String> {
        require(
            self.run == *run,
            "API dispatch permit belongs to another run",
        )
    }
}
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Authority {
    pub(crate) policy: String,
    pub(crate) setup: String,
    pub(crate) run_control: String,
}
pub(crate) struct CountApproval {
    pub(crate) draft: ScopedApiDraft,
    pub(crate) quote: ScopedPriceQuote,
    pub(crate) authority: Authority,
}
pub(crate) struct PreparedApproval {
    pub(crate) prepared: PreparedScopedApi,
    pub(crate) permit: ApiPreparationPermit,
}
pub(crate) enum Request {
    Dispatch {
        run: Box<ApiRunIdentity>,
        reply: oneshot::Sender<Result<DispatchPermit, String>>,
    },
    Count {
        run: Box<ApiRunIdentity>,
        call: Box<CallIdentity>,
        draft: Box<ScopedApiDraft>,
        reply: oneshot::Sender<Result<CountApproval, String>>,
    },
    Counted {
        run: Box<ApiRunIdentity>,
        call: Box<CallIdentity>,
        authority: Authority,
        result: Result<Box<CountedScopedApi>, ApiFailure>,
        reply: oneshot::Sender<Result<PreparedApproval, String>>,
    },
    Settle {
        run: Box<ApiRunIdentity>,
        authority: Authority,
        journal: Box<ApiCallJournal>,
        reply: oneshot::Sender<Result<(ApiCallJournal, ApiFeePermit), String>>,
    },
    Continue {
        run: Box<ApiRunIdentity>,
        authority: Authority,
        call: Box<CallIdentity>,
        reply: oneshot::Sender<Result<(), String>>,
    },
}
#[cfg(test)]
type FixtureCount = std::sync::Arc<
    dyn Fn(ScopedApiDraft, ScopedPriceQuote) -> Result<CountedScopedApi, ApiFailure> + Send + Sync,
>;
#[cfg(test)]
type FixtureSend = std::sync::Arc<
    dyn Fn(
            &PreparedScopedApi,
            u32,
        )
            -> Result<cerulean_adapters::api_generation::scoped::ScopedApiResult, ApiFailure>
        + Send
        + Sync,
>;
#[cfg(test)]
#[derive(Clone)]
pub(crate) struct Fixture {
    pub(crate) count: FixtureCount,
    pub(crate) send: FixtureSend,
}
pub(crate) struct Driver<'a> {
    pub(crate) client: &'a ApiClient,
    pub(crate) sender: &'a mpsc::Sender<Update>,
    pub(crate) last: Option<(CallIdentity, Authority)>,
    #[cfg(test)]
    pub(crate) fixture: Option<Fixture>,
}
async fn acknowledged<T>(response: oneshot::Receiver<Result<T, String>>) -> Result<T, String> {
    tokio::time::timeout(Duration::from_secs(30), response)
        .await
        .map_err(|_| "API money acknowledgement timed out; never retry")?
        .map_err(|_| "API money acknowledgement lost; never retry")?
}
impl Driver<'_> {
    fn notify(&self, request: Request) -> Result<(), String> {
        self.sender
            .send(Update::Currency(Box::new(request)))
            .map_err(|_| "original Host money owner unavailable".into())
    }
}
impl crate::api_execution::RoundDriver for Driver<'_> {
    fn scoped(&self) -> bool {
        true
    }
    async fn authorize(&mut self, run: &ApiRunIdentity) -> Result<Option<DispatchPermit>, String> {
        let (reply, response) = oneshot::channel();
        self.notify(Request::Dispatch {
            run: Box::new(run.clone()),
            reply,
        })?;
        acknowledged(response).await.map(Some)
    }
    async fn round(
        &mut self,
        run: &ApiRunIdentity,
        round: u32,
        request: ApiRequest,
        advice: Option<&crate::api_advice_link::AppliedAdvice>,
        root: &Path,
        cancel: &mut oneshot::Receiver<()>,
    ) -> Result<(CallIdentity, ApiReply, String), String> {
        if let Some(advice) = advice {
            advice.validate_request(run, &request)?;
        }
        let draft = self.client.prepare_scoped(request)?;
        let call =
            crate::api_execution::call_identity(run, round, draft.fingerprint().request.clone())?;
        if let Some(advice) = advice {
            advice.link(root, run, &call)?;
        }
        let (reply, response) = oneshot::channel();
        self.notify(Request::Count {
            run: Box::new(run.clone()),
            call: Box::new(call.clone()),
            draft: Box::new(draft),
            reply,
        })?;
        let approval = acknowledged(response).await?;
        let authority = approval.authority;
        let result = {
            #[cfg(test)]
            if let Some(fixture) = &self.fixture {
                (fixture.count)(approval.draft, approval.quote)
            } else {
                self.client
                    .count_scoped_round(approval.draft, approval.quote, cancel)
                    .await
            }
            #[cfg(not(test))]
            self.client
                .count_scoped_round(approval.draft, approval.quote, cancel)
                .await
        };
        // Always return the original count outcome, even after cancellation.
        let (reply, response) = oneshot::channel();
        self.notify(Request::Counted {
            run: Box::new(run.clone()),
            call: Box::new(call.clone()),
            authority: authority.clone(),
            result: result.map(Box::new),
            reply,
        })?;
        let prepared = acknowledged(response).await?;
        crate::api_execution::check_cancel(cancel)?;
        let mut journal =
            ApiCallJournal::open(&root.join("rounds").join(format!("{round:03}")), &call)?;
        let pending = {
            #[cfg(test)]
            if let Some(fixture) = &self.fixture {
                journal
                    .execute_scoped_with(prepared.prepared, prepared.permit, |p| async move {
                        (fixture.send)(&p, round)
                    })
                    .await?
            } else {
                journal
                    .execute_scoped(prepared.prepared, prepared.permit, cancel)
                    .await?
            }
            #[cfg(not(test))]
            journal
                .execute_scoped(prepared.prepared, prepared.permit, cancel)
                .await?
        };
        let (reply, response) = oneshot::channel();
        self.notify(Request::Settle {
            run: Box::new(run.clone()),
            authority: authority.clone(),
            journal: Box::new(journal),
            reply,
        })?;
        let (mut journal, fee) = acknowledged(response).await?;
        let result = journal
            .release_scoped_reply(pending, fee)?
            .map_err(|e| format!("API response unusable after fee observation: {e:?}"))?;
        let head = journal.head().to_owned();
        drop(journal);
        self.last = Some((call.clone(), authority));
        Ok((call, result, head))
    }
    async fn checkpoint(&mut self, run: &ApiRunIdentity) -> Result<(), String> {
        let (call, authority) = self.last.as_ref().ok_or("API previous fee ACK missing")?;
        let (reply, response) = oneshot::channel();
        self.notify(Request::Continue {
            run: Box::new(run.clone()),
            authority: authority.clone(),
            call: Box::new(call.clone()),
            reply,
        })?;
        acknowledged(response).await
    }
}
