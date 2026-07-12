---
name: Bug report
about: Report incorrect styling, concealment, or patch behavior
title: ''
labels: bug
assignees: ''
---

**Platform**
iOS or macOS (Catalyst counts as macOS), plus OS version.

**Engine version / commit**
The `KamiCore.xcframework` build's source commit, or the tag/version if you built from a release.

**Minimal markdown input + ops to reproduce**
The smallest document that shows the bug, plus the exact sequence of edits/selection moves/taps that trigger it (e.g. "type `**bo`, then move caret to offset 0").

```markdown
<!-- input here -->
```

**Expected vs actual styling**
What the engine/adapter should have rendered vs. what it actually rendered. A screenshot or screen recording helps for concealment/reveal issues.

**Additional context**
Anything else relevant — reveal mode (line/element), custom `KamiTheme`, etc.
