import { useEffect, useRef, useState } from "react";
import type { HistoricalResult, ProjectCurrencyBudget, Snapshot } from "./contracts";
import type { HistoricalQuery } from "./HistoricalChargeForm";
import { CurrencyAccounts } from "./CurrencyAccounts";

export function HistoricalCurrencyPanel({ snapshot, busy, query }: { snapshot: Snapshot | null; busy: boolean; query: HistoricalQuery }) {
  const [local, setLocal] = useState<{ base: Snapshot | null; result: HistoricalResult; accounts: ProjectCurrencyBudget[] } | null>(null);
  const latest = useRef(snapshot); latest.current = snapshot;
  const active = useRef(true);
  useEffect(() => { active.current = true; return () => { active.current = false; }; }, []);
  // A full newer snapshot is authoritative. Local query results never replace
  // projects, tasks, credentials, or any other omitted workbench collections.
  const fresh = local?.base === snapshot ? local : null;
  const read: HistoricalQuery = async command => {
    const base = latest.current;
    const result = await query(command);
    if (result && active.current && latest.current === base) {
      setLocal(previous => {
        const accounts = new Map((previous?.base === base ? previous.accounts : base?.currencyBudgets ?? []).map(a => [a.projectId, a]));
        for (const account of result.currencyBudgets) accounts.set(account.projectId, account);
        return { base, result, accounts: [...accounts.values()] };
      });
    }
    return result;
  };
  return <CurrencyAccounts accounts={fresh?.accounts ?? snapshot?.currencyBudgets ?? []}
    history={snapshot?.currencyHistory} accounted={snapshot?.currencyAccounted}
    sources={fresh ? fresh.result.historicalSources : snapshot?.historicalSources ?? null}
    accounting={fresh ? fresh.result.historicalAccounting : snapshot?.historicalAccounting ?? null}
    query={read} busy={busy}/>;
}
