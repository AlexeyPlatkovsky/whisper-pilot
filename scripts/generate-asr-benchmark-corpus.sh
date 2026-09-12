#!/usr/bin/env bash
set -euo pipefail

output_dir="${1:-/tmp/whisper-pilot-asr-corpus}"
mkdir -p "$output_dir"

say -v Milena -r 165 -o "$output_dir/ru.aiff" \
  "Сегодня мы проверяем локальную расшифровку речи. Алексей отправит сборку номер сорок два в пятнадцать тридцать. Бюджет проекта составляет двести долларов."
say -v Samantha -r 170 -o "$output_dir/en.aiff" \
  "Today we are testing local speech transcription. Alex will send build number forty two at three thirty. The project budget is two hundred dollars."
say -v Milena -r 165 -o "$output_dir/mixed-ru.aiff" \
  "Сегодня обсуждаем новую версию приложения."
say -v Samantha -r 170 -o "$output_dir/mixed-en.aiff" \
  "Please update build W P one twenty seven and run the streaming test."

for language in ru en; do
  ffmpeg -hide_banner -loglevel error -y -i "$output_dir/$language.aiff" \
    -ac 1 -ar 16000 "$output_dir/$language.wav"
done
ffmpeg -hide_banner -loglevel error -y \
  -i "$output_dir/mixed-ru.aiff" -i "$output_dir/mixed-en.aiff" \
  -filter_complex '[0:a][1:a]concat=n=2:v=0:a=1,aresample=16000' \
  -ac 1 "$output_dir/mixed.wav"

rm "$output_dir/ru.aiff" "$output_dir/en.aiff" \
  "$output_dir/mixed-ru.aiff" "$output_dir/mixed-en.aiff"

{
  printf 'file\tduration_seconds\tsha256\treference\n'
  printf 'ru.wav\t%s\t%s\t%s\n' \
    "$(ffprobe -v error -show_entries format=duration -of default=nw=1:nk=1 "$output_dir/ru.wav")" \
    "$(shasum -a 256 "$output_dir/ru.wav" | awk '{print $1}')" \
    'Сегодня мы проверяем локальную расшифровку речи. Алексей отправит сборку номер сорок два в пятнадцать тридцать. Бюджет проекта составляет двести долларов.'
  printf 'en.wav\t%s\t%s\t%s\n' \
    "$(ffprobe -v error -show_entries format=duration -of default=nw=1:nk=1 "$output_dir/en.wav")" \
    "$(shasum -a 256 "$output_dir/en.wav" | awk '{print $1}')" \
    'Today we are testing local speech transcription. Alex will send build number forty two at three thirty. The project budget is two hundred dollars.'
  printf 'mixed.wav\t%s\t%s\t%s\n' \
    "$(ffprobe -v error -show_entries format=duration -of default=nw=1:nk=1 "$output_dir/mixed.wav")" \
    "$(shasum -a 256 "$output_dir/mixed.wav" | awk '{print $1}')" \
    'Сегодня обсуждаем новую версию приложения. Please update build W P one twenty seven and run the streaming test.'
} > "$output_dir/manifest.tsv"
echo "Generated fixed ASR corpus in $output_dir"
