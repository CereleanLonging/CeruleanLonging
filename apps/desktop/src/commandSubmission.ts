import type { Command, CommandReceipt, CommandResult, Snapshot } from "./contracts";

export type SubmissionOutcome =
  | { type: "accepted"; receipt: CommandReceipt; snapshot: Snapshot | null; refreshError: string | null }
  | { type: "query"; snapshot: Snapshot }
  | { type: "unconfirmed"; error: string };

export async function submitAndRead(command: Command, submit: (command: Command) => Promise<CommandResult>, read: () => Promise<Snapshot>): Promise<SubmissionOutcome> {
  let result: CommandResult;
  try { result = await submit(command); }
  catch (error) { return { type: "unconfirmed", error: String(error) }; }
  if (result.type === "snapshot") return { type: "query", snapshot: result.value };
  // Refresh is a separate read. Its failure cannot undo a received acceptance
  // or authorize submitting the original command again.
  try { return { type: "accepted", receipt: result.value, snapshot: await read(), refreshError: null }; }
  catch (error) { return { type: "accepted", receipt: result.value, snapshot: null, refreshError: String(error) }; }
}
