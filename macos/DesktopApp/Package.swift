// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "AutomationDesktopApp",
    platforms: [.macOS(.v14)],
    products: [
        .executable(name: "AutomationDesktopApp", targets: ["AutomationDesktopApp"])
    ],
    targets: [
        .executableTarget(name: "AutomationDesktopApp"),
        .testTarget(
            name: "AutomationDesktopAppTests",
            dependencies: ["AutomationDesktopApp"]
        )
    ]
)
