const SPEAKER_COLOR_COUNT = 10;

export function speakerColorClass(speakerId: number): string {
  const index =
    ((speakerId % SPEAKER_COLOR_COUNT) + SPEAKER_COLOR_COUNT) %
    SPEAKER_COLOR_COUNT;
  return `wp-speaker-color-${index}`;
}

export function speakerLabel(speakerId: number): string {
  return `Speaker ${speakerId + 1}`;
}
