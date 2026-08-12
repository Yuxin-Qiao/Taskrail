import XCTest
@testable import AutomationDesktopApp

final class ModelDecodingTests: XCTestCase {
    func testAnyCodablePreservesPrimitiveDisplayValues() throws {
        let data = Data(#"{"text":"hello","number":1.5,"flag":true,"empty":null}"#.utf8)
        let values = try JSONDecoder().decode([String: AnyCodable].self, from: data)

        XCTAssertEqual(values["text"]?.value, "hello")
        XCTAssertEqual(values["number"]?.value, "1.5")
        XCTAssertEqual(values["flag"]?.value, "true")
        XCTAssertEqual(values["empty"]?.value, "null")
    }

    func testRunLogsDecodeUsesDaemonSnakeCaseFields() throws {
        let data = Data(#"{"run_id":"run-1","automation_id":"hello","status":"succeeded","stdout":"ok\n","stderr":""}"#.utf8)
        let logs = try JSONDecoder().decode(RunLogsSummary.self, from: data)

        XCTAssertEqual(logs.id, "run-1")
        XCTAssertEqual(logs.automationID, "hello")
        XCTAssertEqual(logs.status, "succeeded")
        XCTAssertEqual(logs.stdout, "ok\n")
    }
}
