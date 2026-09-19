// Author: Lukas Bower
// Purpose: Produce a genuine XCTest result for the native release-provider reference.
// Copyright 2026 Lukas Bower
import XCTest
@testable import NativeReference
final class ReferenceTests: XCTestCase {
    func testReference() { XCTAssertEqual(referenceValue(), 27) }
}
