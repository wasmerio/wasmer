//
//  DylibExampleTests.swift
//  DylibExampleTests
//
//  Created by Nathan Horrigan on 15/08/2021.
//

import XCTest
@testable import DylibExample

class DylibExampleTests: XCTestCase {

    override func setUpWithError() throws {
        // Put setup code here. This method is called before the invocation of each test method in the class.
    }

    override func tearDownWithError() throws {
        // Put teardown code here. This method is called after the invocation of each test method in the class.
    }

    func testExample() throws {
        let sum = calculate_sum(5, 2)
        assert(sum == 7, "WASM loaded successfully")
    }

    func testPerformanceExample() throws {
        // This is an example of a performance test case.
        self.measure {
            // Put the code you want to measure the time of here.
        }
    }

}
