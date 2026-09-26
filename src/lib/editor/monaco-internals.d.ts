// Monaco internals reached into on purpose (monaco-editor is pinned); see lib/editor/monaco.ts.
declare module "monaco-editor/editor/browser/widget/diffEditor/features/overviewRulerFeature" {
  export const OverviewRulerFeature: { ONE_OVERVIEW_WIDTH: number; ENTIRE_DIFF_OVERVIEW_WIDTH: number };
}

declare module "monaco-editor/editor/browser/widget/diffEditor/registrations.contribution" {
  export const diffWholeLineAddDecoration: { className: string };
  export const diffWholeLineDeleteDecoration: { className: string };
}

declare module "monaco-editor/editor/common/core/ranges/lineRange" {
  /** Lines [start, end), 1-based. */
  export class LineRange {
    constructor(startLineNumber: number, endLineNumberExclusive: number);
    readonly startLineNumber: number;
    readonly endLineNumberExclusive: number;
  }
}

declare module "monaco-editor/editor/common/diff/rangeMapping" {
  import type { Range } from "monaco-editor/editor/editor.api";
  import type { LineRange } from "monaco-editor/editor/common/core/ranges/lineRange";
  export class RangeMapping {
    constructor(originalRange: Range, modifiedRange: Range);
  }
  export class DetailedLineRangeMapping {
    constructor(original: LineRange, modified: LineRange, innerChanges: RangeMapping[] | undefined);
    readonly original: LineRange;
    readonly modified: LineRange;
  }
}

declare module "monaco-editor/editor/browser/widget/diffEditor/diffProviderFactoryService" {
  import type { editor } from "monaco-editor/editor/editor.api";
  import type { DetailedLineRangeMapping } from "monaco-editor/editor/common/diff/rangeMapping";
  export interface DocumentDiff {
    changes: readonly DetailedLineRangeMapping[];
    identical: boolean;
    quitEarly: boolean;
    moves: readonly unknown[];
  }
  export class WorkerBasedDocumentDiffProvider {
    computeDiff(original: editor.ITextModel, modified: editor.ITextModel, ...rest: unknown[]): Promise<DocumentDiff>;
  }
}

declare module "monaco-editor/editor/common/diff/defaultLinesDiffComputer/defaultLinesDiffComputer" {
  import type { DetailedLineRangeMapping } from "monaco-editor/editor/common/diff/rangeMapping";
  export class DefaultLinesDiffComputer {
    computeDiff(
      original: string[],
      modified: string[],
      options: { ignoreTrimWhitespace: boolean; maxComputationTimeMs: number; computeMoves: boolean; extendToSubwords?: boolean },
    ): { changes: readonly DetailedLineRangeMapping[]; hitTimeout: boolean };
  }
}
