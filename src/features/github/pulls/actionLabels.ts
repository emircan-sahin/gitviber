import type { MergeMethod, ReviewEvent } from "@/lib/api";

export const METHODS: Record<MergeMethod, string> = { merge: "Create a merge commit", squash: "Squash and merge", rebase: "Rebase and merge" };

export const REVIEWS: Record<ReviewEvent, { label: string; note: string; done: string }> = {
  COMMENT: { label: "Comment", note: "General feedback without explicit approval.", done: "Review submitted" },
  APPROVE: { label: "Approve", note: "Give your approval to merge these changes.", done: "Approved" },
  REQUEST_CHANGES: { label: "Request changes", note: "Feedback that must be addressed before merging.", done: "Changes requested" },
};
