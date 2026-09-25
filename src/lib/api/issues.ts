import { invoke } from "@tauri-apps/api/core";
import type { PullComment, Target } from "./github";

export interface IssueLabel {
  name: string;
  /** Hex without the '#'. */
  color: string;
  description: string;
}

export interface Issue {
  number: number;
  title: string;
  state: "open" | "closed";
  stateReason: "completed" | "not_planned" | "reopened" | null;
  author: string;
  labels: IssueLabel[];
  assignees: string[];
  /** Comment count. */
  comments: number;
  createdAt: string;
  updatedAt: string;
  url: string;
}

export interface IssueDetail extends Issue {
  body: string;
  thread: PullComment[];
  /** Who closed it, if closed: an author may reopen only what they closed themselves. */
  closedBy: string | null;
}

export interface IssueCounts {
  open: number;
  closed: number;
}

export type CloseReason = "completed" | "not_planned";

export const issues = {
  /** `labels`: only issues carrying all of them. */
  list: (target: Target, filter: "open" | "closed" | "all", labels: string[] = []) => invoke<Issue[]>("issue_list", { target, filter, labels }),
  /** How many issues are open and closed, carrying all of `labels`. */
  counts: (target: Target, labels: string[] = []) => invoke<IssueCounts>("issue_counts", { target, labels }),
  /** Every label defined in the repository. */
  labels: (target: Target) => invoke<IssueLabel[]>("issue_labels", { target }),
  detail: (target: Target, number: number) => invoke<IssueDetail>("issue_detail", { target, number }),
  create: (target: Target, title: string, body: string) => invoke<Issue>("issue_create", { target, title, body }),
  edit: (target: Target, number: number, title: string, body: string) => invoke<Issue>("issue_edit", { target, number, title, body }),
  setOpen: (target: Target, number: number, open: boolean, reason: CloseReason = "completed") =>
    invoke<Issue>("issue_set_open", { target, number, open, reason }),
  comment: (target: Target, number: number, body: string) => invoke<void>("issue_comment", { target, number, body }),
  /** Replaces the issue's labels; returns the ones it carries now. */
  setLabels: (target: Target, number: number, labels: string[]) => invoke<IssueLabel[]>("issue_set_labels", { target, number, labels }),
  /** Permanent; GitHub allows it to repository admins only. */
  delete: (target: Target, number: number) => invoke<void>("issue_delete", { target, number }),
};
