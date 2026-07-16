# Platform Bugs

TextKit/AppKit/UIKit quirk ledger — one file for platform-behavior bugs that would
otherwise stay scattered across gotchas, test comments, and research docs. Check
here before re-investigating an AppKit/TextKit oddity; add an entry (or a
negative-findings entry) for anything new before "fixing" it.

## Status vocabulary

- **Active-workaround** — confirmed platform bug; a workaround is live in the code today.
- **Documented-unsupported** — confirmed platform limitation; no workaround exists, behavior is documented as unsupported instead.
- **Deferred-risk** — confirmed exposure with no live trigger yet; tracked with an explicit revive condition instead of being fixed pre-emptively.
- **Fixed-upstream** — was a platform bug; Apple has since fixed it in a recorded OS/framework version; the workaround is a removal candidate.
- **Not-a-bug** — investigated and confirmed correct/by-design behavior. Lives in [Negative findings](#negative-findings), not as a live entry.

## Entry format

Title · **Status** (from the vocabulary above) · **Affected**: OS/framework + versions
observed (`not recorded` is acceptable for seed entries — recording versions is the
retest task, not new investigation) · **Symptom** · **Workaround**: exact recipe with
a code pointer · **Upstream**: FB number if filed, else `none filed` · **Retest**: date.

## Entries

### 1. `.layoutManager` access silently downgrades TextKit 2 → TextKit 1

- **Status**: Active-workaround
- **Affected**: macOS/iOS, AppKit `NSTextView` / UIKit `UITextView` on TextKit 2. Versions: not recorded.
- **Symptom**: Reading a text view's `.layoutManager` property, even just to check it, silently drops the view out of TextKit 2 compatibility mode into TextKit 1 — every downstream assumption the applier makes about TextKit 2 stops holding.
- **Workaround**: Never touch `.layoutManager`; `.textStorage` is the only surface `KamiTextStorageApplier` and hosts should use. Code pointer: `AGENTS.md:78`.
- **Upstream**: none filed.
- **Retest**: 2026-10-12.

### 2. AppKit undo fires no edit delegate — only a selection change

- **Status**: Active-workaround
- **Affected**: macOS, AppKit `NSTextView` (Cmd+Z / Cmd+Shift+Z, menu or keyboard). Versions: not recorded.
- **Symptom**: Undo/redo mutates `NSTextStorage` without firing `shouldChangeTextIn`/`textDidChange` — only a selection-change delegate call follows, so `KamiTextSync` never sees the edit through its normal `willChange`/`didChange` path.
- **Workaround**: `KamiTextSync.selectionChanged` detects the resulting `engine.lenUtf16 != storage.length` mismatch with no edit in flight and reseeds from scratch (`KamiTextSync.swift:300-311`). A length-preserving undo does not trip that guard and instead heals on the next length-changing edit, by design — see entry in [Negative findings](#negative-findings). Header doc: `KamiTextSync.swift:25-28`. Code pointer: `AGENTS.md:79`. Bulletproof coverage recipe (not wired by default): hosts add an `NSTextStorageDelegate.didProcessEditing` hook (`KamiTextSync.swift:28`).
- **Upstream**: none filed.
- **Retest**: 2026-10-12.

### 3. Stranded IME composition after an interrupted marked-text session

- **Status**: Active-workaround
- **Affected**: macOS AppKit (inline predictions, accent popup) and UIKit (same shape: `markedTextRange != nil` is the composition signal, `unmarkText()` is shared API). Versions: not recorded.
- **Symptom**: macOS inline predictions and similar features can set marked text outside CJK/accent input; if the composition is interrupted (caret move, focus loss) without a normal commit, `hasMarkedText()`/`markedTextRange` sticks and `KamiTextSync` — correctly refusing to sync mid-composition — never resyncs unless a normal `didChange(isComposing: false)` eventually arrives, which a stranded composition may never produce.
- **Workaround**: a view cannot become first responder while it holds an active composition, so focus regain is a reliable out-of-band "composition is over" signal. Host recipe: override `becomeFirstResponder()`, call `unmarkText()` if `hasMarkedText()`, then `KamiTextSync.recoverIfDesynced(text:storage:selectedRange:)`, which does a full content compare (not just length, since Hangul jamo→syllable and accent-popup replacement are length-preserving) and reseeds via `seed`. Code pointers: `KamiTextSync.swift:33-49` (header doc), `KamiTextSync.swift:341-355` (`recoverIfDesynced`); wired in `KamiDemoMac/main.swift:96-103`; tested in `bindings/swift/KamiTextKit/Tests/KamiTextKitTests/MarkedTextDesyncTests.swift` (`regainingFocusRecoversStrandedComposition` at line 35, `lengthPreservingStrandRecoversViaContentCompare` at line 69).
- **Upstream**: none filed.
- **Retest**: 2026-10-12.

### 4. AppKit `fixAttributes` font-substitution risk over 0.01pt hidden runs

- **Status**: Deferred-risk
- **Affected**: macOS/iOS, default `NSTextStorage`/`fixAttributes(in:)` attribute-fixing pass. Versions: not recorded.
- **Symptom**: AppKit's default `fixAttributes` pass runs font-substitution over the whole storage, including the 0.01pt hidden-delimiter runs `DefaultKamiTheme` uses for conceal (`KamiTheme.swift`). The substituted glyphs are harmless (hidden runs are always plain ASCII delimiters), but per the competitor precedent the real risk is the default fixing pass stripping other attributes it doesn't recognize, and a narrow-glyph-coverage custom theme's *visible* body text would additionally get no substitution where it actually needs one, since KamiTextKit has no custom `NSTextStorage` subclass to intervene.
- **Workaround**: none active — deferred. `DefaultKamiTheme`'s system fonts have broad CoreText fallback so exposure is low today. If/when KamiTextKit gains a custom `NSTextStorage` subclass, port the competitor's `fixAttributes`/`fixFontSubstitution` approach: skip runs with `font.pointSize > 1.0` only, walk by composed-character-sequence (not per UTF-16 code unit) to keep ZWJ/skin-tone emoji sequences intact. The subclass is a deliberate deferral, not an oversight: `DefaultKamiTheme`'s broad-coverage system fonts keep today's exposure low, and the two confirmed repros below define exactly what the subclass must handle when built.
- **Upstream**: none filed.
- **Revive trigger**: first consumer shipping a narrow-glyph-coverage custom `KamiTheme` — re-triage immediately if that happens, regardless of retest date.
- **Retest**: 2026-10-12.
- **Confirmed repro (2026-07-12, default theme — the "narrow theme" precondition is NOT required)**: a keycap emoji whose base character doubles as a markdown delimiter, e.g. `*before *️⃣ after*` where the emphasis-closing `*` is also the base of `*️⃣` (`* + U+FE0F + U+20E3`). The engine conceals that `*` as a delimiter byte; AppKit's fixing pass walks composed character sequences and font-substitutes the whole grapheme to Apple Color Emoji — so the two disagree, and attribute-only concealment cannot split the sequence. Found by the corpus differential fuzz (`corpora/emoji-zwj-stress.md`; the construct is now excluded from the doc with a pointer here). Consequence: concealment of a delimiter byte that participates in a composed emoji sequence is unreliable — the `NSTextStorage` subclass (or an engine-side rule refusing to conceal such delimiters) is the fix when revived.
- **Second confirmed facet (2026-07-12)**: for glyphs the system meta-font lacks under a *text-presentation* selector (U+263A/U+2714 + VS15), the concrete substitute the fixing pass stores is **history-dependent** — the same character in the same document got `Helvetica` on one `NSTextStorage` and `Menlo-Regular` on another, apparently keyed to neighboring-run fonts. Raw font-name attribute reads over such glyphs are therefore not comparable across storages. Harness consequence: `DifferentialFuzzTests` normalizes both sides with an explicit `fixAttributes(in:)` pass before comparing (comparing what renders, not pre-fix state), and the stress corpus doc avoids VS15 text-presentation forms.

### 5. Plural `shouldChangeTextInRanges` shadows the singular delegate method

- **Status**: Active-workaround
- **Affected**: macOS, AppKit `NSTextViewDelegate`. Versions: not recorded.
- **Symptom**: If an AppKit host implements both `textView(_:shouldChangeTextIn:replacementString:)` and `textView(_:shouldChangeTextInRanges:replacementStrings:)`, AppKit calls only the plural variant — a host that stashes edits solely in the singular method silently stops seeing them (multi-cursor edits, Replace All) once the plural method exists anywhere in the delegate.
- **Workaround**: implement only the plural `shouldChangeTextInRanges`; stash single-range edits (call `KamiTextSync.willChange`) and let multi-range edits fall through to `didChange`'s reseed fallback. Code pointers: `AGENTS.md:80`; reference host wiring `KamiDemoMac/main.swift:190-208`.
- **Upstream**: none filed.
- **Retest**: 2026-10-12.

### 6. UIKit undo fires `textViewDidChange` with no `shouldChangeTextIn` — recovery reseed must be attribute-only

- **Status**: Active-workaround
- **Affected**: iOS, UIKit `UITextView` undo/redo. Versions: not recorded (reported from a consumer app, 2026-07-12).
- **Symptom**: undo mutates the storage and fires `textViewDidChange` without the pre-edit hook, so `KamiTextSync` has no stash and takes its desync-recovery reseed. When that reseed replaced the whole storage via `setAttributedString`, UIKit reset the caret rendering and undo coalescing at exactly the moment the user was interacting — perceived as "the cursor disappears after undo".
- **Workaround**: `KamiTextSync.seed` restyles **in place** (attribute-only full pass, zero character edits) whenever the storage already holds the target text — true for every recovery reseed, since the storage is the source of truth and only the engine is stale. Code pointers: `KamiTextSync.swift` (`seed`, in-place branch); pinned by `CaretRecoveryTests.desyncRecoveryReseedIsAttributeOnly`.
- **Upstream**: none filed.
- **Retest**: 2026-10-12.

### 7. `UITextView` caret stops rendering after storage mutation inside the selection-change callback

- **Status**: Active-workaround (device verification pending — the mechanism is reproduced in tests, the visual repro needs a real device pass)
- **Affected**: iOS, UIKit `UITextView`. Versions: not recorded (reported from a consumer app, 2026-07-12).
- **Symptom**: reveal/conceal restyles storage attributes under the caret inside `textViewDidChangeSelection`; UIKit's selection view can leave the caret un-drawn until the next input — perceived as "the cursor disappears when moving the caret near markers".
- **Workaround**: `KamiTextSync.selectionChanged` returns whether it restyled (`@discardableResult Bool`); UIKit hosts refresh the caret after a `true` return by reassigning `textView.selectedTextRange = textView.selectedTextRange`. AppKit hosts use `updateInsertionPointStateAndRestartTimer(true)` — wired in `KamiDemoMac/main.swift` (`textViewDidChangeSelection`). Return values pinned by `CaretRecoveryTests.selectionChangedReportsRestyles`.
- **Upstream**: none filed.
- **Retest**: 2026-10-12.

## Negative findings

Investigated, confirmed not a bug — kept here so these are not re-investigated.

### Heading+strong font composition is correct by construction

**Status**: Not-a-bug. `analysis.rs`'s flatten step already unions ancestor+child kind
bits onto one segment before Swift theming ever runs (e.g. `# **word**` produces
`HEADING1|STRONG` on one segment), so `DefaultKamiTheme` never needs to reconcile two
separately-applied font sets the way an AST-descent styler would — there is no
"nested bold shrinks to inline size" class of bug to fix. Pinned by
`bindings/swift/KamiTextKit/Tests/KamiTextKitTests/KamiThemeTests.swift:29-37`
(`headingPlusStrongPreservesHeadingSizeAndAddsBoldTrait`: heading point size preserved,
bold trait added), pinned by `KamiThemeTests.swift:29-37`.

### Length-preserving undo heals on the next length change, by design

**Status**: Not-a-bug. A length-preserving AppKit undo (e.g. toggling a single
character back) fires only a selection-change delegate call, which cannot distinguish
it from a normal caret move because `engine.lenUtf16 == storage.length` still holds —
so `KamiTextSync` does not reseed for it immediately. The next length-changing edit
resyncs it correctly regardless, because that edit's own `willChange`/`didChange`
path re-derives state from the current storage content. This is accepted behavior,
not a gap to close with more machinery — see `KamiTextSync.swift:25-28` before
attempting a fix. Hosts wanting bulletproof coverage of this specific window can add
the `NSTextStorageDelegate.didProcessEditing` hook the same doc comment prescribes.

### TextKit 2: geometry queries outside the viewport degrade tap hit-testing (iOS)

**Status**: Host-side hazard, worked around in hosts — not an engine bug. With
`UITextView(usingTextLayoutManager: true)`, `UITextInput` geometry calls
(`selectionRects(for:)`, `firstRect(for:)`, …) force layout of the queried range.
TextKit 2 lays out viewport-locally; forcing layout for ranges far outside the
viewport (e.g. measuring every fenced code block in a large document) destabilizes
UITextView's tap mapping so `closestPosition(to:)` falls back to `endOfDocument` —
observed on-device (2026-07-16, KamiNotes iOS: tapping mid-document placed the caret
at the end of large notes). Small documents never reproduce it (fully laid out).
Hosts drawing range-based decorations must clamp geometry work to
`textLayoutManager.textViewportLayoutController.viewportRange` (+ a bounded margin);
reading `viewportRange` itself forces no layout. Reference implementation:
`KamiNotesIOS/Sources/Editor/CodeBlockDecoration.swift` (`visibleUTF16Range`,
band-clamped `blockRects`). WWDC21 "Meet TextKit 2" warns against off-viewport
layout reads; the exact UIKit fallback internal is inferred (closed source), but
remove-the-forcing ⇒ bug gone was confirmed by two independent reviews.
macOS/AppKit: the end-of-document jump was NOT reproduced (2026-07-16, KamiNotesMac,
4 screenshot-backed unfocused-click attempts on a 600-line note) — the same synchronous
focus reseed exists there (`EditorSession.setEditorFocused`) but AppKit's click→selection
ordering appears to shield it. Watched, not deferred; see the mac repo's DESIGN.md ledger.
