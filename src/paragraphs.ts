// Windows carry no pause/VAD signal (fixed ~7s slices, not silence-delimited
// — see streaming_session.rs), so paragraphs are inferred from a
// sentence-boundary + length heuristic instead, with a window-count cap so an
// un-punctuated stretch can't grow forever.

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
