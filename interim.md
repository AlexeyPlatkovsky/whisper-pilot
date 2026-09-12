# Interim review: транскрибация, Streaming и режим Recorder

Дата: 2026-09-11  
Ветка: `feat/wp-104-cloud-streaming-byok`  
Ревизия: `96839a65a5480479f14fec4b33313c5bad597b95`

## Резюме

У WhisperPilot уже хорошая local-first основа: тяжёлая нативная работа вынесена из async reactor, контекст Whisper кэшируется, Meeting и локальный Streaming не могут одновременно использовать один Whisper, модели скачиваются с проверкой SHA-256, diarization изолирована в дочернем процессе, а обычный транскрипт сохраняется до запуска менее надёжного этапа diarization. Автотестов много.

Режим диктофона/надиктовки следует делать **внутри WhisperPilot**, а не отдельным приложением. Он повторно использует почти все дорогие части: Tauri/Rust lifecycle, управление моделями, Whisper, события, SQLite, настройки, редактор транскрипта и экспорт. Отдельное приложение продублирует эти подсистемы и создаст конкуренцию за модели, микрофон, глобальный хоткей и хранилище.

Но Recorder нельзя добавлять как ещё один большой условный React-компонент поверх текущей архитектуры. Сначала состояние аудиозахвата и ASR-сессии должно стать Rust-owned сервисом, переживающим переключение вкладок, скрытие главного окна и показ плавающего окна. Главный найденный дефект: активный Streaming сейчас можно размонтировать переходом в Meeting, при этом Rust продолжает захват. После возврата UI считает, что запись остановлена, не может выполнить Stop, а новый Start отвергается backend.

Рекомендуемый порядок:

1. Стабилизировать lifecycle существующего Streaming и сохранение хвоста аудио.
2. Ввести общий Rust capture coordinator и отдельный адаптер микрофона.
3. Добавить вкладку Recorder и сохраняемые сессии диктофона.
4. Поверх coordinator добавить плавающий кружок и глобальный хоткей.
5. Qwen3-ASR подключать только через абстракцию ASR engine и после реального бенчмарка на Apple Silicon.

Production-код в рамках ревью не менялся. Добавлен только этот документ.

## Что фактически реализовано сейчас

### Meeting: транскрибация аудио/видео

- FFmpeg целиком декодирует выбранный файл в 16 kHz mono PCM в памяти (`src-tauri/src/audio.rs:71-133`).
- Whisper `large-v3-turbo` Q8 работает через `whisper-rs`/whisper.cpp с Metal и автоопределением языка (`src-tauri/src/transcribe.rs:85-95`, `177-205`).
- Транскрипт сохраняется до начала diarization (`src-tauri/src/commands/transcription.rs:105-161`).
- Разделение по голосам — **speaker diarization**: pyannote segmentation плюс выбираемый CAM++ или TitaNet-large embedding и кластеризация на Rust (`src-tauri/src/models/catalog.rs:53-88`, `src-tauri/src/diarize/`).
- MFU — структурированный результат локальной LLM: summary, decisions, action items, open questions и participants. В каталоге есть Qwen2.5 3B и Qwen3 4B GGUF (`src-tauri/src/models/catalog.rs:90-117`).

### Streaming: транскрибация системного аудио

- На текущей ветке Streaming захватывает **только системное аудио**, без микрофона (`src-tauri/src/streaming_audio.rs:1-4`, `32-43`).
- Аудио забирается каждые 100 ms, но локальная расшифровка не выдаётся миллисекундными чанками. Whisper декодирует фиксированные неперекрывающиеся окна по **7 секунд** (`src-tauri/src/streaming_audio.rs:16-19`, `src-tauri/src/streaming_session.rs:12-22`, `149-217`). UI-задержка — как минимум время накопления окна плюс время decode.
- Язык определяется автоматически. UI пишет EN/RU/TR, хотя сам Whisper этими тремя языками не ограничен.
- Live Translation работает локально через выбранную Qwen LLM; есть frontend-очередь по окнам и single-flight guard для переводов.
- Текущая ветка также добавляет BYOK cloud Streaming для Deepgram, AssemblyAI и OpenAI. Ключи берутся из Keychain и не попадают в URL или frontend-события.

## Приоритетные находки

| Приоритет | Находка | Влияние | Рекомендация |
|---|---|---|---|
| P0 | Active Streaming можно «осиротить» переходом в Meeting | Захват и Rust-задачи продолжаются, но после возврата Stop выключен, а Start отвергается | Сделать capture state Rust-owned и доступным для запроса; оставить единый controller живым между view; как быстрый containment — запретить смену режима во время capture |
| P0 | Stop локального Streaming выбрасывает последнее неполное окно | Может исчезнуть почти 7 секунд речи | На закрытии канала декодировать значимый хвост; покрыть границы silence/min-duration тестами |
| P1 | Жёсткие неперекрывающиеся окна по 7 секунд | Высокая задержка, обрезка слов и нестабильный RU/EN code-switching на границе | Добавить VAD/utterance segmentation либо overlap + stable-prefix reconciliation и deduplication |
| P1 | Resume cloud-сессии начинает timestamps снова с нуля | Новые окна могут пересекаться со старой шкалой времени | Брать offset из последнего сохранённого `end_ms` и прибавлять к времени нового transport |
| P1 | В аудиотрактах используются unbounded channels | При медленном decode/сети аудио может неограниченно накапливаться в RAM | Bounded queues, явная политика block/drop/fail, метрики отставания и overload event |
| P1 | Каждый MFU, Prettify и перевод заново грузит LLM | Большая стартовая задержка, повторная загрузка 1.6–2.2 GB, давление на RAM/температуру | Постоянный LLM worker/runtime, сериализованная priority queue, cancellation и кэш по модели |
| P1 | Длинные Meeting-файлы и diarization умножают аудио в памяти | Многочасовые файлы могут потребовать несколько GB до памяти моделей | File-backed/streaming PCM, убрать полный `Vec<f32>` clone, создавать segmentation windows только текущим batch |
| P1 | У MFU/Prettify нет стратегии для длинного input | Большой транскрипт поздно упадёт на лимите 16K context | Preflight token budget, chunk/map-reduce, явная информация о сокращении |
| P2 | Diarization clustering зависит от порядка, speaker count не реализован | Разделение может меняться от последовательности; известное число голосов не помогает | Бенчмарк DER/JER против AHC/spectral и настоящий fixed-count режим |
| P2 | Выбор моделей описывает task, а не engine/capabilities | Qwen3-ASR нельзя безопасно добавить «ещё одной Whisper-моделью» | `AsrEngine`, model bundles и capability metadata для streaming/timestamps/languages |
| P2 | Settings пишутся неатомарно, повреждённый JSON молча сбрасывает defaults | Crash может потерять настройки; hotkey/позиция окна усилят эффект | temp + fsync + atomic rename, backup и диагностируемая ошибка |
| P2 | Security/docs drift | `csp` равен null; architecture противоречит фактическому Meeting progress | Ограничивающий CSP и синхронизация authoritative docs при реализации |

## Подробный разбор

### 1. Lifecycle Streaming нужно исправить до плавающего UI

`StreamingView` локально владеет `isRunning` и runtime listeners (`src/StreamingView.tsx:101-133`, `337-412`). Кнопка Meeting всегда вызывает `onClose` (`src/StreamingView.tsx:1225-1229`). После этого `App` размонтирует view и сбрасывает только frontend-флаг активности (`src/App.tsx:628-645`), не вызывая `stop_streaming_session`.

Rust runtime остаётся в `AppState`, поэтому захват продолжается. После возврата новый компонент начинается с `isRunning = false`; Start получает «a Streaming session is already running», а Stop выключен (`src/StreamingView.tsx:1151-1168`, `src-tauri/src/commands/streaming.rs:305-316`, `500-512`). Текущий тест лишь фиксирует вызов `onClose`, в том числе в небезопасном состоянии (`src/StreamingView.test.tsx:892-900`).

Для Recorder одна и та же backend-сессия должна переживать:

- переходы между Meeting, Streaming и Recorder;
- скрытие главного окна;
- открытие/закрытие bubble и live-caption window;
- запуск через global shortcut;
- renderer reload или позднюю подписку на события.

Рекомендуемая модель:

```text
Rust CaptureCoordinator (source of truth)
  Idle
    -> Starting { mode, session_id }
    -> Capturing { SystemAudio | Microphone }
    -> Stopping
    -> Idle
    -> Error { code, recoverable }

main window ─┐
bubble window ├── query snapshot + state/segment events
hotkey ───────┘
```

Coordinator должен отдавать typed snapshot и монотонные state events. UI только отображает состояние. Одновременно активен максимум один live audio capture. Все async-события несут session/generation ID, чтобы старый результат не обновил новую сессию.

Быстрый containment — выключить смену режима в `Starting`, `Capturing` и `Stopping`. Это не конечное решение: при скрытом main window backend всё равно обязан жить независимо от React tree.

### 2. Окна Streaming и потеря хвоста

100 ms — только частота пересылки захваченных samples. Локальный decode начинается после 112,000 samples, то есть через 7 секунд при 16 kHz. `run_windowed_decode` вырезает точные окна без overlap. При disconnect цикл завершается, не декодируя остаток (`src-tauri/src/streaming_session.rs:184-217`). Отдельный тест прямо закрепляет выбрасывание хвоста (`src-tauri/src/streaming_session.rs:425-455`).

Рекомендуемое развитие:

- На Stop финализировать хвост, если в нём достаточно речи; VAD или консервативный duration/RMS guard должен защищать от hallucination на тишине.
- Для low-latency диктовки использовать utterance/VAD pipeline либо overlapping windows. Первый эксперимент: inference window 2–4 s, step 0.5–1.0 s и stable-prefix commit; production-значения выбрать по измерениям.
- Разделить `partial` и `committed` events. Можно менять только нестабильный suffix; committed text неизменяем.
- Протащить монотонный sample index через capture, decode и persistence. Timestamps выводить из sample position, а не времени прихода результата.
- Детектировать overload. Если real-time factor долго выше 1.0, очередь не должна незаметно расти.

Качество проверять на реальных RU, EN и mixed RU/EN наборах: first-partial latency, commit latency, RTF, WER/CER, потери/дубли на границах и RAM за 60 минут.

### 3. Timeline при resume Cloud Streaming

`CloudTransport::run` начинает `captured_samples` с нуля на каждом новом WebSocket и возвращает `end_ms` относительно этого connection (`src-tauri/src/cloud_streaming.rs:346-365`, `429-465`). При resume `drive_cloud_results` восстанавливает номер следующего окна, но задаёт `previous_end_ms = 0` (`src-tauri/src/commands/streaming.rs:202-235`). Первое resumed cloud-окно поэтому снова начинается в 0 ms и может перекрыть существующий timeline.

Resume preparation должна возвращать `starting_window_index` и `timeline_offset_ms`, прочитанные из последней committed DB row в одной транзакции. Persisted time = `offset + transport_relative_ms`. Нужен provider-independent тест: существующие окна, partial, несколько resumed finals, строго монотонное время.

Мост capture → cloud сначала использует unbounded `std::sync::mpsc::channel`, а уже затем bounded Tokio channel (`src-tauri/src/commands/streaming.rs:370-387`). Первый канал обнуляет защиту второго. Локальные capture/result channels тоже unbounded (`449-459`). Ограничение должно стоять на первой producer boundary.

### 4. Meeting decode и память

`Command::output()` полностью собирает PCM в RAM. Затем код копирует его в in-memory WAV и декодирует в `Vec<f32>` (`src-tauri/src/audio.rs:71-133`). Перед transcription весь float vector клонируется, чтобы оригинал позже ушёл в diarization (`src-tauri/src/commands/transcription.rs:68-88`).

Для двух часов 16 kHz mono один f32-вектор занимает около 461 MB decimal. Фаза PCM/WAV или clone перед Whisper приближает только аудиоданные к 1 GB, ещё до моделей. Нужны:

- FFmpeg output в temporary raw-PCM/CAF file или bounded decoder;
- прямое чтение s16le без искусственного WAV header для `hound`;
- file-backed samples/mmap или `Arc<[f32]>`, если RAM-path всё же нужен;
- отказ от полного clone только ради `spawn_blocking`;
- понятный preflight по duration/disk/RAM.

Стоит сохранить хорошие свойства: native work уже в `spawn_blocking`, пустой Whisper output отклоняется, а plain transcript коммитится до diarization, поэтому crash diarization не уничтожает результат.

### 5. Качество и память speaker diarization

Pipeline хорошо изолирует падения и имеет детерминированные тесты, но есть два ограничения:

1. Segmentation двигает 10-секундное model window на 10% и заранее материализует все overlapping audio windows (`src-tauri/src/diarize/segmentation.rs:13-16`, `66-84`, `242-278`). Временно это примерно десять копий аудио. Следует хранить start indices и материализовывать только текущий batch.
2. Embeddings распределяются online nearest-centroid threshold clustering в порядке input (`src-tauri/src/diarize/clustering.rs:26-100`). Алгоритм детерминирован, но не глобально оптимален. Положительный `speaker_count` явно отвергается (`src-tauri/src/diarize/pipeline.rs:19-30`).

До замены алгоритма нужен воспроизводимый DER/JER corpus: короткие/длинные записи, 2+ голосов, overlap, похожие голоса, шум, RU/EN. Сравнить текущий подход с agglomerative hierarchical/spectral clustering и режимом известного speaker count. Child-process boundary, heartbeat и timeout сохранить.

Сейчас каждому Whisper segment назначается один speaker по максимальному overlap. Segment через реальную смену говорящего всё равно получает один voice ID. Word timestamps или forced alignment позволят делить его по diarization boundaries, но изменение следует принимать только после измеримого выигрыша и определения edit-stability.

### 6. MFU, Prettify и Live Translation

`run_inference` при каждом вызове инициализирует llama.cpp, грузит GGUF и создаёт контекст 16K (`src-tauri/src/llm.rs:284-304`). Для Live Translation это особенно дорого: вызов возможен на каждое завершённое Streaming window. `translation_busy` сериализует переводы, но MFU/Prettify идут другими commands и могут выполняться параллельно переводу (`src-tauri/src/state.rs:52-58`, `src-tauri/src/commands/mfu.rs:38-130`).

Рекомендуемая схема:

- один постоянный `LlmRuntime` по selected model/fingerprint;
- одна bounded scheduler queue для translation, MFU и Prettify;
- приоритет interactive translation над background summary;
- cancellation при изменении source window, закрытии session или выключении перевода;
- новый logical context на job при повторном использовании loaded weights;
- общий memory budget с Whisper/Qwen ASR.

MFU и Prettify кладут весь transcript в фиксированный 16K context без preflight/chunking. Нужны token estimation, hierarchical/chunked summary и явные предупреждения. Выбор prompt сейчас работает по правилу «русский, если найден хотя бы один кириллический символ»; лучше language-agnostic prompts или надёжные language metadata.

Повреждённый MFU JSON молча превращается в summary, а остальные поля становятся пустыми (`src-tauri/src/llm.rs:102-143`). Лучше constrained JSON/schema generation, где runtime это поддерживает, затем одна bounded repair attempt. После повторной ошибки raw output можно сохранить, но UI должен показать structured-result warning.

`canPrettify` не включает `!llmModelReady`, поэтому Craft/Prettify остаются доступными без рабочей LLM (`src/StreamingView.tsx:1017-1042`, `1170-1176`, `1401-1415`). Backend ошибку обработает, но UI должен заранее отключать control с понятной причиной, как Live Translation.

### 7. Frontend, persistence, security и docs

`src/App.tsx` — 1,253 строки, `src/StreamingView.tsx` — 1,759. Lifecycle-дефект выше — одно из следствий того, что transport state, persistence orchestration, translation queues и presentation находятся внутри условного view. Нужны общий app shell, маленькие route views и hooks/controllers с pure reducers. Backend coordinator остаётся authority для native capture state.

Meeting segments используют array index как React key (`src/App.tsx:1064-1104`). Следует дать сохранённым segments стабильные IDs; это поможет partial revisions, разделению по speaker boundary и редактированию.

Ошибочный settings JSON молча сбрасывается в defaults, а запись идёт напрямую в final path (`src-tauri/src/settings.rs:128-135`, `253-257`). Recorder добавит shortcut и позиции окон, поэтому atomic persistence станет важнее.

Tauri CSP сейчас `null` (`src-tauri/tauri.conf.json:28-30`). Provider WebSockets живут в Rust, поэтому webview можно закрыть строгим CSP. Для bubble/caption нужна отдельная минимальная capability, не автоматическое наследование всех main-window IPC permissions.

Есть docs drift: `docs/architecture.md:109-112` утверждает, что Meeting не отправляет progress callback/event, но `decode_and_transcribe` вызывает `transcribe_with_progress` и шлёт `transcription_progress` (`src-tauri/src/commands/transcription.rs:68-84`).

## Recorder/dictation: рекомендуемый дизайн

### Продуктовое решение

Добавить Recorder третьим first-class режимом:

```text
Meeting | Streaming | Recorder
 file      system      microphone
 batch     live         live + optional audio recording
```

Верхнюю панель можно оставить визуально той же, но actions должны зависеть от общего capture state. Центр показывает Recorder session transcript с partial и committed text.

До разработки нужно решить один вопрос: «диктофон» означает только transcript или приложение обязано хранить, проигрывать и экспортировать исходное microphone audio? Настоящий Recorder должен сохранять аудио. Рекомендация: атомарная запись (CAF надёжен на macOS, WAV можно делать при export), обработка disk-full, восстановление оборванной записи, retention/delete/export. Если остаётся только текст, режим точнее назвать Dictation.

### Захват микрофона

Текущий source `Info.plist` содержит только ScreenCaptureKit usage description; `NSMicrophoneUsageDescription` удалён на этой system-audio-only ветке. Apple требует этот key для microphone API. Его нужно вернуть с Recorder-specific текстом и сделать явные состояния authorization/request/denied; пустой stream не должен трактоваться как denial.

Нужна абстракция `AudioSource` с отдельными macOS adapters:

- `SystemAudioSource`: существующий ScreenCaptureKit;
- `MicrophoneSource`: CoreAudio/CPAL или AVAudioEngine;
- общие normalized f32 chunks с sample rate, channels, sample index и session ID;
- resampling boundary, если hardware rate отличается от ASR rate;
- обработка device change/interruption и input-level telemetry.

На первом этапе Recorder захватывает только микрофон. Mixing microphone + system audio в Streaming лучше оставить отдельным измеряемым feature.

### Плавающий кружок 120 px

Использовать **отдельное Tauri window**, а не уменьшать main window. Так сохраняются main layout/min-size/titlebar и проще тестировать drag/click.

Переход:

1. Сохранить outer position главного окна.
2. Показать bubble 120 × 120 в top-left главного окна.
3. Только после появления bubble скрыть main.
4. Drag меняет только позицию bubble.
5. Настоящий click скрывает bubble, ставит top-left main в позицию bubble с clamp к work area, показывает и фокусирует main.
6. Если монитор исчез или scale изменился, восстановить окно в ближайшей видимой work area.

Click и drag различать по pointer-down origin и threshold 4–6 logical px. После `startDragging` подавить synthetic click. Реально проверить Retina scaling, несколько мониторов, Spaces, Stage Manager и fullscreen apps.

| Кольцо | Смысл | Доступный нецветовой сигнал |
|---|---|---|
| Зелёное | Активен microphone или system-audio capture | recording icon + «Listening» в tooltip/ARIA |
| Жёлтое | Live capture нет; batch transcription может продолжаться | idle icon + «Idle» |
| Красное | Устойчивая capture/ASR error, требующая внимания | error icon + короткий статус; click открывает детали |

Цвет не должен быть единственным сигналом. Transient validation error не должна оставлять bubble навсегда красным.

Always on Top и видимость во всех Spaces/fullscreen — разные macOS behaviors. Сначала setting должен управлять `alwaysOnTop`; если нужна видимость в каждом Space/fullscreen, это надо явно зафиксировать и настроить AppKit collection behavior. Tauri предоставляет `alwaysOnTop` и `visibleOnAllWorkspaces`, но Stage Manager/fullscreen требуют проверки на реальном macOS.

Идеально круглый transparent webview — не бесплатный config change: Tauri указывает, что macOS transparency требует `macOSPrivateApi`, а это исключает Mac App Store. Сначала надо определить канал дистрибуции. Для прямого DMG это может быть приемлемо; для App Store стоит исследовать маленький нативный AppKit `NSPanel` или приемлемый непрозрачный вариант.

Capture нельзя связывать с JavaScript timers при hidden main view. По документации Tauri default background policy может throttling timers и со временем unload скрытый view. Capture, ASR, persistence, hotkey и authoritative status должны оставаться в Rust.

### Global shortcut и live text

У Tauri есть официальный global-shortcut plugin для macOS. Регистрировать и обрабатывать shortcut лучше в Rust, чтобы он работал при hidden webviews. Нужны сохраняемый настраиваемый chord, обнаружение конфликта и показ реально зарегистрированной комбинации.

Рекомендуемое действие: одна комбинация toggle Start/Stop для Recorder. Это детерминированно и снижает риск случайной многочасовой записи. Если желаемое поведение — «hotkey только стартует, останавливает лишь UI Stop», его нужно явно выбрать до разработки.

При старте по hotkey:

- создать новую Recorder session;
- показать bubble и компактное live-caption window рядом либо временно раскрыть bubble;
- нестабильный partial визуально отделить от committed text;
- не забирать keyboard focus у текущего приложения до клика по WhisperPilot;
- показать permission/model error без focus stealing;
- Stop мгновенно прекращает capture, затем показывает короткий `finalizing` до flush хвоста.

Компактная caption panel лучше полного main window при каждом hotkey. Click по bubble по-прежнему раскрывает полный Recorder view.

## Оценка Qwen3-ASR-1.7B

Вероятно, речь о **Qwen3-ASR-1.7B**. На дату ревью официальный model card указывает Russian, English и Turkish среди 30 языков, offline и streaming recognition и language identification. Timestamps даёт отдельный Qwen3-ForcedAligner-0.6B для поддерживаемых языков. Официальный Python streaming path сейчас доступен только через vLLM и не возвращает timestamps.

Сейчас существует ggml-org GGUF conversion для свежего llama.cpp, включая Q8 около 2.17 GB. Это повышает шансы на Apple Silicon, но текущая интеграция WhisperPilot с `llama-cpp-2` — text-only path для MFU/Prettify. Qwen3-ASR требует audio projector/model bundle и более новых multimodal APIs. Одной строкой в catalog рядом с Whisper его корректно не добавить.

Рекомендуемая абстракция:

```rust
trait AsrEngine {
    fn capabilities(&self) -> AsrCapabilities;
    fn transcribe_file(&self, source: AudioSourceRef, sink: SegmentSink) -> Result<()>;
    fn start_stream(&self, config: StreamConfig, sink: SegmentSink) -> Result<StreamHandle>;
}

struct AsrCapabilities {
    streaming: bool,
    timestamps: TimestampCapability,
    language_detection: bool,
    supported_languages: Set<Language>,
    required_assets: Vec<ModelAsset>,
}
```

Перед интеграцией нужен короткий macOS spike:

- текущий Whisper large-v3-turbo Q8;
- Qwen3-ASR 0.6B и 1.7B через свежий llama.cpp GGUF path;
- при нестабильном llama.cpp API — опционально MLX-native path;
- official Transformers только как benchmark reference, не предпочтительный bundled runtime.

Не следует встраивать Python/vLLM/CUDA stack в нативное macOS-приложение. Whisper остаётся default, пока Qwen не пройдёт:

- RU, EN и mixed RU/EN WER/CER;
- RTF ниже выбранного бюджета на поддерживаемых Macs;
- acceptable first-partial/finalization latency;
- 30/60-minute memory/stability;
- предсказуемый download size и peak RAM;
- совместимость segmentation/timestamps с persistence, export и diarization.

Qwen может оказаться качественнее для диктовки, но 0.6B может быть практичнее для live partials. 1.7B можно оставить quality-вариантом для batch/final reconciliation. Решить должны измерения.

## Предлагаемые этапы

### Phase 0 — стабилизация

- Исправить orphaned Streaming lifecycle и добавить backend state snapshot.
- Flush trailing local audio на Stop.
- Исправить cloud resume timestamps.
- Ограничить capture/result queues, добавить lag telemetry.
- Кэшировать и планировать LLM runtime.
- Синхронизировать Meeting progress docs.

Exit: switch/hide/show/renderer-remount не теряют управление session; Stop сохраняет финальную речь; resumed cloud timeline монотонен; 60-minute run имеет bounded memory.

### Phase 1 — Recorder core

- Rust `CaptureCoordinator` и `MicrophoneSource`.
- Microphone usage description и permission UX.
- Решение и реализация raw-audio persistence/recovery.
- Recorder tables либо сознательно обобщённая session schema.
- Partial/commit semantics, cancellation и finalization.

Exit: Recorder работает из main window для RU, EN и mixed speech, включая denial, device removal, silence, Stop и recovery после relaunch.

### Phase 2 — третий режим и shortcut

- Typed three-mode `ModeToggle`.
- Та же верхняя панель, привязанная к coordinator state.
- Recorder history, edit/copy/export и, при выбранном scope, playback/audio export.
- Configurable Rust-owned global shortcut с conflict UX.
- Compact live-caption presentation.

Exit: shortcut Start/Stop надёжен при фокусе в другом app и hidden main; focus не крадётся без явного действия.

### Phase 3 — bubble

- Least-privilege `bubble` window и optional caption window.
- 120 px UI, status semantics, drag/click и position restore.
- Always on Top setting и определённое поведение Spaces/fullscreen.
- Выбор transparent Tauri window или native AppKit panel по каналу distribution.

Exit: real-macOS checks на Retina/non-Retina monitors, Spaces, Stage Manager, fullscreen, sleep/wake и display disconnect.

### Phase 4 — Qwen ASR

- Runtime spike и сохранённые benchmark results.
- Engine/capability-aware model catalog и cache invalidation.
- Выбранный Qwen runtime за `AsrEngine`.
- Migration/fallback при удалённой или unsupported selected model.

Exit: измеримые quality/latency/memory targets пройдены, Whisper остаётся рабочим fallback.

## Выполненная проверка

Read-only/static и automated baseline на указанной ревизии:

- `git diff --check` — passed.
- `npm run typecheck` — passed.
- `npm run lint`, `npm run lint:comments`, `npm run format:check` — passed.
- `npm run test:run` — 29 файлов, 401 тест passed.
- `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check` — passed.
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` — passed.
- `cargo test --manifest-path src-tauri/Cargo.toml` — passed; model/Metal/real-audio integration tests с `ignored` не запускались.

Rust build выдал warning о дублирующемся `@executable_path/../Frameworks` rpath. Это не падение теста, но link config стоит дедуплицировать.

Не выполнялись:

- реальный Whisper decode на representative RU/EN/code-switching media;
- long-session ScreenCaptureKit;
- реальный DER/JER diarization;
- вызовы cloud providers;
- Qwen3-ASR benchmark на Apple Silicon;
- real-window проверки bubble/Spaces/Stage Manager/fullscreen: функции ещё нет.

Поэтому отчёт оценивает структуру кода, correctness paths и test baseline, но не сертифицирует production-качество распознавания.

## Проверенные внешние первичные источники

- [Официальный репозиторий Qwen3-ASR](https://github.com/QwenLM/Qwen3-ASR)
- [Официальный model card Qwen3-ASR-1.7B](https://huggingface.co/Qwen/Qwen3-ASR-1.7B)
- [ggml-org Qwen3-ASR-1.7B GGUF](https://huggingface.co/ggml-org/Qwen3-ASR-1.7B-GGUF)
- [Tauri global shortcut plugin](https://v2.tauri.app/plugin/global-shortcut/)
- [Tauri v2 window/configuration reference](https://v2.tauri.app/reference/config/)
- [Apple `NSMicrophoneUsageDescription`](https://developer.apple.com/documentation/BundleResources/Information-Property-List/NSMicrophoneUsageDescription)
- [Apple `NSWindow.CollectionBehavior`](https://developer.apple.com/documentation/appkit/nswindow/collectionbehavior-swift.struct)

## Итоговая рекомендация

Делать Recorder внутри WhisperPilot. Функция органично относится к продукту и не требует отдельного приложения. Реализовывать её как новый capture domain за общим Rust coordinator, а не копией `StreamingView`.

Обязательная предпосылка: native session state должен жить дольше любого UI window. После этого третья вкладка, global shortcut и floating bubble становятся обычными клиентами одного сервиса, а не хрупкими special cases. Qwen3-ASR лучше подключать отдельным измеренным engine integration и не связывать с первым релизом Recorder.
