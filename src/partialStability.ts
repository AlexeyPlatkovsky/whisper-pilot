export interface StablePartial {
  stable: string;
  unstable: string;
}

/**
 * Promotes only text repeated by two consecutive ASR hypotheses. Everything
 * after that agreement boundary remains provisional and replaceable.
 */
export function stabilizePartial(
  previous: string,
  current: string,
): StablePartial {
  const previousWords = words(previous);
  const currentWords = words(current);
  let shared = 0;
  while (
    shared < previousWords.length &&
    shared < currentWords.length &&
    previousWords[shared] === currentWords[shared]
  ) {
    shared += 1;
  }

  return {
    stable: currentWords.slice(0, shared).join(" "),
    unstable: currentWords.slice(shared).join(" "),
  };
}

function words(text: string): string[] {
  return text.trim().split(/\s+/u).filter(Boolean);
}
