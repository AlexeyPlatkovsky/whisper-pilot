export type DiffSpan = { type: "same" | "del" | "add"; text: string };

function tokenize(s: string): string[] {
  return s.match(/\S+|\s+/g) ?? [];
}

/** Word/whitespace-token diff via the standard LCS dynamic-programming
 * approach. O(n*m) time and space, deemed acceptable for a Streaming
 * transcript's bounded size (see WP-75's Constraints). */
export function computeWordDiff(original: string, updated: string): DiffSpan[] {
  const a = tokenize(original);
  const b = tokenize(updated);
  const n = a.length;
  const m = b.length;

  const dp: number[][] = Array.from({ length: n + 1 }, () =>
    new Array<number>(m + 1).fill(0),
  );
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      dp[i][j] =
        a[i] === b[j]
          ? dp[i + 1][j + 1] + 1
          : Math.max(dp[i + 1][j], dp[i][j + 1]);
    }
  }

  const spans: DiffSpan[] = [];
  let i = 0;
  let j = 0;
  while (i < n && j < m) {
    if (a[i] === b[j]) {
      spans.push({ type: "same", text: a[i] });
      i++;
      j++;
    } else if (dp[i + 1][j] >= dp[i][j + 1]) {
      spans.push({ type: "del", text: a[i] });
      i++;
    } else {
      spans.push({ type: "add", text: b[j] });
      j++;
    }
  }
  while (i < n) {
    spans.push({ type: "del", text: a[i] });
    i++;
  }
  while (j < m) {
    spans.push({ type: "add", text: b[j] });
    j++;
  }
  return spans;
}
