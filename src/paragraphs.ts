// Acoustic windows are transport/decode boundaries, not semantic boundaries.
// Paragraphs normally close only after enough text has accumulated *and* the
// latest window ends a sentence. A 12-window safety ceiling prevents a model
// that emits no punctuation from creating one unbounded display block.

const MIN_PARAGRAPH_CHARS = 240;
const MIN_WINDOWS_PER_PARAGRAPH = 4;
const HARD_MAX_WINDOWS_PER_PARAGRAPH = 12;

function endsSentence(text: string): boolean {
  return /[.!?…][”"')\]»]*$/.test(text.trim());
}

/** The single definition of "what counts as closed", applied by
 * `groupWindowsIntoParagraphs` to each candidate paragraph as windows are
 * appended — WP-100. This is a display-grouping concern only: since WP-103,
 * Live Translation triggers per window and no longer waits on a paragraph
 * closing (see StreamingView.tsx's reconcile effect). */
function paragraphMeetsCloseCondition<
  W extends { text: string; outcome_ok: boolean },
>(paragraph: W[]): boolean {
  const chars = paragraph.reduce((sum, w) => sum + w.text.length, 0);
  const last = paragraph[paragraph.length - 1];
  const longEnough = chars >= MIN_PARAGRAPH_CHARS;
  const enoughWindows = paragraph.length >= MIN_WINDOWS_PER_PARAGRAPH;
  const atSentenceEnd =
    last !== undefined && last.outcome_ok && endsSentence(last.text);
  return (
    ((longEnough || enoughWindows) && atSentenceEnd) ||
    paragraph.length >= HARD_MAX_WINDOWS_PER_PARAGRAPH
  );
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
