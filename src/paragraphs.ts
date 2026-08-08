/** Groups Streaming's fixed-size decode windows into paragraphs for
 * readability. Windows carry no pause/VAD signal (see `streaming_session.rs`
 * — they're fixed ~7s slices of continuous audio, not silence-delimited), so
 * there is no natural "paragraph break" in the data. This applies a
 * sentence-boundary + length heuristic instead: start a new paragraph once
 * the accumulated text both reaches a minimum length and ends a sentence, or
 * once too many windows have piled up without ever hitting a sentence end
 * (a run-on span, or a stretch of fail-open windows) — otherwise a single
 * un-punctuated stretch could grow forever. */

const MIN_PARAGRAPH_CHARS = 240;
const MAX_WINDOWS_PER_PARAGRAPH = 6;

function endsSentence(text: string): boolean {
  return /[.!?]["')\]]*$/.test(text.trim());
}

export function groupWindowsIntoParagraphs<
  W extends { text: string; outcome_ok: boolean },
>(windows: W[]): W[][] {
  const paragraphs: W[][] = [];
  let current: W[] = [];
  let currentChars = 0;

  for (const window of windows) {
    current.push(window);
    currentChars += window.text.length;

    const longEnough = currentChars >= MIN_PARAGRAPH_CHARS;
    const atSentenceEnd = window.outcome_ok && endsSentence(window.text);
    const tooManyWindows = current.length >= MAX_WINDOWS_PER_PARAGRAPH;

    if ((longEnough && atSentenceEnd) || tooManyWindows) {
      paragraphs.push(current);
      current = [];
      currentChars = 0;
    }
  }
  if (current.length > 0) paragraphs.push(current);
  return paragraphs;
}
