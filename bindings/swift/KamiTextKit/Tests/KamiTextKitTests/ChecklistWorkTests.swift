import Foundation
import Testing
@testable import KamiTextKit
#if canImport(UIKit)
import UIKit
#else
import AppKit
#endif

@Suite(.serialized) @MainActor
struct ChecklistWorkTests {
    private func source(count: Int, mixed: Bool) -> String {
        (0..<count).map { i in
            mixed && i % 4 == 0 ? "- [x] parent \(i)\n  - [ ] child\n\n" : "- [x] completed item \(i)\n"
        }.joined() + "\nfinal body"
    }

    private func append(_ sync: KamiTextSync, _ storage: NSTextStorage) {
        let end = storage.length
        sync.willChange(range: NSRange(location: end, length: 0), replacement: "!", storageLength: end, isComposing: false)
        storage.replaceCharacters(in: NSRange(location: end, length: 0), with: "!")
        sync.didChange(text: storage.string, storage: storage, selectedRange: NSRange(location: end + 1, length: 0), isComposing: false)
    }

    @Test func releaseChecklistSamples() throws {
        for (count, mixed) in [(250, false), (2000, false), (1000, true)] {
            let text = source(count: count, mixed: mixed)
            for sample in 0..<6 {
                let sync = KamiTextSync()
                let storage = NSTextStorage()
                let clock = ContinuousClock()
                let start = clock.now
                sync.seed(text: text, storage: storage, selectedRange: NSRange(location: 0, length: 0))
                let seeded = clock.now
                append(sync, storage)
                let finished = clock.now
                #expect(try sync.engine?.text() == storage.string)
                print("CHECKLIST count=\(count) mixed=\(mixed) sample=\(sample) seed=\(start.duration(to: seeded)) finalEdit=\(seeded.duration(to: finished))")
            }
        }
    }

    #if DEBUG
    @Test(arguments: [128, 256]) func deterministicWorkIsBounded(count: Int) throws {
        let sync = KamiTextSync()
        let storage = NSTextStorage()
        sync.seed(text: source(count: count, mixed: false), storage: storage, selectedRange: NSRange(location: 0, length: 0))
        let seedVisits = sync.checkedParagraphVisits
        append(sync, storage)
        print("CHECKLIST-WORK count=\(count) paragraphs=\(seedVisits) comparisons=\(sync.checkedCoverageComparisons)")
        #expect(seedVisits <= 4 * count)
        #expect(sync.checkedCoverageComparisons <= 4 * count)
        #expect(try sync.engine?.text() == storage.string)
    }
    #endif
}
