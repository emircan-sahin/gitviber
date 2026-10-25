import { cx } from "../ui.tsx";
import { tokenize, type DiffLine, type Token } from "../demo-data.ts";

// VS Code Dark+, the app's default dark syntax theme, on the app's background instead of its own.
const DARK_PLUS: Record<NonNullable<Token["tone"]>, string> = {
  kw: "text-[#569cd6]",
  flow: "text-[#c586c0]",
  str: "text-[#ce9178]",
  num: "text-[#b5cea8]",
  fn: "text-[#dcdcaa]",
  type: "text-[#4ec9b0]",
  com: "text-[#6a9955]",
  var: "text-[#9cdcfe]",
};

export interface NumberedLine extends DiffLine {
  old: string;
  new: string;
}

/** Numbers each line on the old and new side, the way the inline diff shows them. */
export function numbered(lines: DiffLine[], [oldStart, newStart]: [number, number]): NumberedLine[] {
  let o = oldStart;
  let n = newStart;
  return lines.map((line) => ({
    ...line,
    old: line.kind === "+" || oldStart === 0 ? "" : String(o++),
    new: line.kind === "-" ? "" : String(n++),
  }));
}

function Code({ text }: { text: string }) {
  return tokenize(text).map((t, j) => (
    <span key={j} className={t.tone && DARK_PLUS[t.tone]}>
      {t.text}
    </span>
  ));
}

export interface FileLine {
  text: string;
  /** The gutter's change bar: a line the diff added, or one it changed. */
  bar?: "add" | "mod";
}

/** The file itself, the diff's new side: a 3px change bar at the edge, one number column, a caret while editing. */
export function FileRows({ lines, caret }: { lines: FileLine[]; caret?: number }) {
  return (
    <div className="bg-bg pt-1 font-mono text-[12.5px] leading-5 whitespace-pre text-[#d4d4d4]">
      {lines.map((line, i) => (
        <div key={i} className="relative flex h-5">
          {line.bar && <span className={cx("absolute inset-y-0 left-1.5 w-[3px]", line.bar === "add" ? "bg-added" : "bg-primary")} />}
          <span className="w-[42px] shrink-0 pr-2 text-right text-subtle">{i + 1}</span>
          <span className="w-[14px] shrink-0" />
          <span className="min-w-0 flex-1 overflow-hidden">
            <Code text={line.text} />
            {i === caret && <span className="ml-px inline-block h-4 w-px translate-y-[3px] animate-blink bg-fg" />}
          </span>
        </div>
      ))}
    </div>
  );
}

/**
 * The unified diff as the app draws it (crates/diff/src/paint.rs): the code font at 12.5 on 20px
 * lines, the old and new numbers in a gutter tinted for added or removed lines, then a sign
 * column and the code on the row's tint.
 */
export function DiffRows({ lines, fresh }: { lines: NumberedLine[]; fresh?: number }) {
  return (
    <div className="bg-bg pt-1 font-mono text-[12.5px] leading-5 whitespace-pre text-[#d4d4d4]">
      {lines.map((line, i) => {
        const add = line.kind === "+";
        const del = line.kind === "-";
        const gutter = cx("w-[31px] shrink-0 pr-1.5 text-right text-subtle", add && "bg-add-gutter", del && "bg-del-gutter");
        return (
          <div key={i} className={cx("flex h-5", i === fresh && "line-in")}>
            <span className={gutter}>{line.old}</span>
            <span className={gutter}>{line.new}</span>
            <span className={cx("min-w-0 flex-1 overflow-hidden", add && "bg-add-bg", del && "bg-del-bg")}>
              <span className="inline-block w-[19px] text-center text-subtle">{add ? "+" : del ? "−" : ""}</span>
              <Code text={line.text} />
            </span>
          </div>
        );
      })}
    </div>
  );
}
