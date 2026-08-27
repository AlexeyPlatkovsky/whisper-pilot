// Windows carry no pause/VAD signal (fixed ~7s slices, not silence-delimited
// — see streaming_session.rs), so paragraphs are inferred from a
// sentence-boundary + length heuristic instead, with a window-count cap so an
// un-punctuated stretch can't grow forever.

const MIN_PARAGRAPH_CHARS = 240;
// Bounds worst-case latency for unbroken speech with no early sentence break.
// See TaskPilot WP-100 for the measured latency this value trades off.
const MAX_WINDOWS_PER_PARAGRAPH = 4;

function endsSentence(text: string): boolean {
  return /[.!?]["')\]]*$/.test(text.trim());
}

/** The single definition of "what counts as closed", shared by
 * `groupWindowsIntoParagraphs` (applied to each candidate paragraph as
 * windows are appended) and `isParagraphClosed` (applied once to an
 * already-accumulated paragraph) — WP-100. */
function paragraphMeetsCloseCondition<
  W extends { text: string; outcome_ok: boolean },
>(paragraph: W[]): boolean {
  const chars = paragraph.reduce((sum, w) => sum + w.text.length, 0);
  const last = paragraph[paragraph.length - 1];
  const longEnough = chars >= MIN_PARAGRAPH_CHARS;
  const atSentenceEnd =
    last !== undefined && last.outcome_ok && endsSentence(last.text);
  const tooManyWindows = paragraph.length >= MAX_WINDOWS_PER_PARAGRAPH;
  return (longEnough && atSentenceEnd) || tooManyWindows;
}

/** Whether `paragraph`, as currently accumulated, already satisfies the same
 * close condition `groupWindowsIntoParagraphs` applies per window — WP-100.
 * Lets the Streaming reconcile effect enqueue a still-trailing paragraph for
 * translation the moment it closes, instead of always waiting for a sibling
 * paragraph to start forming. */
export function isParagraphClosed<
  W extends { text: string; outcome_ok: boolean },
>(paragraph: W[]): boolean {
  return paragraph.length > 0 && paragraphMeetsCloseCondition(paragraph);
}

export function groupWindowsIntoParagraphs<
  W extends { text: string; outcome_ok: boolean },
>(windows: W[]): W[][] {
  const paragraphs: W[][] = [];
  let current: W[] = [];

  for (const window of windows) {
    current.push(window);

    if (paragraphMeetsCloseCondition(current)) {
      paragraphs.push(current);
      current = [];
    }
  }
  if (current.length > 0) paragraphs.push(current);
  return paragraphs;
}
