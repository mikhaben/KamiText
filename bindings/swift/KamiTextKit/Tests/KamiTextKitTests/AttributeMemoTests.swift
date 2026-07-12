import Foundation
import Testing
@testable import KamiTextKit
#if canImport(UIKit)
import UIKit
#else
import AppKit
#endif

/// Phase 3.1/3.2 regression: the applier's attribute memo is applier-lifetime
/// (a `private let memo` reference-type box), not per-call — a second apply
/// on the same applier instance must not re-invoke the theme, while a fresh
/// applier instance starts with a cold cache.
@MainActor
struct AttributeMemoTests {
    @Test func segmentAttributesAreMemoizedForApplierLifetime() throws {
        let text = "# Head\n\nBody **bold** text\n\n- [ ] a task\n"
        let engine = try KamiEngine(text: text)
        let theme = CountingTheme()
        let applier = KamiTextStorageApplier(theme: theme)
        let storage = NSTextStorage(string: text)

        try applier.applyFull(engine: engine, to: storage)
        let firstPassCount = theme.attributesCallCount
        #expect(firstPassCount > 0)

        try applier.applyFull(engine: engine, to: storage)
        #expect(theme.attributesCallCount == firstPassCount, "second apply on the same applier must not re-invoke the theme")

        let freshApplier = KamiTextStorageApplier(theme: theme)
        try freshApplier.applyFull(engine: engine, to: storage)
        #expect(theme.attributesCallCount > firstPassCount, "a fresh applier instance must start with a cold cache")
    }
}

/// Wraps `DefaultKamiTheme`, counting `attributes(for:concealed:)` calls so a
/// test can observe whether the applier's memo (not the theme) is doing the
/// caching.
private final class CountingTheme: KamiTheme {
    private let base = DefaultKamiTheme()
    private(set) var attributesCallCount = 0

    func attributes(for kinds: KamiKindSet, concealed: Bool) -> [NSAttributedString.Key: Any] {
        attributesCallCount += 1
        return base.attributes(for: kinds, concealed: concealed)
    }

    func checkedTaskOverlayAttributes() -> [NSAttributedString.Key: Any] {
        base.checkedTaskOverlayAttributes()
    }
}
