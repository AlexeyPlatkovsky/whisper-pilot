# F006 Streaming Live Translation & MFU Panel Toggle — Scenarios

## Scenarios

### F006-S1: MFU panel visibility persists independently per screen

**Covers:** `F006-R1`

```gherkin
Scenario: MFU panel state survives restart, independently per screen
  Given the MFU panel is switched off on the Meeting screen
    And left on for the Streaming screen
  When the app restarts
  Then Meeting opens with the MFU panel hidden
    And Streaming opens with the MFU panel shown
```

### F006-S2: Hidden MFU panel reveals itself when Craft MFU finishes

**Covers:** `F006-R1`

```gherkin
Scenario: Craft MFU run reveals a hidden panel
  Given the Streaming screen with the MFU panel hidden
  When Craft MFU finishes generating
  Then the MFU panel renders
    And the MFU switch reads on
```

### F006-S3: Live Translation dropdown locks while the switch is on

**Covers:** `F006-R2`

```gherkin
Scenario: Target-language dropdown is locked during translation
  Given a running Streaming session with an LLM model ready
  When the user switches Live Translation on
  Then the target-language dropdown is disabled
  When the user switches Live Translation off
  Then the target-language dropdown is enabled again
```

### F006-S4: Live Translation and Prettify are mutually exclusive

**Covers:** `F006-R2`

```gherkin
Scenario: A prettified transcript blocks Live Translation
  Given a prettified transcript is active on the Streaming screen
  When the user tries to switch Live Translation on
  Then the switch is disabled
    And it states that Prettify must be reverted first
```

### F006-S5: Switching Live Translation on splits the transcript into paired rows

**Covers:** `F006-R3`

```gherkin
Scenario: Split view renders original and translation side by side
  Given a running Streaming session in Russian with an LLM model ready
  When the user switches Live Translation on with target English
  Then the transcript area splits into a two-column paired-row grid
    And each Russian paragraph gains an English translation at the same
      vertical position as its original
```

### F006-S6: Switching Live Translation off restores the single-column view

**Covers:** `F006-R3`

```gherkin
Scenario: Turning translation off reverts the transcript layout
  Given Live Translation is on with a rendered paired-row grid
  When the user switches Live Translation off
  Then the transcript returns to its single-column rendering unchanged
```

### F006-S7: A translated paragraph is persisted and reused

**Covers:** `F006-R4`

```gherkin
Scenario: A stored translation is not re-translated
  Given an active LLM model and a Russian paragraph
  When translate_streaming_paragraph is called with target "en"
  Then it returns English text
    And a streaming_translations row exists for that session, paragraph_key,
      and target language
  When the same paragraph_key is translated again with no source-text change
  Then the existing row is updated in place rather than duplicated
```

### F006-S8: No LLM model blocks translation without writing a row

**Covers:** `F006-R4`

```gherkin
Scenario: Translation fails cleanly with no active model
  Given no active LLM model
  When translate_streaming_paragraph is called
  Then it returns an error
    And no streaming_translations row is written
```

### F006-S9: Backfill runs oldest-first with one call in flight

**Covers:** `F006-R5`

```gherkin
Scenario: Turning translation on backfills existing paragraphs in order
  Given a running Streaming session with several existing paragraphs
  When the user switches Live Translation on
  Then existing paragraphs are translated oldest-first
    And at most one translate call is in flight at any time
    And newly closing paragraphs keep being enqueued and translated as they
      arrive
```

### F006-S10: A same-language paragraph is mirrored without a model call

**Covers:** `F006-R5`

```gherkin
Scenario: An already-target-language paragraph is not translated
  Given Live Translation is on with target English
  When a paragraph whose windows are all detected as English is processed
  Then no translate call is made
    And its right cell shows the original text in a muted mirrored style
```

### F006-S11: A failed translation offers retry without interrupting capture

**Covers:** `F006-R5`

```gherkin
Scenario: A failed paragraph shows retry and the queue continues
  Given Live Translation is on
  When a paragraph's translation call fails
  Then that row shows "Translation failed · Retry" with a retry control
    And the next paragraph is still translated
    And live capture is not interrupted
```

### F006-S12: Export pairs original and translated paragraphs

**Covers:** `F006-R6`

```gherkin
Scenario: Exporting a fully translated session pairs every paragraph
  Given a Streaming session with translations for every paragraph
  When the user exports to Markdown
  Then the file contains each original paragraph followed by its labelled
    translation, in screen order
```

### F006-S13: A paragraph with no translation still exports with a placeholder

**Covers:** `F006-R6`

```gherkin
Scenario: A missing translation does not break paragraph alignment
  Given a session where one paragraph has no translation
  When the user exports
  Then that paragraph is emitted with a not-translated placeholder
    And the remaining paragraphs are unaffected
```

### F006-S14: Export is unchanged when Live Translation is off

**Covers:** `F006-R6`

```gherkin
Scenario: Untranslated export output is byte-identical to the prior behavior
  Given a session with Live Translation off
  When the user copies or exports
  Then the output is identical to the current single-language implementation
```

## Manual Verification Checklist

- [ ] (F006-R1) MFU switch is keyboard-focusable with an accessible checked
      state on both Meeting and Streaming; verified with a screen reader or
      the accessibility inspector.
- [ ] (F006-R3) With Live Translation and the MFU panel both on, the MFU
      aside keeps its fixed width and neither transcript column collapses at
      the app's minimum supported window width — manual check on a real app
      run (`docs/design.md` §Center — transcript narrow-width behavior).
- [ ] (F006-R4, F006-R5) A live Streaming session with mixed Russian/English
      audio and Live Translation on shows no dropped or failed transcription
      windows attributable to translation, verified on a real-Metal run
      (WP-91 DoD; the real-Metal transcription gate per `AGENTS.md`).
- [ ] (F006-R4) Reopening a stopped session with previously stored
      translations renders them immediately with no model call.
- [ ] (F006-R2) Each of the switch's three disabled reasons (no LLM model
      ready, prettified transcript active, Prettify review pending) is
      exposed as a readable stated reason, not just a disabled control,
      confirmed with the accessibility inspector.
