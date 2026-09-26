import { cx } from "../ui.tsx";
import { tokenize, type DiffLine, type Token } from "../demo-data.ts";

// Shiki's Nord, the app's default dark syntax theme, on the app's background instead of Nord's.
const NORD: Record<NonNullable<Token["tone"]>, string> = {
  kw: "text-[#81a1c1]",
  op: "text-[#81a1c1]",
  str: "text-[#a3be8c]",
  num: "text-[#b48ead]",
  fn: "text-[#88c0d0]",
  type: "text-[#8fbcbb]",
  com: "text-[#616e88]",
  punct: "text-[#eceff4]",
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

/**
 * Monaco's inline diff as the app configures it: SF Mono 12.5 on 20px lines, two number
 * columns, a 20px sign column, tinted rows for whole added or removed lines.
 */
function Code({ text }: { text: string }) {
  return tokenize(text).map((t, j) => (
    <span key={j} className={t.tone && NORD[t.tone]}>
      {t.text}
    </span>
  ));
}

export interface FileLine {
  text: string;
  /** The gutter's change bar: a line the diff added, or one it changed. */
  bar?: "add" | "mod";
}

/** The file itself, the diff's new side: one number column, a 3px change bar, a caret while editing. */
export function FileRows({ lines, caret }: { lines: FileLine[]; caret?: number }) {
  return (
    <div className="bg-bg pt-1 font-mono text-[12.5px] leading-5 whitespace-pre text-[#d8dee9]">
      {lines.map((line, i) => (
        <div key={i} className="flex h-5">
          <span className="w-[42px] shrink-0 pr-2 text-right text-subtle">{i + 1}</span>
          <span className="w-[22px] shrink-0 pl-1.5">
            {line.bar && <span className={cx("block h-full w-[3px]", line.bar === "add" ? "bg-added" : "bg-primary")} />}
          </span>
          <span className="min-w-0 flex-1 overflow-hidden">
            <Code text={line.text} />
            {i === caret && <span className="ml-px inline-block h-4 w-px translate-y-[3px] animate-blink bg-fg" />}
          </span>
        </div>
      ))}
    </div>
  );
}

export function DiffRows({ lines, fresh }: { lines: NumberedLine[]; fresh?: number }) {
  return (
    <div className="bg-bg pt-1 font-mono text-[12.5px] leading-5 whitespace-pre text-[#d8dee9]">
      {lines.map((line, i) => {
        const add = line.kind === "+";
        const del = line.kind === "-";
        return (
          <div key={i} className={cx("flex h-5", i === fresh && "line-in")}>
            <span className="w-[42px] shrink-0 pr-2 text-right text-subtle">{line.old}</span>
            <span
              className={cx(
                "flex w-[62px] shrink-0 justify-between pr-1.5 pl-1 text-subtle",
                add && "bg-add-gutter",
                del && "bg-del-gutter",
              )}
            >
              <span className="w-[34px] text-right">{line.new}</span>
              <span className={cx(add && "text-added", del && "text-removed")}>{add ? "+" : del ? "−" : ""}</span>
            </span>
            <span className={cx("min-w-0 flex-1 overflow-hidden pl-1", add && "bg-add-bg", del && "bg-del-bg")}>
              <Code text={line.text} />
            </span>
          </div>
        );
      })}
    </div>
  );
}
