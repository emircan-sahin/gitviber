// The labels link hints (hints.ts) put on links. Pure, so it runs under `node --test`.

/** Vimium's home-row letters: no digits, as Shift+label copies. */
export const HINT_ALPHABET = "sadfjklewcmpgh";

/**
 * Kitty's prefix-free labels (hints_generator.go): label i is the i-th in breadth-first order
 * (s, a, …, ss, sa, …) after skipping enough short ones that no label starts another, so a label
 * is done the moment it's typed. The first are the shortest: hints.ts gives them to the bottom links.
 */
export function hintLabels(n: number, alphabet = HINT_ALPHABET): string[] {
  const k = alphabet.length;
  const skip = Math.floor((Math.max(n, 2) - 2) / (k - 1));
  return Array.from({ length: n }, (_, i) => {
    let label = "";
    for (let num = skip + i; num >= 0; num = Math.floor(num / k) - 1) label = alphabet[num % k] + label;
    return label;
  });
}
